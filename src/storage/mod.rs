use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use crate::{
    catalog::{CacheScope, RootRegistration, ScriptMetaKitConfig},
    core::{ScriptMetaKitError, ScriptMetaKitResult},
    now_timestamp_millis,
};

pub const MAX_CACHE_FILE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CacheSchema {
    pub schema_version: u32,
    #[serde(default)]
    pub min_supported_schema_version: u32,
    pub package_version: String,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub cache_namespace: Option<String>,
    #[serde(default)]
    pub created_at_millis: u64,
}

impl CacheSchema {
    pub const CURRENT_SCHEMA_VERSION: u32 = 2;
    pub const MIN_SUPPORTED_SCHEMA_VERSION: u32 = 1;

    #[must_use]
    pub fn current() -> Self {
        Self {
            schema_version: Self::CURRENT_SCHEMA_VERSION,
            min_supported_schema_version: Self::MIN_SUPPORTED_SCHEMA_VERSION,
            package_version: crate::package_version().to_string(),
            app_id: None,
            cache_namespace: None,
            created_at_millis: now_timestamp_millis(),
        }
    }

    #[must_use]
    pub fn current_for_config(config: &ScriptMetaKitConfig) -> Self {
        Self {
            app_id: Some(config.app_id.clone()),
            cache_namespace: Some(config.cache_namespace.clone()),
            ..Self::current()
        }
    }

