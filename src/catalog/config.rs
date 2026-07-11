use serde::{Deserialize, Serialize};

use crate::{
    core::ParserOptions,
    scanner::{ExtensionPolicy, ScannerOptions},
    watcher::{
        DEFAULT_DEBOUNCE_DELAY_MILLIS, DEFAULT_MAX_DELIVERY_DELAY_MILLIS,
        DEFAULT_MAX_PENDING_PATHS, DEFAULT_NATIVE_EVENT_LATENCY_MILLIS, MonitorRootStrategy,
        OverflowPolicy, WatchPolicy,
    },
};

pub const DEFAULT_UPDATE_REQUEST_TIMEOUT_MILLIS: u64 = 15_000;
pub const DEFAULT_UPDATE_RESOURCE_TIMEOUT_MILLIS: u64 = 15_000;
pub const DEFAULT_UPDATE_RETRY_INITIAL_DELAY_MILLIS: u64 = 500;
pub const DEFAULT_UPDATE_RETRY_BACKOFF_MULTIPLIER: u32 = 3;
pub const DEFAULT_UPDATE_MAX_RETRY_DELAY_MILLIS: u64 = 30_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetaKitConfig {
    pub app_id: String,
    pub cache_namespace: String,
    pub supported_extensions: ExtensionPolicy,
    pub parser: ParserOptions,
    pub scanner: ScannerOptions,
    pub watcher: WatcherOptions,
    pub cache: CacheOptions,
    pub update_check: UpdateCheckOptions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WatcherOptions {
    pub enabled: bool,
    pub watch_policy: WatchPolicy,
    pub debounce_delay_millis: u64,
    pub max_delivery_delay_millis: u64,
    #[serde(default = "default_native_event_latency_millis")]
    pub native_event_latency_millis: u64,
    pub max_pending_paths: usize,
    pub overflow_policy: OverflowPolicy,
    pub monitor_root_strategy: MonitorRootStrategy,
}

impl Default for WatcherOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            watch_policy: WatchPolicy::VisibleOnly,
            debounce_delay_millis: DEFAULT_DEBOUNCE_DELAY_MILLIS,
            max_delivery_delay_millis: DEFAULT_MAX_DELIVERY_DELAY_MILLIS,
            native_event_latency_millis: DEFAULT_NATIVE_EVENT_LATENCY_MILLIS,
            max_pending_paths: DEFAULT_MAX_PENDING_PATHS,
            overflow_policy: OverflowPolicy::MarkAffectedRootsDirty,
            monitor_root_strategy: MonitorRootStrategy::PlatformRecommended,
        }
    }
}

fn default_native_event_latency_millis() -> u64 {
    DEFAULT_NATIVE_EVENT_LATENCY_MILLIS
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CacheOptions {
    pub enabled: bool,
    pub memory_cache: bool,
    pub persistent_cache: bool,
    pub idle_lifetime_millis: u64,
    pub max_memory_nodes: usize,
    pub max_deferred_dirty_directories: usize,
    pub preserve_update_results: bool,
}

impl Default for CacheOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            memory_cache: true,
            persistent_cache: true,
            idle_lifetime_millis: 20 * 60 * 1_000,
            max_memory_nodes: 60_000,
            max_deferred_dirty_directories: 128,
            preserve_update_results: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateCheckOptions {
    pub enabled: bool,
    pub max_concurrent_meta_url_checks: usize,
    pub retry_attempts: usize,
    #[serde(default = "default_update_retry_initial_delay_millis")]
    pub retry_initial_delay_millis: u64,
    #[serde(default = "default_update_retry_backoff_multiplier")]
    pub retry_backoff_multiplier: u32,
    #[serde(default = "default_update_max_retry_delay_millis")]
    pub max_retry_delay_millis: u64,
    #[serde(default = "default_update_request_timeout_millis")]
    pub request_timeout_millis: Option<u64>,
    #[serde(default = "default_update_resource_timeout_millis")]
    pub resource_timeout_millis: Option<u64>,
    pub cache_network_responses: bool,
}

impl Default for UpdateCheckOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent_meta_url_checks: 6,
            retry_attempts: 2,
            retry_initial_delay_millis: default_update_retry_initial_delay_millis(),
            retry_backoff_multiplier: default_update_retry_backoff_multiplier(),
            max_retry_delay_millis: default_update_max_retry_delay_millis(),
            request_timeout_millis: default_update_request_timeout_millis(),
            resource_timeout_millis: default_update_resource_timeout_millis(),
            cache_network_responses: false,
        }
    }
}

fn default_update_retry_initial_delay_millis() -> u64 {
    DEFAULT_UPDATE_RETRY_INITIAL_DELAY_MILLIS
}

fn default_update_retry_backoff_multiplier() -> u32 {
    DEFAULT_UPDATE_RETRY_BACKOFF_MULTIPLIER
}

fn default_update_max_retry_delay_millis() -> u64 {
    DEFAULT_UPDATE_MAX_RETRY_DELAY_MILLIS
}

fn default_update_request_timeout_millis() -> Option<u64> {
    Some(DEFAULT_UPDATE_REQUEST_TIMEOUT_MILLIS)
}

fn default_update_resource_timeout_millis() -> Option<u64> {
    Some(DEFAULT_UPDATE_RESOURCE_TIMEOUT_MILLIS)
}

impl ScriptMetaKitConfig {
    #[must_use]
    pub fn new(app_id: impl Into<String>, cache_namespace: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            cache_namespace: cache_namespace.into(),
            supported_extensions: ExtensionPolicy::script_default(),
            parser: ParserOptions::scripta_compatible(),
            scanner: ScannerOptions::default(),
            watcher: WatcherOptions::default(),
            cache: CacheOptions::default(),
            update_check: UpdateCheckOptions::default(),
        }
    }
}

impl Default for ScriptMetaKitConfig {
    fn default() -> Self {
        Self::new("ScriptMetaKit", "ScriptMetaKit")
    }
}
