use std::{collections::BTreeSet, path::Path};

use serde::{Deserialize, Serialize};

use crate::formats::DEFAULT_SCRIPT_EXTENSIONS;

pub const DEFAULT_SCAN_TIMEOUT_PER_ROOT_MILLIS: u64 = 30_000;
pub const DEFAULT_ROOT_PREFLIGHT_MAX_SCANNED_ITEMS: usize = 10_000;
pub const DEFAULT_ROOT_PREFLIGHT_MAX_DURATION_MILLIS: u64 = 1_000;
pub const DEFAULT_ROOT_PREFLIGHT_MIN_SCANNED_FILE_COUNT_FOR_LARGE_ROOT: usize = 1_000;
pub const DEFAULT_ROOT_PREFLIGHT_MIN_SCRIPT_RATIO_DENOMINATOR: usize = 100;
pub const DEFAULT_ROOT_PREFLIGHT_MIN_SCANNED_ITEMS_FOR_TIME_LIMIT: usize = 500;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExtensionPolicy {
    extensions: BTreeSet<String>,
}

impl ExtensionPolicy {
    #[must_use]
    pub fn new<I, S>(extensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            extensions: extensions
                .into_iter()
                .map(|extension| normalize_extension(extension.as_ref()))
                .filter(|extension| !extension.is_empty())
                .collect(),
        }
    }

    #[must_use]
    pub fn script_default() -> Self {
        Self::new(DEFAULT_SCRIPT_EXTENSIONS.iter().copied())
    }

    #[must_use]
    pub fn contains_path(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| self.contains_extension(extension))
    }

    #[must_use]
    pub fn contains_extension(&self, extension: &str) -> bool {
        let extension = extension.trim().trim_start_matches('.');
        if extension.is_empty() {
            return false;
        }

        if extension.bytes().any(|byte| byte.is_ascii_uppercase()) {
            let normalized = extension.to_ascii_lowercase();
            self.extensions.contains(normalized.as_str())
        } else {
            self.extensions.contains(extension)
        }
    }

    #[must_use]
    pub fn extensions(&self) -> &BTreeSet<String> {
        &self.extensions
    }
}

impl Default for ExtensionPolicy {
    fn default() -> Self {
        Self::script_default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScannerOptions {
    pub max_depth: usize,
    pub max_nodes_per_root: usize,
    pub max_prefix_bytes: usize,
    pub skip_hidden: bool,
    pub skip_packages: bool,
    pub follow_symlinks: bool,
    pub resolve_macos_alias: bool,
    pub reuse_unchanged_records: bool,
    #[serde(default)]
    pub decompile_compiled_osa_during_scan: bool,
    #[serde(default)]
    pub include_empty_directories: bool,
    #[serde(default)]
    pub root_preflight: RootPreflightOptions,
    #[serde(default = "default_scan_timeout_per_root_millis")]
    pub scan_timeout_per_root_millis: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RootPreflightOptions {
    #[serde(default = "default_true")]
    pub reject_trash_roots: bool,
    #[serde(default = "default_true")]
    pub reject_restricted_roots: bool,
    #[serde(default = "default_true")]
    pub reject_low_script_density_large_roots: bool,
    #[serde(default = "default_root_preflight_max_scanned_items")]
    pub max_scanned_items: usize,
    #[serde(default = "default_root_preflight_max_duration_millis")]
    pub max_duration_millis: u64,
    #[serde(default = "default_root_preflight_min_scanned_file_count_for_large_root")]
    pub min_scanned_file_count_for_large_root: usize,
    #[serde(default = "default_root_preflight_min_script_ratio_denominator")]
    pub min_script_ratio_denominator: usize,
    #[serde(default = "default_root_preflight_min_scanned_items_for_time_limit")]
    pub min_scanned_items_for_time_limit: usize,
}

impl Default for RootPreflightOptions {
    fn default() -> Self {
        Self {
            reject_trash_roots: true,
            reject_restricted_roots: true,
            reject_low_script_density_large_roots: true,
            max_scanned_items: DEFAULT_ROOT_PREFLIGHT_MAX_SCANNED_ITEMS,
            max_duration_millis: DEFAULT_ROOT_PREFLIGHT_MAX_DURATION_MILLIS,
            min_scanned_file_count_for_large_root:
                DEFAULT_ROOT_PREFLIGHT_MIN_SCANNED_FILE_COUNT_FOR_LARGE_ROOT,
            min_script_ratio_denominator: DEFAULT_ROOT_PREFLIGHT_MIN_SCRIPT_RATIO_DENOMINATOR,
            min_scanned_items_for_time_limit:
                DEFAULT_ROOT_PREFLIGHT_MIN_SCANNED_ITEMS_FOR_TIME_LIMIT,
        }
    }
}

impl Default for ScannerOptions {
    fn default() -> Self {
        Self {
            max_depth: 24,
            max_nodes_per_root: 10_000,
            max_prefix_bytes: 128 * 1024,
            skip_hidden: true,
            skip_packages: true,
            follow_symlinks: true,
            resolve_macos_alias: true,
            reuse_unchanged_records: true,
            decompile_compiled_osa_during_scan: false,
            include_empty_directories: false,
            root_preflight: RootPreflightOptions::default(),
            scan_timeout_per_root_millis: default_scan_timeout_per_root_millis(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_scan_timeout_per_root_millis() -> Option<u64> {
    Some(DEFAULT_SCAN_TIMEOUT_PER_ROOT_MILLIS)
}

fn default_root_preflight_max_scanned_items() -> usize {
    DEFAULT_ROOT_PREFLIGHT_MAX_SCANNED_ITEMS
}

fn default_root_preflight_max_duration_millis() -> u64 {
    DEFAULT_ROOT_PREFLIGHT_MAX_DURATION_MILLIS
}

fn default_root_preflight_min_scanned_file_count_for_large_root() -> usize {
    DEFAULT_ROOT_PREFLIGHT_MIN_SCANNED_FILE_COUNT_FOR_LARGE_ROOT
}

fn default_root_preflight_min_script_ratio_denominator() -> usize {
    DEFAULT_ROOT_PREFLIGHT_MIN_SCRIPT_RATIO_DENOMINATOR
}

fn default_root_preflight_min_scanned_items_for_time_limit() -> usize {
    DEFAULT_ROOT_PREFLIGHT_MIN_SCANNED_ITEMS_FOR_TIME_LIMIT
}

fn normalize_extension(extension: &str) -> String {
    extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}