    #[must_use]
    pub fn is_supported(&self) -> bool {
        (Self::MIN_SUPPORTED_SCHEMA_VERSION..=Self::CURRENT_SCHEMA_VERSION)
            .contains(&self.schema_version)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CachePayload {
    pub schema: CacheSchema,
    pub scope: CacheScope,
    pub data: Value,
}

impl CachePayload {
    #[must_use]
    pub fn new(scope: CacheScope, data: Value) -> Self {
        Self {
            schema: CacheSchema::current(),
            scope,
            data,
        }
    }

    #[must_use]
    pub fn new_for_config(scope: CacheScope, data: Value, config: &ScriptMetaKitConfig) -> Self {
        Self {
            schema: CacheSchema::current_for_config(config),
            scope,
            data,
        }
    }

    #[must_use]
    pub fn is_supported_schema(&self) -> bool {
        self.schema.is_supported()
    }

    pub fn migrate(mut self) -> ScriptMetaKitResult<Self> {
        if !self.is_supported_schema() {
            return Err(ScriptMetaKitError::Cache(format!(
                "unsupported cache schema version {}",
                self.schema.schema_version
            )));
        }
        if self.schema.schema_version < CacheSchema::CURRENT_SCHEMA_VERSION {
            self.schema.schema_version = CacheSchema::CURRENT_SCHEMA_VERSION;
            self.schema.min_supported_schema_version = CacheSchema::MIN_SUPPORTED_SCHEMA_VERSION;
            if self.schema.created_at_millis == 0 {
                self.schema.created_at_millis = now_timestamp_millis();
            }
        }
        Ok(self)
    }

    pub fn validate_for_config(&self, config: &ScriptMetaKitConfig) -> ScriptMetaKitResult<()> {
        if let Some(app_id) = self.schema.app_id.as_deref()
            && app_id != config.app_id
        {
            return Err(ScriptMetaKitError::Cache(format!(
                "cache app_id `{app_id}` does not match `{}`",
                config.app_id
            )));
        }
        if let Some(namespace) = self.schema.cache_namespace.as_deref()
            && namespace != config.cache_namespace
        {
            return Err(ScriptMetaKitError::Cache(format!(
                "cache namespace `{namespace}` does not match `{}`",
                config.cache_namespace
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CacheRootSignature {
    pub root_id: String,
    pub path: String,
}

#[must_use]
pub fn cache_root_signatures(roots: &[RootRegistration]) -> Vec<CacheRootSignature> {
    let mut signatures = roots
        .iter()
        .map(|root| CacheRootSignature {
            root_id: root.root_id.to_string(),
            path: root.path.to_string_lossy().into_owned(),
        })
        .collect::<Vec<_>>();
    signatures.sort_by(|lhs, rhs| lhs.root_id.cmp(&rhs.root_id));
    signatures
}

pub fn save_cache_payload(
    path: impl AsRef<Path>,
    payload: &CachePayload,
) -> ScriptMetaKitResult<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| ScriptMetaKitError::Io {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })?;
    }

    let temp_path = temporary_cache_path(path);
    let mut temp_guard = TemporaryCacheFile::new(temp_path.clone());
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|error| ScriptMetaKitError::Io {
            path: temp_path.clone(),
            message: error.to_string(),
        })?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, payload)
        .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
    writer.flush().map_err(|error| ScriptMetaKitError::Io {
        path: temp_path.clone(),
        message: error.to_string(),
    })?;
    let file = writer
        .into_inner()
        .map_err(|error| ScriptMetaKitError::Io {
            path: temp_path.clone(),
            message: error.to_string(),
        })?;
    file.sync_all().map_err(|error| ScriptMetaKitError::Io {
        path: temp_path.clone(),
        message: error.to_string(),
    })?;
    replace_cache_file(&temp_path, path)?;
    temp_guard.disarm();
    Ok(())
}

pub fn cache_payload_content_fingerprint(payload: &CachePayload) -> ScriptMetaKitResult<String> {
    #[derive(Serialize)]
    struct FingerprintPayload<'a> {
        schema_version: u32,
        min_supported_schema_version: u32,
        package_version: &'a str,
        app_id: Option<&'a str>,
        cache_namespace: Option<&'a str>,
        scope: CacheScope,
        data: &'a Value,
    }

    let bytes = serde_json::to_vec(&FingerprintPayload {
        schema_version: payload.schema.schema_version,
        min_supported_schema_version: payload.schema.min_supported_schema_version,
        package_version: &payload.schema.package_version,
        app_id: payload.schema.app_id.as_deref(),
        cache_namespace: payload.schema.cache_namespace.as_deref(),
        scope: payload.scope,
        data: &payload.data,
    })
    .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn load_cache_payload(path: impl AsRef<Path>) -> ScriptMetaKitResult<CachePayload> {
    let path = path.as_ref();
    let file_size = fs::metadata(path)
        .map_err(|error| ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?
        .len();
    if file_size > MAX_CACHE_FILE_BYTES {
        return Err(ScriptMetaKitError::Cache(format!(
            "cache file exceeds the {} byte safety limit",
            MAX_CACHE_FILE_BYTES
        )));
    }
    let file = File::open(path).map_err(|error| ScriptMetaKitError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let payload: CachePayload = serde_json::from_reader(BufReader::new(file))
        .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
    payload.migrate()
}

fn temporary_cache_path(path: &Path) -> PathBuf {
    let mut file_name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    file_name.push(format!(".tmp.{}", uuid::Uuid::new_v4()));
    path.with_file_name(file_name)
}

struct TemporaryCacheFile {
    path: PathBuf,
    cleanup: bool,
}

impl TemporaryCacheFile {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            cleanup: true,
        }
    }

    fn disarm(&mut self) {
        self.cleanup = false;
    }
}

impl Drop for TemporaryCacheFile {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
fn replace_cache_file(temp_path: &Path, destination: &Path) -> ScriptMetaKitResult<()> {
    fs::rename(temp_path, destination).map_err(|error| ScriptMetaKitError::Io {
        path: destination.to_path_buf(),
        message: error.to_string(),
    })?;
    sync_parent_directory(destination).map_err(|error| ScriptMetaKitError::Io {
        path: destination.to_path_buf(),
        message: error.to_string(),
    })
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    File::open(parent)?.sync_all()
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn replace_cache_file(temp_path: &Path, destination: &Path) -> ScriptMetaKitResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let temp_path_wide = temp_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: the UTF-16 buffers are null-terminated and remain alive for the
    // duration of the call. MoveFileExW does not retain the pointers.
    let moved = unsafe {
        MoveFileExW(
            temp_path_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(ScriptMetaKitError::Io {
            path: destination.to_path_buf(),
            message: std::io::Error::last_os_error().to_string(),
        });
    }
    Ok(())
}
