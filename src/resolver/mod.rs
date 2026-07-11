use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

#[cfg(feature = "blocking-http")]
use reqwest::{
    Client, StatusCode,
    header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, RETRY_AFTER},
};
#[cfg(not(target_os = "macos"))]
use std::fs::File;
#[cfg(any(test, not(target_os = "macos")))]
use std::io::Read;
#[cfg(feature = "blocking-http")]
use std::{future, sync::mpsc, thread};
use url::Url;

use crate::{
    TimestampMillis,
    core::{
        DistributionMetadata, DistributionResolution, OperationCancellation, ScriptMetaItem,
        ScriptMetaKitError, ScriptMetaKitResult, VersionOrdering, compare_versions,
        decode_script_text, parse_distribution_metadata_records,
        select_distribution_metadata_for_script,
    },
    now_timestamp_millis,
};

#[cfg(feature = "blocking-http")]
use crate::core::OperationCancellationListener;

mod retry;
#[cfg(feature = "blocking-http")]
use retry::parse_retry_after_millis;
pub(crate) use retry::{is_retryable_source_error, retry_after_hint_millis};

// One resolver is scoped to one update operation. Keep enough entries for the
// default parallel work set and redirect convergence without retaining data
// beyond the operation.
const MAX_SOURCE_CACHE_COUNT: usize = 32;
const MAX_PARSED_CACHE_COUNT: usize = 32;
const MAX_SOURCE_FAILURE_CACHE_COUNT: usize = 32;
const MAX_PARSED_FAILURE_CACHE_COUNT: usize = 32;
#[cfg(feature = "blocking-http")]
const MAX_HTTP_VALIDATION_CACHE_COUNT: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DistributionResolverOptions {
    pub max_redirects: usize,
    /// Maximum bytes retained for one extracted SCRIPTMETA block.
    /// The source stream itself is read until a preferred block is found or EOF.
    /// Use 0 to disable this block-size guard.
    pub max_metadata_block_bytes: usize,
    /// Maximum total bytes read from one metadata source. Use 0 to disable.
    pub max_source_bytes: usize,
    pub request_timeout_millis: Option<u64>,
    /// Maximum elapsed time while streaming one metadata source.
    /// HTTP reads are interrupted at the deadline. On macOS, local file reads
    /// use interruptible Dispatch I/O; other platforms check the deadline
    /// between operating-system read calls.
    pub resource_timeout_millis: Option<u64>,
    pub cache_enabled: bool,
}

impl Default for DistributionResolverOptions {
    fn default() -> Self {
        Self {
            max_redirects: 8,
            max_metadata_block_bytes: 256 * 1024,
            max_source_bytes: 4 * 1024 * 1024,
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
    cancellation: Option<OperationCancellation>,
    source_retry_attempts: usize,
    #[cfg(feature = "blocking-http")]
    http_validation_cache: HttpValidationCache,
    #[cfg(feature = "blocking-http")]
    client: Client,
    #[cfg(feature = "blocking-http")]
    http_executor: Arc<Mutex<Option<HttpExecutor>>>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct HttpValidationCache {
    #[cfg(feature = "blocking-http")]
    inner: Arc<Mutex<HttpValidationCacheState>>,
}

#[cfg(feature = "blocking-http")]
#[derive(Clone, Debug, Default)]
struct HttpValidationCacheState {
    entries: BTreeMap<String, HttpValidationEntry>,
    order: Vec<String>,
}

#[cfg(feature = "blocking-http")]
#[derive(Clone, Debug)]
struct HttpValidationEntry {
    etag: Option<String>,
    last_modified: Option<String>,
    source: LoadedMetadataSource,
}

impl HttpValidationCache {
    #[cfg(feature = "blocking-http")]
    fn request_validators(
        &self,
        key: &str,
    ) -> ScriptMetaKitResult<(Option<String>, Option<String>)> {
        let mut cache = self
            .inner
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("HTTP validation cache is poisoned".into()))?;
        let validators = cache
            .entries
            .get(key)
            .map(|entry| (entry.etag.clone(), entry.last_modified.clone()))
            .unwrap_or_default();
        if cache.entries.contains_key(key) {
            touch_cache_key(&mut cache.order, key);
        }
        Ok(validators)
    }

    #[cfg(feature = "blocking-http")]
    fn cached_source(&self, key: &str) -> ScriptMetaKitResult<Option<LoadedMetadataSource>> {
        let mut cache = self
            .inner
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("HTTP validation cache is poisoned".into()))?;
        let source = cache.entries.get(key).map(|entry| entry.source.clone());
        if source.is_some() {
            touch_cache_key(&mut cache.order, key);
        }
        Ok(source)
    }

    #[cfg(feature = "blocking-http")]
    fn store_response(
        &self,
        key: String,
        etag: Option<String>,
        last_modified: Option<String>,
        source: LoadedMetadataSource,
    ) -> ScriptMetaKitResult<()> {
        let mut cache = self
            .inner
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("HTTP validation cache is poisoned".into()))?;
        if etag.is_none() && last_modified.is_none() {
            cache.entries.remove(&key);
            cache.order.retain(|cached_key| cached_key != &key);
            return Ok(());
        }
        touch_cache_key(&mut cache.order, &key);
        cache.entries.insert(
            key,
            HttpValidationEntry {
                etag,
                last_modified,
                source,
            },
        );
        while cache.entries.len() > MAX_HTTP_VALIDATION_CACHE_COUNT {
            if let Some(oldest) = cache.order.first().cloned() {
                cache.order.remove(0);
                cache.entries.remove(&oldest);
            } else {
                break;
            }
        }
        Ok(())
    }

