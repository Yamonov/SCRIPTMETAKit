use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, Read},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[cfg(feature = "blocking-http")]
use reqwest::blocking::Client;
use url::Url;

use crate::{
    TimestampMillis,
    core::{
        DistributionMetadata, DistributionResolution, ScriptMetaItem, ScriptMetaKitError,
        ScriptMetaKitResult, VersionOrdering, compare_versions, decode_script_text,
        parse_distribution_metadata_records, select_distribution_metadata_for_script,
    },
    now_timestamp_millis,
};

const MAX_SOURCE_CACHE_COUNT: usize = 4;
const MAX_PARSED_CACHE_COUNT: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DistributionResolverOptions {
    pub max_redirects: usize,
    /// Maximum bytes retained for one extracted SCRIPTMETA block.
    /// The source stream itself is read until a preferred block is found or EOF.
    /// Use 0 to disable this block-size guard.
    pub max_metadata_block_bytes: usize,
    pub request_timeout_millis: Option<u64>,
    /// Maximum elapsed time while streaming one metadata source.
    /// This is checked between read calls and cannot interrupt a blocked OS read.
    pub resource_timeout_millis: Option<u64>,
    pub cache_enabled: bool,
}

impl Default for DistributionResolverOptions {
    fn default() -> Self {
        Self {
            max_redirects: 8,
            max_metadata_block_bytes: 256 * 1024,
            request_timeout_millis: Some(15_000),
            resource_timeout_millis: Some(15_000),
            cache_enabled: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DistributionResolver {
    options: DistributionResolverOptions,
    cache: Arc<Mutex<DistributionResolverCache>>,
    #[cfg(feature = "blocking-http")]
    client: Client,
}

#[derive(Clone, Debug, Default)]
struct DistributionResolverCache {
    source_cache: BTreeMap<String, Arc<LoadedMetadataSource>>,
    source_order: Vec<String>,
    parsed_cache: BTreeMap<String, Arc<Vec<DistributionMetadata>>>,
    parsed_order: Vec<String>,
}

impl DistributionResolver {
    pub fn new(options: DistributionResolverOptions) -> ScriptMetaKitResult<Self> {
        #[cfg(feature = "blocking-http")]
        {
            let mut builder = Client::builder();
            if let Some(timeout) = options.request_timeout_millis {
                builder = builder.timeout(Duration::from_millis(timeout));
            }
            let client = builder
                .build()
                .map_err(|error| ScriptMetaKitError::Url(error.to_string()))?;
            Ok(Self {
                options,
                cache: Arc::new(Mutex::new(DistributionResolverCache::default())),
                client,
            })
        }

        #[cfg(not(feature = "blocking-http"))]
        {
            let _ = Duration::from_millis(options.request_timeout_millis.unwrap_or(0));
            let _ = Duration::from_millis(options.resource_timeout_millis.unwrap_or(0));
            Ok(Self {
                options,
                cache: Arc::new(Mutex::new(DistributionResolverCache::default())),
            })
        }
    }

    pub fn resolve(
        &self,
        start_url: &Url,
        script_id: &str,
        checked_at: TimestampMillis,
    ) -> ScriptMetaKitResult<DistributionResolution> {
        let mut current_url = start_url.clone();
        let mut redirect_count = 0usize;
        let mut visited_urls = BTreeSet::from([start_url.as_str().to_string()]);
        let mut latest_url_history = Vec::new();
        let mut last_redirect_url: Option<Url> = None;

        loop {
            let source = self.load_source(&current_url)?;
            let records = self.parse_distribution_records(&source)?;
            let metadata = select_distribution_metadata_for_script(&records, script_id)?;

            if let Some(metadata_script_id) = &metadata.script_id
                && metadata_script_id != script_id
                && metadata.latest_url.is_none()
            {
                return Ok(DistributionResolution {
                    latest_version: None,
                    latest_page_url: metadata.latest_page_url,
                    final_page_url: source.resolved_url.clone(),
                    latest_url_history,
                    checked_at,
                    is_unresolved: true,
                    note: Some(format!(
                        "distribution script id `{metadata_script_id}` does not match `{script_id}`"
                    )),
                    redirect_count: Some(redirect_count as u32),
                });
            }

            let next_url = metadata
                .latest_url
                .as_ref()
                .map(|url| source.resolved_url.join(url.as_str()))
                .transpose()?;

            if let Some(next_url) = next_url {
                latest_url_history.push(next_url.clone());
                let next_url_key = next_url.as_str().to_string();
                if next_url_key == current_url.as_str() {
                    let is_unresolved = metadata.latest_version.is_none();
                    return Ok(DistributionResolution {
                        latest_version: metadata.latest_version,
                        latest_page_url: metadata.latest_page_url.or(last_redirect_url),
                        final_page_url: source.resolved_url.clone(),
                        latest_url_history,
                        checked_at,
                        is_unresolved,
                        note: Some("same-page Latest-URL was ignored".to_string()),
                        redirect_count: Some(redirect_count as u32),
                    });
                }

                if visited_urls.contains(&next_url_key) {
                    let is_unresolved = metadata.latest_version.is_none();
                    return Ok(DistributionResolution {
                        latest_version: metadata.latest_version,
                        latest_page_url: metadata.latest_page_url.or(last_redirect_url),
                        final_page_url: source.resolved_url.clone(),
                        latest_url_history,
                        checked_at,
                        is_unresolved,
                        note: Some("circular Latest-URL reference was ignored".to_string()),
                        redirect_count: Some(redirect_count as u32),
                    });
                }

                if redirect_count >= self.options.max_redirects {
                    return Ok(DistributionResolution {
                        latest_version: metadata.latest_version,
                        latest_page_url: metadata.latest_page_url.or(last_redirect_url),
                        final_page_url: source.resolved_url.clone(),
                        latest_url_history,
                        checked_at,
                        is_unresolved: true,
                        note: Some("Latest-URL redirect limit exceeded".to_string()),
                        redirect_count: Some(redirect_count as u32),
                    });
                }
                redirect_count += 1;
                visited_urls.insert(next_url_key);
                last_redirect_url = Some(next_url.clone());
                current_url = next_url;
                continue;
            }

            let latest_page_url = metadata.latest_page_url.clone().or(last_redirect_url);
            let is_unresolved = metadata.latest_version.is_none();
            let note = if is_unresolved {
                Some("Latest-Version was not found".to_string())
            } else {
                metadata.note
            };

            return Ok(DistributionResolution {
                latest_version: metadata.latest_version,
                latest_page_url,
                final_page_url: source.resolved_url.clone(),
                latest_url_history,
                checked_at,
                is_unresolved,
                note,
                redirect_count: Some(redirect_count as u32),
            });
        }
    }

    fn load_source(&self, url: &Url) -> ScriptMetaKitResult<Arc<LoadedMetadataSource>> {
        let cache_key = url.as_str().to_string();
        if self.options.cache_enabled
            && let Some(source) = self.cached_source(&cache_key)?
        {
            return Ok(source);
        }

        let source = Arc::new(self.load_uncached_source(url)?);
        if self.options.cache_enabled {
            self.store_source(cache_key, Arc::clone(&source))?;
        }
        Ok(source)
    }

    fn load_uncached_source(&self, url: &Url) -> ScriptMetaKitResult<LoadedMetadataSource> {
        if is_gist_url(url) {
            return self.load_first_valid_metadata_source(
                url,
                gist_raw_content_urls(url),
                "SCRIPTMETA.txt was not found in the gist",
            );
        }
        if is_github_repository_url(url) {
            return self.load_first_valid_metadata_source(
                url,
                github_repository_raw_content_urls(url)?,
                "SCRIPTMETA.txt was not found at the root of the GitHub repository",
            );
        }
        if is_github_directory_url(url) {
            return self.load_first_valid_metadata_source(
                url,
                github_directory_raw_content_urls(url)?,
                "SCRIPTMETA.txt was not found in the GitHub directory",
            );
        }
        self.load_plain_source(url)
    }

    fn parse_distribution_records(
        &self,
        source: &LoadedMetadataSource,
    ) -> ScriptMetaKitResult<Arc<Vec<DistributionMetadata>>> {
        let cache_key = source.resolved_url.as_str().to_string();
        if self.options.cache_enabled
            && let Some(records) = self.cached_records(&cache_key)?
        {
            return Ok(records);
        }

        let records = Arc::new(parse_distribution_metadata_records(&source.text)?);
        if self.options.cache_enabled {
            self.store_records(cache_key, Arc::clone(&records))?;
        }
        Ok(records)
    }

    fn cached_source(&self, key: &str) -> ScriptMetaKitResult<Option<Arc<LoadedMetadataSource>>> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("distribution cache is poisoned".to_string()))?;
        let source = cache.source_cache.get(key).cloned();
        if source.is_some() {
            touch_cache_key(&mut cache.source_order, key);
        }
        Ok(source)
    }

    fn store_source(
        &self,
        key: String,
        source: Arc<LoadedMetadataSource>,
    ) -> ScriptMetaKitResult<()> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("distribution cache is poisoned".to_string()))?;
        touch_cache_key(&mut cache.source_order, &key);
        cache.source_cache.insert(key, source);
        evict_source_cache_entries(&mut cache);
        Ok(())
    }

    fn cached_records(
        &self,
        key: &str,
    ) -> ScriptMetaKitResult<Option<Arc<Vec<DistributionMetadata>>>> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("distribution cache is poisoned".to_string()))?;
        let records = cache.parsed_cache.get(key).cloned();
        if records.is_some() {
            touch_cache_key(&mut cache.parsed_order, key);
        }
        Ok(records)
    }

    fn store_records(
        &self,
        key: String,
        records: Arc<Vec<DistributionMetadata>>,
    ) -> ScriptMetaKitResult<()> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("distribution cache is poisoned".to_string()))?;
        touch_cache_key(&mut cache.parsed_order, &key);
        cache.parsed_cache.insert(key, records);
        evict_parsed_cache_entries(&mut cache);
        Ok(())
    }

    fn load_plain_source(&self, url: &Url) -> ScriptMetaKitResult<LoadedMetadataSource> {
        match url.scheme() {
            "file" => Ok(LoadedMetadataSource {
                text: self.load_file(url)?,
                resolved_url: url.clone(),
            }),
            "http" | "https" => self.load_http(url),
            scheme => Err(ScriptMetaKitError::Url(format!(
                "unsupported Meta-URL scheme `{scheme}`"
            ))),
        }
    }

    fn load_first_valid_metadata_source(
        &self,
        original_url: &Url,
        candidate_urls: Vec<Url>,
        not_found_message: &'static str,
    ) -> ScriptMetaKitResult<LoadedMetadataSource> {
        let mut first_error: Option<ScriptMetaKitError> = None;
        for candidate_url in candidate_urls {
            match self.load_plain_source(&candidate_url) {
                Ok(source) => {
                    return Ok(LoadedMetadataSource {
                        text: source.text,
                        resolved_url: original_url.clone(),
                    });
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        if let Some(error @ ScriptMetaKitError::NotImplemented(_)) = first_error {
            return Err(error);
        }

        Err(ScriptMetaKitError::Parse(not_found_message.to_string()))
    }

    fn load_file(&self, url: &Url) -> ScriptMetaKitResult<String> {
        let path = url
            .to_file_path()
            .map_err(|_| ScriptMetaKitError::Url("invalid file:// Meta-URL".to_string()))?;
        let mut file = File::open(&path).map_err(|error| ScriptMetaKitError::Io {
            path: path.clone(),
            message: error.to_string(),
        })?;
        read_metadata_source_block(
            &mut file,
            self.options.max_metadata_block_bytes,
            self.options.resource_timeout_millis,
        )
        .map_err(|error| {
            map_metadata_source_error(error, |error| ScriptMetaKitError::Io {
                path,
                message: error.to_string(),
            })
        })
    }

    #[cfg(feature = "blocking-http")]
    fn load_http(&self, url: &Url) -> ScriptMetaKitResult<LoadedMetadataSource> {
        let mut response = self
            .client
            .get(url.as_str())
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|error| ScriptMetaKitError::Url(error.to_string()))?;
        let resolved_url = response.url().clone();
        let text = read_metadata_source_block(
            &mut response,
            self.options.max_metadata_block_bytes,
            self.options.resource_timeout_millis,
        )
        .map_err(|error| {
            map_metadata_source_error(error, |error| ScriptMetaKitError::Url(error.to_string()))
        })?;
        Ok(LoadedMetadataSource { text, resolved_url })
    }

    #[cfg(not(feature = "blocking-http"))]
    fn load_http(&self, _url: &Url) -> ScriptMetaKitResult<LoadedMetadataSource> {
        Err(ScriptMetaKitError::NotImplemented(
            "HTTP/HTTPS Meta-URL resolution requires the `blocking-http` feature".to_string(),
        ))
    }
}

#[derive(Clone, Debug)]
struct LoadedMetadataSource {
    text: String,
    resolved_url: Url,
}

fn touch_cache_key(order: &mut Vec<String>, key: &str) {
    order.retain(|cached_key| cached_key != key);
    order.push(key.to_string());
}

fn evict_source_cache_entries(cache: &mut DistributionResolverCache) {
    if MAX_SOURCE_CACHE_COUNT == 0 {
        cache.source_cache.clear();
        cache.source_order.clear();
        return;
    }

    while cache.source_order.len() > MAX_SOURCE_CACHE_COUNT {
        let evicted = cache.source_order.remove(0);
        cache.source_cache.remove(&evicted);
    }
}

fn evict_parsed_cache_entries(cache: &mut DistributionResolverCache) {
    if MAX_PARSED_CACHE_COUNT == 0 {
        cache.parsed_cache.clear();
        cache.parsed_order.clear();
        return;
    }

    while cache.parsed_order.len() > MAX_PARSED_CACHE_COUNT {
        let evicted = cache.parsed_order.remove(0);
        cache.parsed_cache.remove(&evicted);
    }
}

const SOURCE_READ_CHUNK_BYTES: usize = 16 * 1024;
const SOURCE_MARKER_TAIL_BYTES: usize = b"SCRIPTMETA-DIST-BEGIN".len() - 1;
const DIST_BEGIN_BYTES: &[u8] = b"SCRIPTMETA-DIST-BEGIN";
const DIST_END_BYTES: &[u8] = b"SCRIPTMETA-DIST-END";
const BODY_BEGIN_BYTES: &[u8] = b"<body";

#[derive(Debug)]
enum MetadataSourceReadError {
    Io(io::Error),
    Timeout(String),
    Parse(String),
}

impl From<io::Error> for MetadataSourceReadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

fn map_metadata_source_error(
    error: MetadataSourceReadError,
    io_error: impl FnOnce(io::Error) -> ScriptMetaKitError,
) -> ScriptMetaKitError {
    match error {
        MetadataSourceReadError::Io(error) => io_error(error),
        MetadataSourceReadError::Timeout(message) => ScriptMetaKitError::Timeout(message),
        MetadataSourceReadError::Parse(message) => ScriptMetaKitError::Parse(message),
    }
}

fn read_metadata_source_block(
    reader: &mut impl Read,
    max_block_bytes: usize,
    resource_timeout_millis: Option<u64>,
) -> Result<String, MetadataSourceReadError> {
    let mut chunk = [0_u8; SOURCE_READ_CHUNK_BYTES];
    let started = Instant::now();
    let timeout = resource_timeout_millis.map(Duration::from_millis);
    let mut body_tail = Vec::with_capacity(SOURCE_MARKER_TAIL_BYTES);
    let mut body_search_buffer =
        Vec::with_capacity(SOURCE_READ_CHUNK_BYTES + SOURCE_MARKER_TAIL_BYTES);
    let mut before_body_scanner = MetadataBlockScanner::new(max_block_bytes);
    let mut body_scanner = MetadataBlockScanner::new(max_block_bytes);
    let mut fallback_block: Option<Vec<u8>> = None;
    let mut found_body = false;

    loop {
        check_resource_timeout(started, timeout)?;
        let read_len = reader.read(&mut chunk)?;
        check_resource_timeout(started, timeout)?;
        if read_len == 0 {
            break;
        }
        let bytes = &chunk[..read_len];

        if found_body {
            if let Some(block) = body_scanner.feed(bytes)? {
                return Ok(metadata_block_to_string(block));
            }
            continue;
        }

        body_search_buffer.clear();
        body_search_buffer.extend_from_slice(&body_tail);
        body_search_buffer.extend_from_slice(bytes);

        if let Some(body_index) =
            find_ascii_case_insensitive_bytes(&body_search_buffer, BODY_BEGIN_BYTES)
        {
            let current_body_start = body_index.saturating_sub(body_tail.len());
            if fallback_block.is_none()
                && let Some(block) = before_body_scanner.feed(&bytes[..current_body_start])?
            {
                fallback_block = Some(block);
            }

            found_body = true;
            if let Some(block) = body_scanner.feed(&body_search_buffer[body_index..])? {
                return Ok(metadata_block_to_string(block));
            }
            continue;
        }

        if fallback_block.is_none()
            && let Some(block) = before_body_scanner.feed(bytes)?
        {
            fallback_block = Some(block);
        }
        update_search_tail(&mut body_tail, &body_search_buffer);
    }

    if !found_body && let Some(block) = fallback_block {
        return Ok(metadata_block_to_string(block));
    }

    Err(MetadataSourceReadError::Parse(
        "missing SCRIPTMETA distribution block".to_string(),
    ))
}

fn check_resource_timeout(
    started: Instant,
    timeout: Option<Duration>,
) -> Result<(), MetadataSourceReadError> {
    if let Some(timeout) = timeout
        && started.elapsed() >= timeout
    {
        return Err(MetadataSourceReadError::Timeout(format!(
            "metadata source read timed out after {} ms",
            timeout.as_millis()
        )));
    }
    Ok(())
}

struct MetadataBlockScanner {
    max_block_bytes: usize,
    tail: Vec<u8>,
    search_buffer: Vec<u8>,
    dist_block: Option<Vec<u8>>,
}

impl MetadataBlockScanner {
    fn new(max_block_bytes: usize) -> Self {
        Self {
            max_block_bytes,
            tail: Vec::with_capacity(SOURCE_MARKER_TAIL_BYTES),
            search_buffer: Vec::with_capacity(SOURCE_READ_CHUNK_BYTES + SOURCE_MARKER_TAIL_BYTES),
            dist_block: None,
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> Result<Option<Vec<u8>>, MetadataSourceReadError> {
        if bytes.is_empty() {
            return Ok(None);
        }

        if let Some(block) = self.dist_block.as_mut() {
            let search_start = block
                .len()
                .saturating_sub(DIST_END_BYTES.len().saturating_sub(1));
            append_metadata_bytes(block, bytes, self.max_block_bytes, "SCRIPTMETA-DIST")?;
            if let Some(end_offset) =
                find_ascii_case_insensitive_bytes(&block[search_start..], DIST_END_BYTES)
            {
                let end_index = search_start + end_offset + DIST_END_BYTES.len();
                block.truncate(end_index);
                let block = self.dist_block.take().expect("active distribution block");
                return Ok(Some(block));
            }
            return Ok(None);
        }

        self.search_buffer.clear();
        self.search_buffer.extend_from_slice(&self.tail);
        self.search_buffer.extend_from_slice(bytes);

        if let Some(begin_index) =
            find_ascii_case_insensitive_bytes(&self.search_buffer, DIST_BEGIN_BYTES)
        {
            let mut block = metadata_block_from_bytes(
                &self.search_buffer[begin_index..],
                self.max_block_bytes,
                "SCRIPTMETA-DIST",
            )?;
            if let Some(end_offset) =
                find_ascii_case_insensitive_bytes(&block[DIST_BEGIN_BYTES.len()..], DIST_END_BYTES)
            {
                let end_index = DIST_BEGIN_BYTES.len() + end_offset + DIST_END_BYTES.len();
                block.truncate(end_index);
                return Ok(Some(block));
            }
            self.dist_block = Some(block);
            return Ok(None);
        }

        update_search_tail(&mut self.tail, &self.search_buffer);
        Ok(None)
    }
}

fn metadata_block_from_bytes(
    bytes: &[u8],
    max_block_bytes: usize,
    label: &str,
) -> Result<Vec<u8>, MetadataSourceReadError> {
    if max_block_bytes != 0 && bytes.len() > max_block_bytes {
        return Err(MetadataSourceReadError::Parse(format!(
            "{label} block exceeds {max_block_bytes} bytes"
        )));
    }
    let mut block = Vec::with_capacity(bytes.len());
    block.extend_from_slice(bytes);
    Ok(block)
}

fn append_metadata_bytes(
    block: &mut Vec<u8>,
    bytes: &[u8],
    max_block_bytes: usize,
    label: &str,
) -> Result<(), MetadataSourceReadError> {
    if max_block_bytes != 0 && block.len().saturating_add(bytes.len()) > max_block_bytes {
        return Err(MetadataSourceReadError::Parse(format!(
            "{label} block exceeds {max_block_bytes} bytes"
        )));
    }
    block.extend_from_slice(bytes);
    Ok(())
}

fn metadata_block_to_string(bytes: Vec<u8>) -> String {
    decode_script_text(&bytes)
}

fn update_search_tail(tail: &mut Vec<u8>, source: &[u8]) {
    tail.clear();
    let start = source.len().saturating_sub(SOURCE_MARKER_TAIL_BYTES);
    tail.extend_from_slice(&source[start..]);
}

fn find_ascii_case_insensitive_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

fn is_gist_url(url: &Url) -> bool {
    url.host_str()
        .is_some_and(|host| host.to_ascii_lowercase().contains("gist.github.com"))
}

fn is_github_repository_url(url: &Url) -> bool {
    url.host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("github.com"))
        && normalized_path_components(url).len() == 2
}

fn is_github_directory_url(url: &Url) -> bool {
    if !url
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("github.com"))
    {
        return false;
    }
    let components = normalized_path_components(url);
    components.len() >= 5 && components[2] == "tree" && !components[3].is_empty()
}

fn normalized_path_components(url: &Url) -> Vec<&str> {
    url.path_segments()
        .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
        .unwrap_or_default()
}

fn gist_raw_content_urls(url: &Url) -> Vec<Url> {
    let components = normalized_path_components(url);
    if components.len() < 2 {
        return Vec::new();
    }
    let user = components[0];
    let gist_id = components[1];
    [
        format!("https://gist.githubusercontent.com/{user}/{gist_id}/raw/SCRIPTMETA.txt"),
        format!("https://gist.githubusercontent.com/{user}/{gist_id}/raw/scriptmeta.txt"),
        format!("https://gist.github.com/{user}/{gist_id}/raw/SCRIPTMETA.txt"),
        format!("https://gist.github.com/{user}/{gist_id}/raw/scriptmeta.txt"),
    ]
    .into_iter()
    .filter_map(|url| Url::parse(&url).ok())
    .collect()
}

fn github_repository_raw_content_urls(url: &Url) -> ScriptMetaKitResult<Vec<Url>> {
    let components = normalized_path_components(url);
    if components.len() != 2 {
        return Err(ScriptMetaKitError::Url(
            "invalid GitHub repository Meta-URL".to_string(),
        ));
    }
    let owner = components[0];
    let repo = components[1];
    Ok(["HEAD", "main", "master"]
        .into_iter()
        .filter_map(|ref_name| {
            Url::parse(&format!(
                "https://raw.githubusercontent.com/{owner}/{repo}/{ref_name}/SCRIPTMETA.txt"
            ))
            .ok()
        })
        .collect())
}

fn github_directory_raw_content_urls(url: &Url) -> ScriptMetaKitResult<Vec<Url>> {
    let components = normalized_path_components(url);
    if components.len() < 5 || components[2] != "tree" || components[3].is_empty() {
        return Err(ScriptMetaKitError::Url(
            "invalid GitHub directory Meta-URL".to_string(),
        ));
    }

    let owner = components[0];
    let repo = components[1];
    let branch = components[3];
    let directory = components[4..].join("/");
    let url = Url::parse(&format!(
        "https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{directory}/SCRIPTMETA.txt"
    ))?;
    Ok(vec![url])
}

#[derive(Clone, Debug)]
pub struct UpdateResolver {
    distribution_resolver: DistributionResolver,
}

impl UpdateResolver {
    pub fn new(options: DistributionResolverOptions) -> ScriptMetaKitResult<Self> {
        Ok(Self {
            distribution_resolver: DistributionResolver::new(options)?,
        })
    }

    pub fn resolve_item(&self, item: &ScriptMetaItem) -> ScriptMetaKitResult<ResolvedItemUpdate> {
        let checked_at = now_timestamp_millis();
        let Some(current_version) = item.version.as_deref() else {
            return Ok(ResolvedItemUpdate::not_checkable(checked_at));
        };
        let Some(meta_url) = item.meta_url.as_ref() else {
            return Ok(ResolvedItemUpdate::not_checkable(checked_at));
        };

        let resolution =
            self.distribution_resolver
                .resolve(meta_url, &item.script_id, checked_at)?;
        let status = if resolution.is_unresolved {
            crate::UpdateStatus::Failed
        } else {
            match resolution.latest_version.as_deref() {
                Some(latest) => match compare_versions(current_version, latest) {
                    VersionOrdering::Less => crate::UpdateStatus::UpdateAvailable,
                    VersionOrdering::Equal | VersionOrdering::Greater => {
                        crate::UpdateStatus::UpToDate
                    }
                },
                None => crate::UpdateStatus::Failed,
            }
        };

        Ok(ResolvedItemUpdate {
            checked_at,
            resolution: Some(resolution),
            status,
            error: None,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedItemUpdate {
    pub checked_at: TimestampMillis,
    pub resolution: Option<DistributionResolution>,
    pub status: crate::UpdateStatus,
    pub error: Option<String>,
}

impl ResolvedItemUpdate {
    #[must_use]
    pub fn not_checkable(checked_at: TimestampMillis) -> Self {
        Self {
            checked_at,
            resolution: None,
            status: crate::UpdateStatus::NotCheckable,
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{
        DIST_BEGIN_BYTES, DistributionResolver, DistributionResolverOptions,
        MAX_PARSED_CACHE_COUNT, MAX_SOURCE_CACHE_COUNT, SOURCE_READ_CHUNK_BYTES,
        gist_raw_content_urls, github_directory_raw_content_urls,
        github_repository_raw_content_urls, is_gist_url, is_github_directory_url,
        is_github_repository_url, read_metadata_source_block,
    };
    use url::Url;

    #[test]
    fn maps_gist_page_url_to_raw_scriptmeta_url() {
        let url = Url::parse("https://gist.github.com/Yamonov/6f00bd65e486513d82f773f858ac76cb")
            .expect("url");

        assert!(is_gist_url(&url));
        assert_eq!(
            gist_raw_content_urls(&url).first().map(Url::as_str),
            Some(
                "https://gist.githubusercontent.com/Yamonov/6f00bd65e486513d82f773f858ac76cb/raw/SCRIPTMETA.txt"
            )
        );
    }

    #[test]
    fn treats_plain_url_as_plain_source_url() {
        let url = Url::parse("https://example.com/SCRIPTMETA.txt").expect("url");

        assert!(!is_gist_url(&url));
        assert!(!is_github_repository_url(&url));
        assert!(!is_github_directory_url(&url));
    }

    #[test]
    fn maps_github_directory_url_to_raw_scriptmeta_url() {
        let url = Url::parse("https://github.com/Yamonov/Iwashiya_Scripts/tree/main/Illustrator")
            .expect("url");

        assert!(is_github_directory_url(&url));
        assert_eq!(
            github_directory_raw_content_urls(&url)
                .expect("directory raw url")
                .first()
                .map(Url::as_str),
            Some(
                "https://raw.githubusercontent.com/Yamonov/Iwashiya_Scripts/main/Illustrator/SCRIPTMETA.txt"
            )
        );
    }

    #[test]
    fn maps_github_repository_url_to_ordered_raw_candidates() {
        let url = Url::parse("https://github.com/Yamonov/Iwashiya_Scripts").expect("url");

        assert!(is_github_repository_url(&url));
        let candidates = github_repository_raw_content_urls(&url).expect("repository candidates");
        assert_eq!(candidates.len(), 3);
        assert_eq!(
            candidates.iter().map(Url::as_str).collect::<Vec<_>>(),
            vec![
                "https://raw.githubusercontent.com/Yamonov/Iwashiya_Scripts/HEAD/SCRIPTMETA.txt",
                "https://raw.githubusercontent.com/Yamonov/Iwashiya_Scripts/main/SCRIPTMETA.txt",
                "https://raw.githubusercontent.com/Yamonov/Iwashiya_Scripts/master/SCRIPTMETA.txt",
            ]
        );
    }

    #[test]
    fn reads_distribution_block_after_large_prefix() {
        let mut source = Vec::new();
        source.resize(
            DistributionResolverOptions::default().max_metadata_block_bytes + 128,
            b'x',
        );
        source.extend_from_slice(
            b"\nSCRIPTMETA-DIST-BEGIN\nScript-ID: com.example.large\nLatest-Version: 9.0.0\nSCRIPTMETA-DIST-END\n",
        );
        let mut reader = Cursor::new(source);

        let block = read_metadata_source_block(
            &mut reader,
            DistributionResolverOptions::default().max_metadata_block_bytes,
            None,
        )
        .expect("metadata block");

        assert!(block.contains("Script-ID: com.example.large"));
        assert!(block.contains("Latest-Version: 9.0.0"));
    }

    #[test]
    fn finds_distribution_marker_split_across_read_chunks() {
        let mut source = Vec::new();
        source.resize(SOURCE_READ_CHUNK_BYTES - (DIST_BEGIN_BYTES.len() / 2), b'x');
        source.extend_from_slice(
            b"SCRIPTMETA-DIST-BEGIN\nScript-ID: com.example.split\nLatest-Version: 1.2.3\nSCRIPTMETA-DIST-END",
        );
        let mut reader = Cursor::new(source);

        let block = read_metadata_source_block(
            &mut reader,
            DistributionResolverOptions::default().max_metadata_block_bytes,
            None,
        )
        .expect("metadata block");

        assert!(block.contains("Script-ID: com.example.split"));
    }

    #[test]
    fn reads_distribution_markers_case_insensitively() {
        let source = b"scriptmeta-dist-begin\nScript-ID: com.example.case\nVersion: 1.0.0\nscriptmeta-dist-end";
        let mut reader = Cursor::new(source);

        let block = read_metadata_source_block(
            &mut reader,
            DistributionResolverOptions::default().max_metadata_block_bytes,
            None,
        )
        .expect("metadata block");

        assert!(block.contains("Script-ID: com.example.case"));
    }

    #[test]
    fn ignores_legacy_script_block_before_distribution_block() {
        let source = b"SCRIPTMETA-BEGIN\nScript-ID: com.example.legacy\nVersion: 1.0.0\nSCRIPTMETA-END\npadding\nSCRIPTMETA-DIST-BEGIN\nScript-ID: com.example.dist\nLatest-Version: 2.0.0\nSCRIPTMETA-DIST-END";
        let mut reader = Cursor::new(source);

        let block = read_metadata_source_block(
            &mut reader,
            DistributionResolverOptions::default().max_metadata_block_bytes,
            None,
        )
        .expect("metadata block");

        assert!(block.contains("Script-ID: com.example.dist"));
        assert!(!block.contains("com.example.legacy"));
    }

    #[test]
    fn prefers_distribution_block_inside_html_body() {
        let source = br#"
<html>
<head>
<meta name="description" content="SCRIPTMETA-DIST-BEGIN Script-ID=com.example.head Version=1.0.0 SCRIPTMETA-DIST-END">
</head>
<body>
<p>SCRIPTMETA-DIST-BEGIN<br>Script-ID=com.example.body<br>Version=2.0.0<br>SCRIPTMETA-DIST-END</p>
</body>
</html>
"#;
        let mut reader = Cursor::new(source);

        let block = read_metadata_source_block(
            &mut reader,
            DistributionResolverOptions::default().max_metadata_block_bytes,
            None,
        )
        .expect("metadata block");

        assert!(block.contains("Script-ID=com.example.body"));
        assert!(!block.contains("Script-ID=com.example.head"));
    }

    #[test]
    fn rejects_head_distribution_block_when_html_body_exists_without_metadata() {
        let source = br#"
<html>
<head>
<meta name="description" content="SCRIPTMETA-DIST-BEGIN Script-ID=com.example.head Version=1.0.0 SCRIPTMETA-DIST-END">
</head>
<body><p>No distribution metadata here.</p></body>
</html>
"#;
        let mut reader = Cursor::new(source);

        let error = read_metadata_source_block(
            &mut reader,
            DistributionResolverOptions::default().max_metadata_block_bytes,
            None,
        )
        .expect_err("head metadata should not be used when an HTML body exists");

        assert!(
            matches!(error, super::MetadataSourceReadError::Parse(message) if message == "missing SCRIPTMETA distribution block")
        );
    }

    #[test]
    fn rejects_legacy_script_block_when_distribution_block_is_missing() {
        let source =
            b"SCRIPTMETA-BEGIN\nScript-ID: com.example.legacy\nVersion: 1.0.0\nSCRIPTMETA-END";
        let mut reader = Cursor::new(source);

        let error = read_metadata_source_block(
            &mut reader,
            DistributionResolverOptions::default().max_metadata_block_bytes,
            None,
        )
        .expect_err("legacy script block is not distribution metadata");

        assert!(
            matches!(error, super::MetadataSourceReadError::Parse(message) if message == "missing SCRIPTMETA distribution block")
        );
    }

    #[test]
    fn times_out_metadata_source_read_between_chunks() {
        let source = b"SCRIPTMETA-DIST-BEGIN\nScript-ID: com.example.timeout\n";
        let mut reader = Cursor::new(source);

        let error = read_metadata_source_block(
            &mut reader,
            DistributionResolverOptions::default().max_metadata_block_bytes,
            Some(0),
        )
        .expect_err("zero timeout should stop the stream");

        assert!(
            matches!(error, super::MetadataSourceReadError::Timeout(message) if message.contains("timed out"))
        );
    }

    #[test]
    fn reuses_loaded_distribution_source_in_one_resolver() {
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("SCRIPTMETA.txt");
        std::fs::write(
            &metadata_path,
            "SCRIPTMETA-DIST-BEGIN\n\
Script-ID: com.example.one\nVersion: 1.0.0\n\
Script-ID: com.example.two\nVersion: 2.0.0\n\
SCRIPTMETA-DIST-END\n",
        )
        .expect("metadata");

        let resolver =
            DistributionResolver::new(DistributionResolverOptions::default()).expect("resolver");
        let url = Url::from_file_path(&metadata_path).expect("file url");

        let first = resolver
            .resolve(&url, "com.example.one", 1)
            .expect("first resolution");
        assert_eq!(first.latest_version.as_deref(), Some("1.0.0"));

        std::fs::write(
            &metadata_path,
            "SCRIPTMETA-DIST-BEGIN\n\
Script-ID: com.example.two\nVersion: 9.0.0\n\
SCRIPTMETA-DIST-END\n",
        )
        .expect("rewrite metadata");

        let second = resolver
            .resolve(&url, "com.example.two", 2)
            .expect("second resolution");
        assert_eq!(second.latest_version.as_deref(), Some("2.0.0"));

        let cache = resolver.cache.lock().expect("resolver cache");
        assert_eq!(cache.source_cache.len(), 1);
        assert_eq!(cache.parsed_cache.len(), 1);
    }

    #[test]
    fn reuses_latest_url_chain_sources_in_one_resolver() {
        let temp = tempfile::tempdir().expect("tempdir");
        let first_path = temp.path().join("first.txt");
        let second_path = temp.path().join("second.txt");
        let second_url = Url::from_file_path(&second_path).expect("second file url");
        std::fs::write(
            &first_path,
            format!(
                "SCRIPTMETA-DIST-BEGIN\n\
Script-ID: com.example.one\nLatest-URL: {second_url}\n\
SCRIPTMETA-DIST-END\n"
            ),
        )
        .expect("first metadata");
        std::fs::write(
            &second_path,
            "SCRIPTMETA-DIST-BEGIN\n\
Script-ID: com.example.one\nVersion: 3.0.0\n\
SCRIPTMETA-DIST-END\n",
        )
        .expect("second metadata");

        let resolver =
            DistributionResolver::new(DistributionResolverOptions::default()).expect("resolver");
        let first_url = Url::from_file_path(&first_path).expect("first file url");
        let first = resolver
            .resolve(&first_url, "com.example.one", 1)
            .expect("first resolution");
        assert_eq!(first.latest_version.as_deref(), Some("3.0.0"));

        std::fs::write(
            &second_path,
            "SCRIPTMETA-DIST-BEGIN\n\
Script-ID: com.example.one\nVersion: 9.0.0\n\
SCRIPTMETA-DIST-END\n",
        )
        .expect("rewrite second metadata");

        let second = resolver
            .resolve(&first_url, "com.example.one", 2)
            .expect("second resolution");
        assert_eq!(second.latest_version.as_deref(), Some("3.0.0"));

        let cache = resolver.cache.lock().expect("resolver cache");
        assert_eq!(cache.source_cache.len(), 2);
        assert_eq!(cache.parsed_cache.len(), 2);
    }

    #[test]
    fn caps_distribution_resolver_cache_sizes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let resolver =
            DistributionResolver::new(DistributionResolverOptions::default()).expect("resolver");

        for index in 0..(MAX_PARSED_CACHE_COUNT + 2) {
            let metadata_path = temp.path().join(format!("SCRIPTMETA-{index}.txt"));
            std::fs::write(
                &metadata_path,
                format!(
                    "SCRIPTMETA-DIST-BEGIN\nScript-ID: com.example.{index}\nVersion: {index}.0.0\nSCRIPTMETA-DIST-END\n"
                ),
            )
            .expect("metadata");
            let url = Url::from_file_path(&metadata_path).expect("file url");
            let script_id = format!("com.example.{index}");
            resolver
                .resolve(&url, &script_id, index as u64)
                .expect("resolution");
        }

        let cache = resolver.cache.lock().expect("resolver cache");
        assert_eq!(cache.source_cache.len(), MAX_SOURCE_CACHE_COUNT);
        assert_eq!(cache.source_order.len(), MAX_SOURCE_CACHE_COUNT);
        assert_eq!(cache.parsed_cache.len(), MAX_PARSED_CACHE_COUNT);
        assert_eq!(cache.parsed_order.len(), MAX_PARSED_CACHE_COUNT);
    }
}