    pub(crate) fn clear(&self) {
        #[cfg(feature = "blocking-http")]
        if let Ok(mut cache) = self.inner.lock() {
            cache.entries.clear();
            cache.order.clear();
        }
    }
}

#[derive(Clone, Debug, Default)]
struct DistributionResolverCache {
    source_cache: BTreeMap<String, Arc<LoadedMetadataSource>>,
    source_order: Vec<String>,
    source_failure_cache: BTreeMap<String, CachedSourceFailure>,
    source_failure_order: Vec<String>,
    parsed_cache: BTreeMap<String, Arc<Vec<DistributionMetadata>>>,
    parsed_order: Vec<String>,
    parsed_failure_cache: BTreeMap<String, Arc<ScriptMetaKitError>>,
    parsed_failure_order: Vec<String>,
    source_flights: BTreeMap<String, Arc<SourceLoadFlight>>,
}

#[derive(Debug, Default)]
struct SourceLoadFlight {
    result: Mutex<Option<ScriptMetaKitResult<Arc<LoadedMetadataSource>>>>,
    completed: Condvar,
}

enum SourceLoadFlightRole {
    Ready(ScriptMetaKitResult<Arc<LoadedMetadataSource>>),
    Leader(Arc<SourceLoadFlight>),
    Follower(Arc<SourceLoadFlight>),
}

#[derive(Clone, Debug)]
struct CachedSourceFailure {
    error: Arc<ScriptMetaKitError>,
    attempts: usize,
    terminal: bool,
}

#[cfg(feature = "blocking-http")]
#[derive(Clone, Debug)]
struct HttpExecutor {
    state: Arc<HttpExecutorState>,
}

#[cfg(feature = "blocking-http")]
#[derive(Debug)]
struct HttpExecutorState {
    sender: Mutex<Option<mpsc::Sender<HttpRequest>>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

#[cfg(feature = "blocking-http")]
struct HttpRequest {
    client: Client,
    url: Url,
    max_metadata_block_bytes: usize,
    max_source_bytes: usize,
    resource_timeout_millis: Option<u64>,
    cancellation: Option<OperationCancellation>,
    validation_cache: HttpValidationCache,
    response: mpsc::SyncSender<ScriptMetaKitResult<LoadedMetadataSource>>,
}

#[cfg(feature = "blocking-http")]
struct HttpLoadRequest {
    client: Client,
    url: Url,
    max_metadata_block_bytes: usize,
    max_source_bytes: usize,
    resource_timeout_millis: Option<u64>,
    cancellation: Option<OperationCancellation>,
    validation_cache: HttpValidationCache,
}

#[cfg(feature = "blocking-http")]
struct HttpCancellationWaiter {
    receiver: tokio::sync::oneshot::Receiver<()>,
    _listener: OperationCancellationListener,
}

#[cfg(feature = "blocking-http")]
impl HttpCancellationWaiter {
    fn new(cancellation: Option<&OperationCancellation>) -> Option<Self> {
        let cancellation = cancellation?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let sender = Arc::new(Mutex::new(Some(sender)));
        let listener_sender = Arc::clone(&sender);
        let listener = cancellation.register_cancel_listener(move || {
            if let Some(sender) = listener_sender
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            {
                let _ = sender.send(());
            }
        });
        Some(Self {
            receiver,
            _listener: listener,
        })
    }
}

#[cfg(feature = "blocking-http")]
impl HttpExecutor {
    fn new() -> ScriptMetaKitResult<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|error| ScriptMetaKitError::Url(error.to_string()))?;
        let (sender, receiver) = mpsc::channel::<HttpRequest>();
        let worker = thread::Builder::new()
            .name("scriptmetakit-http".to_string())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    runtime.spawn(async move {
                        let result = load_http_request(request.client.clone(), &request).await;
                        let _ = request.response.send(result);
                    });
                }
                runtime.shutdown_timeout(Duration::from_secs(1));
            })
            .map_err(|error| ScriptMetaKitError::Url(error.to_string()))?;
        Ok(Self {
            state: Arc::new(HttpExecutorState {
                sender: Mutex::new(Some(sender)),
                worker: Mutex::new(Some(worker)),
            }),
        })
    }

    fn load(&self, request: HttpLoadRequest) -> ScriptMetaKitResult<LoadedMetadataSource> {
        let (response, receiver) = mpsc::sync_channel(1);
        let sender = self
            .state
            .sender
            .lock()
            .map_err(|_| ScriptMetaKitError::Url("HTTP executor lock is poisoned".to_string()))?
            .as_ref()
            .cloned()
            .ok_or_else(|| ScriptMetaKitError::Url("HTTP executor is unavailable".to_string()))?;
        sender
            .send(HttpRequest {
                client: request.client,
                url: request.url,
                max_metadata_block_bytes: request.max_metadata_block_bytes,
                max_source_bytes: request.max_source_bytes,
                resource_timeout_millis: request.resource_timeout_millis,
                cancellation: request.cancellation,
                validation_cache: request.validation_cache,
                response,
            })
            .map_err(|_| ScriptMetaKitError::Url("HTTP executor is unavailable".to_string()))?;
        receiver.recv().map_err(|_| {
            ScriptMetaKitError::Url("HTTP executor stopped unexpectedly".to_string())
        })?
    }
}

#[cfg(feature = "blocking-http")]
impl Drop for HttpExecutor {
    fn drop(&mut self) {
        if Arc::strong_count(&self.state) != 1 {
            return;
        }
        let sender = self
            .state
            .sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        drop(sender);
        let worker = self
            .state
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

impl DistributionResolver {
    pub fn new(options: DistributionResolverOptions) -> ScriptMetaKitResult<Self> {
        Self::new_with_cancellation(options, None)
    }

    pub fn new_with_cancellation(
        options: DistributionResolverOptions,
        cancellation: Option<OperationCancellation>,
    ) -> ScriptMetaKitResult<Self> {
        Self::new_with_cancellation_and_retry(options, cancellation, 0)
    }

    pub(crate) fn new_with_cancellation_and_retry(
        options: DistributionResolverOptions,
        cancellation: Option<OperationCancellation>,
        source_retry_attempts: usize,
    ) -> ScriptMetaKitResult<Self> {
        Self::new_with_cancellation_retry_and_http_cache(
            options,
            cancellation,
            source_retry_attempts,
            HttpValidationCache::default(),
        )
    }

    pub(crate) fn new_with_cancellation_retry_and_http_cache(
        options: DistributionResolverOptions,
        cancellation: Option<OperationCancellation>,
        source_retry_attempts: usize,
        http_validation_cache: HttpValidationCache,
    ) -> ScriptMetaKitResult<Self> {
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
                cancellation,
                source_retry_attempts,
                http_validation_cache,
                client,
                http_executor: Arc::new(Mutex::new(None)),
            })
        }

        #[cfg(not(feature = "blocking-http"))]
        {
            let _ = http_validation_cache;
            let _ = Duration::from_millis(options.request_timeout_millis.unwrap_or(0));
            let _ = Duration::from_millis(options.resource_timeout_millis.unwrap_or(0));
            Ok(Self {
                options,
                cache: Arc::new(Mutex::new(DistributionResolverCache::default())),
                cancellation,
                source_retry_attempts,
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
            self.ensure_not_cancelled()?;
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
                metadata
                    .note
                    .or_else(|| Some("Latest-Version was not found".to_string()))
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
        self.ensure_not_cancelled()?;
        let cache_key = url.as_str().to_string();
        if self.options.cache_enabled {
            if let Some(source) = self.cached_source(&cache_key)? {
                return Ok(source);
            }
            if let Some(error) = self.cached_terminal_source_failure(&cache_key)? {
                return Err(error.as_ref().clone());
            }
        }

        if !self.options.cache_enabled {
            return self.load_source_and_update_cache(url, cache_key);
        }

        match self.source_load_flight(&cache_key)? {
            SourceLoadFlightRole::Ready(result) => result,
            SourceLoadFlightRole::Follower(flight) => self.wait_for_source_load(flight),
            SourceLoadFlightRole::Leader(flight) => {
                let result = self.load_source_and_update_cache(url, cache_key.clone());
                self.complete_source_load(&cache_key, &flight, result.clone())?;
                result
            }
        }
    }

    fn load_source_and_update_cache(
        &self,
        url: &Url,
        cache_key: String,
    ) -> ScriptMetaKitResult<Arc<LoadedMetadataSource>> {
        let source = match self.load_uncached_source(url) {
            Ok(source) => Arc::new(source),
            Err(error) => {
                let error = source_error_with_url(url, error);
                if self.options.cache_enabled
                    && !self
                        .cancellation
                        .as_ref()
                        .is_some_and(OperationCancellation::is_cancelled)
                {
                    self.record_source_failure(cache_key, &error)?;
                }
                return Err(error);
            }
        };
        self.ensure_not_cancelled()?;
        if self.options.cache_enabled {
            self.clear_source_failure(&cache_key)?;
            self.store_source(cache_key, Arc::clone(&source))?;
        }
        Ok(source)
    }

    fn source_load_flight(&self, key: &str) -> ScriptMetaKitResult<SourceLoadFlightRole> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("distribution cache is poisoned".to_string()))?;
        if let Some(source) = cache.source_cache.get(key).cloned() {
            touch_cache_key(&mut cache.source_order, key);
            return Ok(SourceLoadFlightRole::Ready(Ok(source)));
        }
        if let Some(error) = cache
            .source_failure_cache
            .get(key)
            .filter(|failure| failure.terminal)
            .map(|failure| failure.error.as_ref().clone())
        {
            touch_cache_key(&mut cache.source_failure_order, key);
            return Ok(SourceLoadFlightRole::Ready(Err(error)));
        }
        if let Some(flight) = cache.source_flights.get(key) {
            return Ok(SourceLoadFlightRole::Follower(Arc::clone(flight)));
        }
        let flight = Arc::new(SourceLoadFlight::default());
        cache
            .source_flights
            .insert(key.to_string(), Arc::clone(&flight));
        Ok(SourceLoadFlightRole::Leader(flight))
    }

    fn wait_for_source_load(
        &self,
        flight: Arc<SourceLoadFlight>,
    ) -> ScriptMetaKitResult<Arc<LoadedMetadataSource>> {
        let mut result = flight
            .result
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("source flight is poisoned".to_string()))?;
        loop {
            if let Some(result) = result.as_ref() {
                return result.clone();
            }
            self.ensure_not_cancelled()?;
            let (next_result, _) = flight
                .completed
                .wait_timeout(result, Duration::from_millis(50))
                .map_err(|_| ScriptMetaKitError::Cache("source flight is poisoned".to_string()))?;
            result = next_result;
        }
    }

    fn complete_source_load(
        &self,
        key: &str,
        flight: &SourceLoadFlight,
        result: ScriptMetaKitResult<Arc<LoadedMetadataSource>>,
    ) -> ScriptMetaKitResult<()> {
        {
            let mut flight_result = flight
                .result
                .lock()
                .map_err(|_| ScriptMetaKitError::Cache("source flight is poisoned".to_string()))?;
            *flight_result = Some(result);
        }
        flight.completed.notify_all();
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("distribution cache is poisoned".to_string()))?;
        cache.source_flights.remove(key);
        Ok(())
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
        if self.options.cache_enabled {
            if let Some(records) = self.cached_records(&cache_key)? {
                return Ok(records);
            }
            if let Some(error) = self.cached_parsed_failure(&cache_key)? {
                return Err(error.as_ref().clone());
            }
        }

        let records = match parse_distribution_metadata_records(&source.text) {
            Ok(records) => Arc::new(records),
            Err(error) => {
                if self.options.cache_enabled {
                    self.store_parsed_failure(cache_key, &error)?;
                }
                return Err(error);
            }
        };
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

    fn cached_terminal_source_failure(
        &self,
        key: &str,
    ) -> ScriptMetaKitResult<Option<Arc<ScriptMetaKitError>>> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("distribution cache is poisoned".to_string()))?;
        let failure = cache
            .source_failure_cache
            .get(key)
            .filter(|failure| failure.terminal)
            .map(|failure| Arc::clone(&failure.error));
        if failure.is_some() {
            touch_cache_key(&mut cache.source_failure_order, key);
        }
        Ok(failure)
    }

    fn record_source_failure(
        &self,
        key: String,
        error: &ScriptMetaKitError,
    ) -> ScriptMetaKitResult<()> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("distribution cache is poisoned".to_string()))?;
        let attempts = cache
            .source_failure_cache
            .get(&key)
            .map_or(1, |failure| failure.attempts.saturating_add(1));
        let terminal = !is_retryable_source_error(error) || attempts > self.source_retry_attempts;
        touch_cache_key(&mut cache.source_failure_order, &key);
        cache.source_failure_cache.insert(
            key,
            CachedSourceFailure {
                error: Arc::new(error.clone()),
                attempts,
                terminal,
            },
        );
        evict_source_failure_cache_entries(&mut cache);
        Ok(())
    }

    fn clear_source_failure(&self, key: &str) -> ScriptMetaKitResult<()> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("distribution cache is poisoned".to_string()))?;
        cache.source_failure_cache.remove(key);
        cache
            .source_failure_order
            .retain(|cached_key| cached_key != key);
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

    fn cached_parsed_failure(
        &self,
        key: &str,
    ) -> ScriptMetaKitResult<Option<Arc<ScriptMetaKitError>>> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("distribution cache is poisoned".to_string()))?;
        let failure = cache.parsed_failure_cache.get(key).cloned();
        if failure.is_some() {
            touch_cache_key(&mut cache.parsed_failure_order, key);
        }
        Ok(failure)
    }

    fn store_parsed_failure(
        &self,
        key: String,
        error: &ScriptMetaKitError,
    ) -> ScriptMetaKitResult<()> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("distribution cache is poisoned".to_string()))?;
        touch_cache_key(&mut cache.parsed_failure_order, &key);
        cache
            .parsed_failure_cache
            .insert(key, Arc::new(error.clone()));
        evict_parsed_failure_cache_entries(&mut cache);
        Ok(())
    }

    pub(crate) fn terminal_source_failure(
        &self,
        url: &Url,
    ) -> ScriptMetaKitResult<Option<Arc<ScriptMetaKitError>>> {
        if !self.options.cache_enabled {
            return Ok(None);
        }
        self.cached_terminal_source_failure(url.as_str())
    }

    pub(crate) fn is_terminal_source_failure(
        &self,
        error: &ScriptMetaKitError,
    ) -> ScriptMetaKitResult<bool> {
        if !self.options.cache_enabled {
            return Ok(false);
        }
        let cache = self
            .cache
            .lock()
            .map_err(|_| ScriptMetaKitError::Cache("distribution cache is poisoned".to_string()))?;
        Ok(cache
            .source_failure_cache
            .values()
            .any(|failure| failure.terminal && failure.error.as_ref() == error))
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
        let mut first_retryable_error: Option<ScriptMetaKitError> = None;
        for candidate_url in candidate_urls {
            match self.load_plain_source(&candidate_url) {
                Ok(source) => {
                    return Ok(LoadedMetadataSource {
                        text: source.text,
                        resolved_url: original_url.clone(),
                    });
                }
                Err(error) => {
                    if first_retryable_error.is_none() && is_retryable_source_error(&error) {
                        first_retryable_error = Some(error.clone());
                    }
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        if let Some(error @ ScriptMetaKitError::NotImplemented(_)) = first_error {
            return Err(error);
        }
        if let Some(error) = first_retryable_error {
            return Err(error);
        }

        Err(ScriptMetaKitError::Parse(not_found_message.to_string()))
    }

    fn load_file(&self, url: &Url) -> ScriptMetaKitResult<String> {
        let path = url
            .to_file_path()
            .map_err(|_| ScriptMetaKitError::Url("invalid file:// Meta-URL".to_string()))?;

        #[cfg(target_os = "macos")]
        let read_result = read_metadata_source_file_controlled(
            &path,
            self.options.max_metadata_block_bytes,
            self.options.max_source_bytes,
            self.options.resource_timeout_millis,
            self.cancellation.as_ref(),
        );

        #[cfg(not(target_os = "macos"))]
        let read_result = {
            let mut file = File::open(&path).map_err(|error| ScriptMetaKitError::Io {
                path: path.clone(),
                message: io_error_message(&error),
            })?;
            read_metadata_source_block_controlled(
                &mut file,
                self.options.max_metadata_block_bytes,
                self.options.max_source_bytes,
                self.options.resource_timeout_millis,
                self.cancellation.as_ref(),
            )
        };

        read_result.map_err(|error| {
            map_metadata_source_error(error, |error| ScriptMetaKitError::Io {
                path,
                message: io_error_message(&error),
            })
        })
    }

    #[cfg(feature = "blocking-http")]
    fn load_http(&self, url: &Url) -> ScriptMetaKitResult<LoadedMetadataSource> {
        self.ensure_not_cancelled()?;
        let executor = {
            let mut executor = self.http_executor.lock().map_err(|_| {
                ScriptMetaKitError::Url("HTTP executor lock is poisoned".to_string())
            })?;
            if executor.is_none() {
                *executor = Some(HttpExecutor::new()?);
            }
            executor
                .as_ref()
                .expect("HTTP executor initialized")
                .clone()
        };
        executor.load(HttpLoadRequest {
            client: self.client.clone(),
            url: url.clone(),
            max_metadata_block_bytes: self.options.max_metadata_block_bytes,
            max_source_bytes: self.options.max_source_bytes,
            resource_timeout_millis: self.options.resource_timeout_millis,
            cancellation: self.cancellation.clone(),
            validation_cache: self.http_validation_cache.clone(),
        })
    }

    #[cfg(not(feature = "blocking-http"))]
    fn load_http(&self, _url: &Url) -> ScriptMetaKitResult<LoadedMetadataSource> {
        Err(ScriptMetaKitError::NotImplemented(
            "HTTP/HTTPS Meta-URL resolution requires the `blocking-http` feature".to_string(),
        ))
    }

    fn ensure_not_cancelled(&self) -> ScriptMetaKitResult<()> {
        if self
            .cancellation
            .as_ref()
            .is_some_and(OperationCancellation::is_cancelled)
        {
            return Err(ScriptMetaKitError::Timeout(
                "metadata resolution was cancelled".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct LoadedMetadataSource {
    text: String,
    resolved_url: Url,
}

#[cfg(feature = "blocking-http")]
async fn load_http_request(
    client: Client,
    request: &HttpRequest,
) -> ScriptMetaKitResult<LoadedMetadataSource> {
    let cache_key = request.url.as_str().to_string();
    let (etag, last_modified) = request.validation_cache.request_validators(&cache_key)?;
    let cancellation = request.cancellation.clone();
    let mut cancellation_waiter = HttpCancellationWaiter::new(cancellation.as_ref());
    let mut request_builder = client.get(request.url.as_str());
    if let Some(etag) = etag {
        request_builder = request_builder.header(IF_NONE_MATCH, etag);
    } else if let Some(last_modified) = last_modified {
        request_builder = request_builder.header(IF_MODIFIED_SINCE, last_modified);
    }
    let response = tokio::select! {
        biased;
        () = wait_for_http_cancellation(&mut cancellation_waiter) => {
            return Err(cancelled_http_error());
        }
        response = request_builder.send() => {
            response.map_err(http_request_error)?
        }
    };
    if response.status() == StatusCode::NOT_MODIFIED {
        return request
            .validation_cache
            .cached_source(&cache_key)?
            .ok_or_else(|| {
                ScriptMetaKitError::Url(
                    "http_status=304: response has no cached metadata source".to_string(),
                )
            });
    }
    if !response.status().is_success() {
        let retry_after_millis = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_retry_after_millis);
        return Err(ScriptMetaKitError::Url(format!(
            "http_status={}{}: {}",
            response.status().as_u16(),
            retry_after_millis
                .map(|delay| format!(";retry_after_millis={delay}"))
                .unwrap_or_default(),
            response.status()
        )));
    }
    let response_etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let response_last_modified = response
        .headers()
        .get(LAST_MODIFIED)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let resolved_url = response.url().clone();
    let text = read_http_metadata_source_block(
        response,
        request.max_metadata_block_bytes,
        request.max_source_bytes,
        request.resource_timeout_millis,
        cancellation_waiter,
    )
    .await?;
    let source = LoadedMetadataSource { text, resolved_url };
    request.validation_cache.store_response(
        cache_key,
        response_etag,
        response_last_modified,
        source.clone(),
    )?;
    Ok(source)
}

#[cfg(feature = "blocking-http")]
async fn wait_for_http_cancellation(waiter: &mut Option<HttpCancellationWaiter>) {
    let Some(waiter) = waiter else {
        future::pending::<()>().await;
        return;
    };
    let _ = (&mut waiter.receiver).await;
}

#[cfg(feature = "blocking-http")]
fn cancelled_http_error() -> ScriptMetaKitError {
    ScriptMetaKitError::Timeout("HTTP metadata request was cancelled".to_string())
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

fn evict_source_failure_cache_entries(cache: &mut DistributionResolverCache) {
    if MAX_SOURCE_FAILURE_CACHE_COUNT == 0 {
        cache.source_failure_cache.clear();
        cache.source_failure_order.clear();
        return;
    }

    while cache.source_failure_order.len() > MAX_SOURCE_FAILURE_CACHE_COUNT {
        let evicted = cache.source_failure_order.remove(0);
        cache.source_failure_cache.remove(&evicted);
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

fn evict_parsed_failure_cache_entries(cache: &mut DistributionResolverCache) {
    if MAX_PARSED_FAILURE_CACHE_COUNT == 0 {
        cache.parsed_failure_cache.clear();
        cache.parsed_failure_order.clear();
        return;
    }

    while cache.parsed_failure_order.len() > MAX_PARSED_FAILURE_CACHE_COUNT {
        let evicted = cache.parsed_failure_order.remove(0);
        cache.parsed_failure_cache.remove(&evicted);
    }
}

fn io_error_message(error: &io::Error) -> String {
    format!("kind={:?}: {error}", error.kind())
}

#[cfg(feature = "blocking-http")]
fn http_request_error(error: reqwest::Error) -> ScriptMetaKitError {
    if error.is_timeout() {
        ScriptMetaKitError::Timeout(format!("HTTP metadata request timed out: {error}"))
    } else {
        ScriptMetaKitError::Url(format!("transient_transport: {error}"))
    }
}

fn source_error_with_url(url: &Url, error: ScriptMetaKitError) -> ScriptMetaKitError {
    match error {
        ScriptMetaKitError::Url(message) => ScriptMetaKitError::Url(format!("{url}: {message}")),
        ScriptMetaKitError::Timeout(message) => {
            ScriptMetaKitError::Timeout(format!("{url}: {message}"))
        }
        error => error,
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
    Cancelled,
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
        MetadataSourceReadError::Cancelled => {
            ScriptMetaKitError::Timeout("metadata source read was cancelled".to_string())
        }
    }
}

#[cfg(test)]
fn read_metadata_source_block(
    reader: &mut impl Read,
    max_block_bytes: usize,
    resource_timeout_millis: Option<u64>,
) -> Result<String, MetadataSourceReadError> {
    read_metadata_source_block_controlled(
        reader,
        max_block_bytes,
        DistributionResolverOptions::default().max_source_bytes,
        resource_timeout_millis,
        None,
    )
}

#[cfg(any(test, not(target_os = "macos")))]
fn read_metadata_source_block_controlled(
    reader: &mut impl Read,
    max_block_bytes: usize,
    max_source_bytes: usize,
    resource_timeout_millis: Option<u64>,
    cancellation: Option<&OperationCancellation>,
) -> Result<String, MetadataSourceReadError> {
    let mut chunk = [0_u8; SOURCE_READ_CHUNK_BYTES];
    let started = Instant::now();
    let timeout = resource_timeout_millis.map(Duration::from_millis);
    let mut accumulator = MetadataSourceAccumulator::new(max_block_bytes, max_source_bytes);

    loop {
        if cancellation.is_some_and(OperationCancellation::is_cancelled) {
            return Err(MetadataSourceReadError::Cancelled);
        }
        check_resource_timeout(started, timeout)?;
        let read_len = reader.read(&mut chunk)?;
        if cancellation.is_some_and(OperationCancellation::is_cancelled) {
            return Err(MetadataSourceReadError::Cancelled);
        }
        check_resource_timeout(started, timeout)?;
        if read_len == 0 {
            break;
        }
        if let Some(block) = accumulator.feed(&chunk[..read_len])? {
            return Ok(metadata_block_to_string(block));
        }
    }

    accumulator.finish().map(metadata_block_to_string)
}

#[cfg(target_os = "macos")]
fn read_metadata_source_file_controlled(
    path: &std::path::Path,
    max_block_bytes: usize,
    max_source_bytes: usize,
    resource_timeout_millis: Option<u64>,
    cancellation: Option<&OperationCancellation>,
) -> Result<String, MetadataSourceReadError> {
    let reader = scriptmetakit_macos_io::CancelableFileReader::open(path, SOURCE_READ_CHUNK_BYTES)?;
    read_metadata_source_file_reader_controlled(
        reader,
        max_block_bytes,
        max_source_bytes,
        resource_timeout_millis,
        cancellation,
    )
}

#[cfg(target_os = "macos")]
fn read_metadata_source_file_reader_controlled(
    reader: scriptmetakit_macos_io::CancelableFileReader,
    max_block_bytes: usize,
    max_source_bytes: usize,
    resource_timeout_millis: Option<u64>,
    cancellation: Option<&OperationCancellation>,
) -> Result<String, MetadataSourceReadError> {
    use scriptmetakit_macos_io::ReadEvent;
    use std::sync::mpsc::RecvTimeoutError;

    let started = Instant::now();
    let timeout = resource_timeout_millis.map(Duration::from_millis);
    let mut accumulator = MetadataSourceAccumulator::new(max_block_bytes, max_source_bytes);
    let _cancellation_listener = cancellation.map(|cancellation| {
        let handle = reader.cancellation_handle();
        cancellation.register_cancel_listener(move || handle.cancel())
    });

    loop {
        if cancellation.is_some_and(OperationCancellation::is_cancelled) {
            reader.cancel();
            return Err(MetadataSourceReadError::Cancelled);
        }
        if let Err(error) = check_resource_timeout(started, timeout) {
            reader.cancel();
            return Err(error);
        }

        let event = if let Some(timeout) = timeout {
            let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                reader.cancel();
                return Err(MetadataSourceReadError::Timeout(format!(
                    "metadata source read timed out after {} ms",
                    timeout.as_millis()
                )));
            };
            reader.receive_timeout(remaining)
        } else {
            reader.receive().map_err(|_| RecvTimeoutError::Disconnected)
        };

        match event {
            Ok(ReadEvent::Data(bytes)) => {
                if let Some(block) = accumulator.feed(&bytes)? {
                    reader.cancel();
                    return Ok(metadata_block_to_string(block));
                }
            }
            Ok(ReadEvent::Complete(Ok(()))) => {
                return accumulator.finish().map(metadata_block_to_string);
            }
            Ok(ReadEvent::Complete(Err(error))) => {
                if error.raw_os_error() == Some(libc::ECANCELED)
                    && cancellation.is_some_and(OperationCancellation::is_cancelled)
                {
                    return Err(MetadataSourceReadError::Cancelled);
                }
                return Err(MetadataSourceReadError::Io(error));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(MetadataSourceReadError::Io(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "macOS file read ended without a completion event",
                )));
            }
        }
    }
}

#[cfg(feature = "blocking-http")]
async fn read_http_metadata_source_block(
    mut response: reqwest::Response,
    max_block_bytes: usize,
    max_source_bytes: usize,
    resource_timeout_millis: Option<u64>,
    mut cancellation_waiter: Option<HttpCancellationWaiter>,
) -> ScriptMetaKitResult<String> {
    let deadline = resource_timeout_millis
        .map(Duration::from_millis)
        .map(|timeout| tokio::time::Instant::now() + timeout);
    if max_source_bytes > 0
        && response
            .content_length()
            .is_some_and(|length| length > max_source_bytes as u64)
    {
        return Err(ScriptMetaKitError::Parse(format!(
            "metadata source exceeds the {max_source_bytes} byte limit"
        )));
    }
    let mut accumulator = MetadataSourceAccumulator::new(max_block_bytes, max_source_bytes);

    loop {
        let chunk = if let Some(deadline) = deadline {
            tokio::select! {
                biased;
                () = wait_for_http_cancellation(&mut cancellation_waiter) => {
                    return Err(cancelled_http_error());
                }
                () = tokio::time::sleep_until(deadline) => {
                    return Err(ScriptMetaKitError::Timeout(format!(
                        "metadata source read timed out after {} ms",
                        resource_timeout_millis.unwrap_or_default()
                    )));
                }
                chunk = response.chunk() => chunk,
            }
        } else {
            tokio::select! {
                biased;
                () = wait_for_http_cancellation(&mut cancellation_waiter) => {
                    return Err(cancelled_http_error());
                }
                chunk = response.chunk() => chunk,
            }
        }
        .map_err(|error| ScriptMetaKitError::Url(error.to_string()))?;

        let Some(chunk) = chunk else {
            break;
        };
        if let Some(block) = accumulator.feed(&chunk).map_err(|error| {
            map_metadata_source_error(error, |error| ScriptMetaKitError::Url(error.to_string()))
        })? {
            return Ok(metadata_block_to_string(block));
        }
    }

    accumulator
        .finish()
        .map(metadata_block_to_string)
        .map_err(|error| {
            map_metadata_source_error(error, |error| ScriptMetaKitError::Url(error.to_string()))
        })
}

struct MetadataSourceAccumulator {
    body_tail: Vec<u8>,
    body_search_buffer: Vec<u8>,
    before_body_scanner: MetadataBlockScanner,
    body_scanner: MetadataBlockScanner,
    fallback_block: Option<Vec<u8>>,
    found_body: bool,
    max_source_bytes: usize,
    source_bytes: usize,
}

impl MetadataSourceAccumulator {
    fn new(max_block_bytes: usize, max_source_bytes: usize) -> Self {
        Self {
            body_tail: Vec::with_capacity(SOURCE_MARKER_TAIL_BYTES),
            body_search_buffer: Vec::with_capacity(
                SOURCE_READ_CHUNK_BYTES + SOURCE_MARKER_TAIL_BYTES,
            ),
            before_body_scanner: MetadataBlockScanner::new(max_block_bytes),
            body_scanner: MetadataBlockScanner::new(max_block_bytes),
            fallback_block: None,
            found_body: false,
            max_source_bytes,
            source_bytes: 0,
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> Result<Option<Vec<u8>>, MetadataSourceReadError> {
        self.source_bytes = self.source_bytes.saturating_add(bytes.len());
        if self.max_source_bytes > 0 && self.source_bytes > self.max_source_bytes {
            return Err(MetadataSourceReadError::Parse(format!(
                "metadata source exceeds the {} byte limit",
                self.max_source_bytes
            )));
        }
        if self.found_body {
            return self.body_scanner.feed(bytes);
        }

        self.body_search_buffer.clear();
        self.body_search_buffer.extend_from_slice(&self.body_tail);
        self.body_search_buffer.extend_from_slice(bytes);

        if let Some(body_index) =
            find_ascii_case_insensitive_bytes(&self.body_search_buffer, BODY_BEGIN_BYTES)
        {
            let current_body_start = body_index.saturating_sub(self.body_tail.len());
            if self.fallback_block.is_none()
                && let Some(block) = self
                    .before_body_scanner
                    .feed(&bytes[..current_body_start])?
            {
                self.fallback_block = Some(block);
            }

            self.found_body = true;
            return self
                .body_scanner
                .feed(&self.body_search_buffer[body_index..]);
        }

        if self.fallback_block.is_none()
            && let Some(block) = self.before_body_scanner.feed(bytes)?
        {
            self.fallback_block = Some(block);
        }
        update_search_tail(&mut self.body_tail, &self.body_search_buffer);
        Ok(None)
    }

    fn finish(self) -> Result<Vec<u8>, MetadataSourceReadError> {
        if !self.found_body
            && let Some(block) = self.fallback_block
        {
            return Ok(block);
        }
        Err(MetadataSourceReadError::Parse(
            "missing SCRIPTMETA distribution block".to_string(),
        ))
    }
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

    pub fn new_with_cancellation(
        options: DistributionResolverOptions,
        cancellation: OperationCancellation,
    ) -> ScriptMetaKitResult<Self> {
        Self::new_with_cancellation_and_retry(options, cancellation, 0)
    }

    pub(crate) fn new_with_cancellation_and_retry(
        options: DistributionResolverOptions,
        cancellation: OperationCancellation,
        source_retry_attempts: usize,
    ) -> ScriptMetaKitResult<Self> {
        Self::new_with_cancellation_retry_and_http_cache(
            options,
            cancellation,
            source_retry_attempts,
            HttpValidationCache::default(),
        )
    }

    pub(crate) fn new_with_cancellation_retry_and_http_cache(
        options: DistributionResolverOptions,
        cancellation: OperationCancellation,
        source_retry_attempts: usize,
        http_validation_cache: HttpValidationCache,
    ) -> ScriptMetaKitResult<Self> {
        Ok(Self {
            distribution_resolver:
                DistributionResolver::new_with_cancellation_retry_and_http_cache(
                    options,
                    Some(cancellation),
                    source_retry_attempts,
                    http_validation_cache,
                )?,
        })
    }

    pub(crate) fn terminal_source_failure(
        &self,
        url: &Url,
    ) -> ScriptMetaKitResult<Option<Arc<ScriptMetaKitError>>> {
        self.distribution_resolver.terminal_source_failure(url)
    }

    pub(crate) fn is_terminal_source_failure(
        &self,
        error: &ScriptMetaKitError,
    ) -> ScriptMetaKitResult<bool> {
        self.distribution_resolver.is_terminal_source_failure(error)
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

    #[cfg(target_os = "macos")]
    use std::{fs::File, thread, time::Duration};

    use super::{
        DIST_BEGIN_BYTES, DistributionResolver, DistributionResolverOptions,
        MAX_PARSED_CACHE_COUNT, MAX_SOURCE_CACHE_COUNT, SOURCE_READ_CHUNK_BYTES,
        gist_raw_content_urls, github_directory_raw_content_urls,
        github_repository_raw_content_urls, is_gist_url, is_github_directory_url,
        is_github_repository_url, is_retryable_source_error, read_metadata_source_block,
        read_metadata_source_block_controlled,
    };
    #[cfg(target_os = "macos")]
    use super::{MetadataSourceReadError, read_metadata_source_file_controlled};
    #[cfg(target_os = "macos")]
    use crate::OperationCancellation;
    use url::Url;

    #[test]
    fn retry_disposition_distinguishes_terminal_and_transient_failures() {
        use crate::ScriptMetaKitError;

        assert!(!is_retryable_source_error(&ScriptMetaKitError::Url(
            "unsupported Meta-URL scheme `ftp`".to_string()
        )));
        assert!(!is_retryable_source_error(&ScriptMetaKitError::Url(
            "http_status=404: 404 Not Found".to_string()
        )));
        assert!(is_retryable_source_error(&ScriptMetaKitError::Url(
            "http_status=503: 503 Service Unavailable".to_string()
        )));
        for status in [408, 425, 429] {
            assert!(is_retryable_source_error(&ScriptMetaKitError::Url(
                format!("http_status={status}: transient response")
            )));
        }
        assert!(!is_retryable_source_error(&ScriptMetaKitError::Io {
            path: "denied".into(),
            message: "kind=PermissionDenied: denied".to_string(),
        }));
        assert!(is_retryable_source_error(&ScriptMetaKitError::Io {
            path: "missing".into(),
            message: "kind=NotFound: temporarily missing".to_string(),
        }));
        assert!(!is_retryable_source_error(&ScriptMetaKitError::Timeout(
            "metadata resolution was cancelled".to_string()
        )));
    }

    #[cfg(feature = "blocking-http")]
    #[test]
    fn github_and_gist_candidate_search_preserves_transient_fetch_failure() {
        let transient = spawn_status_once("503 Service Unavailable");
        let terminal = spawn_status_once("404 Not Found");
        let original = Url::parse("https://github.com/example/repository").expect("original");
        let resolver =
            DistributionResolver::new(DistributionResolverOptions::default()).expect("resolver");

        let error = resolver
            .load_first_valid_metadata_source(
                &original,
                vec![transient, terminal],
                "SCRIPTMETA.txt was not found",
            )
            .expect_err("candidate search");
        assert!(is_retryable_source_error(&error));
        assert!(error.to_string().contains("503"));
    }

    #[cfg(feature = "blocking-http")]
    fn spawn_status_once(status: &'static str) -> Url {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            thread,
        };

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let response =
                format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            stream.write_all(response.as_bytes()).expect("response");
        });
        Url::parse(&format!("http://{address}/SCRIPTMETA.txt")).expect("URL")
    }

    #[test]
    fn rejects_metadata_source_after_total_byte_limit() {
        let mut source = vec![b'x'; 128];
        source.extend_from_slice(
            b"SCRIPTMETA-DIST-BEGIN\nScript-ID=com.example.limit\nSCRIPTMETA-DIST-END\n",
        );
        let mut reader = Cursor::new(source);
        let error = read_metadata_source_block_controlled(&mut reader, 1024, 64, None, None)
            .expect_err("source must be bounded independently of metadata block size");
        assert!(
            matches!(error, super::MetadataSourceReadError::Parse(message) if message.contains("64 byte limit"))
        );
    }

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

    #[cfg(target_os = "macos")]
    #[test]
    fn interrupts_active_macos_file_read_at_resource_timeout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("large-source.txt");
        File::create(&source)
            .expect("create sparse source")
            .set_len(1024 * 1024 * 1024)
            .expect("size sparse source");

        let started = std::time::Instant::now();
        let error = read_metadata_source_file_controlled(
            &source,
            1024,
            DistributionResolverOptions::default().max_source_bytes,
            Some(5),
            None,
        )
        .expect_err("large active read should time out");

        assert!(
            matches!(error, MetadataSourceReadError::Timeout(_)),
            "unexpected error: {error:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn interrupts_active_macos_file_read_when_operation_is_cancelled() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("large-source.txt");
        File::create(&source)
            .expect("create sparse source")
            .set_len(1024 * 1024 * 1024)
            .expect("size sparse source");

        let cancellation = OperationCancellation::new();
        let _scope = cancellation.begin_scope();
        let cancellation_request = cancellation.clone();
        let canceller = thread::spawn(move || {
            thread::sleep(Duration::from_millis(5));
            cancellation_request.cancel();
        });

        let started = std::time::Instant::now();
        let error = read_metadata_source_file_controlled(
            &source,
            1024,
            DistributionResolverOptions::default().max_source_bytes,
            Some(5_000),
            Some(&cancellation),
        )
        .expect_err("cancelled active read should stop");
        canceller.join().expect("canceller");

        assert!(
            matches!(error, MetadataSourceReadError::Cancelled),
            "unexpected error: {error:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(1));
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
