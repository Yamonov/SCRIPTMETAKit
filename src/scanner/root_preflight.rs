use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    OperationCancellation,
    catalog::{RootError, RootRegistration, RootSnapshot, RootStatus},
    scanner::{ExtensionPolicy, ScannerOptions},
    watcher::normalize_path,
};

use super::path_resolution::resolve_scannable_path;

#[must_use]
pub fn can_read_directory_contents(path: &Path) -> bool {
    fs::read_dir(path).is_ok()
}

pub(crate) fn root_location_issue(
    root_path: &Path,
    options: &ScannerOptions,
) -> Option<(RootStatus, RootError)> {
    if options.root_preflight.reject_trash_roots && is_trash_path(root_path) {
        return Some((
            RootStatus::Unreadable,
            RootError {
                code: "trash".to_string(),
                message: "root path is in Trash".to_string(),
            },
        ));
    }

    if options.root_preflight.reject_restricted_roots && is_restricted_registered_root(root_path) {
        return Some((
            RootStatus::Unreadable,
            RootError {
                code: "restricted_root".to_string(),
                message: "root path is too broad for script scanning".to_string(),
            },
        ));
    }

    None
}

pub(crate) fn root_content_preflight_issue(
    root_path: &Path,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    cancellation: Option<&OperationCancellation>,
) -> Option<(RootStatus, RootError)> {
    if let Some(issue) = root_location_issue(root_path, options) {
        return Some(issue);
    }

    if !options.root_preflight.reject_low_script_density_large_roots {
        return None;
    }

    let scan = scan_root_content(root_path, options, extensions, cancellation);
    root_content_issue_from_scan(&scan, options)
}

fn root_content_issue_from_scan(
    scan: &RootContentScan,
    options: &ScannerOptions,
) -> Option<(RootStatus, RootError)> {
    if scan.cancelled {
        return Some((
            RootStatus::Cancelled,
            RootError {
                code: "cancelled".to_string(),
                message: "root preflight was cancelled".to_string(),
            },
        ));
    }
    let reached_meaningful_time_limit = scan.reached_time_limit
        && scan.scanned_item_count >= options.root_preflight.min_scanned_items_for_time_limit;
    if !scan.reached_item_limit && !reached_meaningful_time_limit {
        return None;
    }
    if scan.scanned_file_count < options.root_preflight.min_scanned_file_count_for_large_root {
        return None;
    }
    let ratio_denominator = options.root_preflight.min_script_ratio_denominator.max(1);
    if scan.script_file_count.saturating_mul(ratio_denominator) >= scan.scanned_file_count {
        return None;
    }

    Some((
        RootStatus::Overflowed,
        RootError {
            code: "too_large_for_script_folder".to_string(),
            message: format!(
                "root preflight scanned {} files and found {} script files before stopping",
                scan.scanned_file_count, scan.script_file_count
            ),
        },
    ))
}

pub(crate) fn preflight_root_registration(
    root: &RootRegistration,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    cancellation: Option<&OperationCancellation>,
) -> RootSnapshot {
    let mut snapshot = RootSnapshot::new(root.root_id.clone(), root.path.clone());
    if cancellation.is_some_and(OperationCancellation::is_cancelled) {
        snapshot.status = RootStatus::Cancelled;
        snapshot.error = Some(RootError {
            code: "cancelled".to_string(),
            message: "root preflight was cancelled".to_string(),
        });
        return snapshot;
    }
    match root.path.try_exists() {
        Ok(true) => {}
        Ok(false) => {
            snapshot.status = RootStatus::Missing;
            snapshot.error = Some(RootError {
                code: "missing_root".to_string(),
                message: "root path does not exist".to_string(),
            });
            return snapshot;
        }
        Err(error) => {
            snapshot.status = RootStatus::Unreadable;
            snapshot.error = Some(RootError {
                code: "root_path_check_failed".to_string(),
                message: error.to_string(),
            });
            return snapshot;
        }
    }
    if let Some((status, error)) = root_location_issue(&root.path, options) {
        snapshot.status = status;
        snapshot.error = Some(error);
        return snapshot;
    }
    let root_resolution = resolve_scannable_path(
        root.path.clone(),
        root.path.clone(),
        options,
        Some(extensions),
    );
    if root_resolution.is_unfollowed_symlink() {
        snapshot.status = RootStatus::Unreadable;
        snapshot.error = Some(RootError {
            code: "symlink_following_disabled".to_string(),
            message: "root is a symlink and symlink following is disabled".to_string(),
        });
        return snapshot;
    }
    if !fs::metadata(&root_resolution.resolved_path).is_ok_and(|metadata| metadata.is_dir()) {
        snapshot.status = RootStatus::Missing;
        snapshot.error = Some(RootError {
            code: "not_directory".to_string(),
            message: "root path does not resolve to a directory".to_string(),
        });
        return snapshot;
    }
    if let Some((status, error)) = root_content_preflight_issue(
        &root_resolution.resolved_path,
        options,
        extensions,
        cancellation,
    ) {
        snapshot.status = status;
        snapshot.error = Some(error);
        return snapshot;
    }
    snapshot.status = RootStatus::Ready;
    snapshot.is_dirty = false;
    snapshot
}

pub(crate) struct RootContentPreflightTracker<'a> {
    options: &'a ScannerOptions,
    extensions: &'a ExtensionPolicy,
    started: Instant,
    timeout: Option<Duration>,
    scan: RootContentScan,
    completed: bool,
}

impl<'a> RootContentPreflightTracker<'a> {
    pub(crate) fn new(options: &'a ScannerOptions, extensions: &'a ExtensionPolicy) -> Self {
        Self {
            options,
            extensions,
            started: Instant::now(),
            timeout: (options.root_preflight.max_duration_millis > 0)
                .then(|| Duration::from_millis(options.root_preflight.max_duration_millis)),
            scan: RootContentScan::default(),
            completed: !options.root_preflight.reject_low_script_density_large_roots,
        }
    }

    pub(crate) fn observe_entry(
        &mut self,
        entry: &fs::DirEntry,
        display_path: &Path,
    ) -> Option<(RootStatus, RootError)> {
        if self.completed {
            return None;
        }
        if self.scan.scanned_item_count >= self.options.root_preflight.max_scanned_items {
            self.scan.reached_item_limit = true;
        } else if self
            .timeout
            .is_some_and(|timeout| self.started.elapsed() >= timeout)
        {
            self.scan.reached_time_limit = true;
        }
        if self.scan.reached_item_limit || self.scan.reached_time_limit {
            self.completed = true;
            return root_content_issue_from_scan(&self.scan, self.options);
        }

        self.scan.scanned_item_count += 1;
        if should_skip_preflight_path(display_path, self.options) {
            return None;
        }
        let Ok(file_type) = entry.file_type() else {
            return None;
        };
        if file_type.is_file() {
            self.scan.scanned_file_count += 1;
            if self.extensions.contains_path(display_path) {
                self.scan.script_file_count += 1;
            }
        }
        None
    }
}

#[derive(Default)]
struct RootContentScan {
    scanned_item_count: usize,
    scanned_file_count: usize,
    script_file_count: usize,
    reached_item_limit: bool,
    reached_time_limit: bool,
    cancelled: bool,
}

struct RootContentScanControl<'a> {
    options: &'a ScannerOptions,
    extensions: &'a ExtensionPolicy,
    started: Instant,
    timeout: Option<Duration>,
    cancellation: Option<&'a OperationCancellation>,
}

fn scan_root_content(
    root_path: &Path,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    cancellation: Option<&OperationCancellation>,
) -> RootContentScan {
    let mut scan = RootContentScan::default();
    let control = RootContentScanControl {
        options,
        extensions,
        started: Instant::now(),
        timeout: (options.root_preflight.max_duration_millis > 0)
            .then(|| Duration::from_millis(options.root_preflight.max_duration_millis)),
        cancellation,
    };
    let mut pending_directories = vec![root_path.to_path_buf()];

    while let Some(directory) = pending_directories.pop() {
        if control
            .cancellation
            .is_some_and(OperationCancellation::is_cancelled)
        {
            scan.cancelled = true;
            break;
        }
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        scan_entries(entries, &control, &mut pending_directories, &mut scan);
        if scan.reached_item_limit || scan.reached_time_limit {
            break;
        }
    }

    scan
}

fn scan_entries(
    entries: fs::ReadDir,
    control: &RootContentScanControl<'_>,
    pending_directories: &mut Vec<PathBuf>,
    scan: &mut RootContentScan,
) {
    for entry in entries {
        if control
            .cancellation
            .is_some_and(OperationCancellation::is_cancelled)
        {
            scan.cancelled = true;
            break;
        }
        if scan.scanned_item_count >= control.options.root_preflight.max_scanned_items {
            scan.reached_item_limit = true;
            break;
        }
        if let Some(timeout) = control.timeout
            && control.started.elapsed() >= timeout
        {
            scan.reached_time_limit = true;
            break;
        }

        let Ok(entry) = entry else {
            continue;
        };
        scan.scanned_item_count += 1;
        let path = entry.path();
        if should_skip_preflight_path(&path, control.options) {
            continue;
        }

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            pending_directories.push(path);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        scan.scanned_file_count += 1;
        if control.extensions.contains_path(&path) {
            scan.script_file_count += 1;
        }
    }
}

fn should_skip_preflight_path(path: &Path, options: &ScannerOptions) -> bool {
    if options.skip_hidden && is_hidden_path(path) {
        return true;
    }
    options.skip_packages && is_package_path(path)
}

fn is_hidden_path(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."))
        || platform_path_is_hidden(path)
}

#[cfg(windows)]
fn platform_path_is_hidden(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;

    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN;

    fs::metadata(path)
        .map(|metadata| metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn platform_path_is_hidden(_path: &Path) -> bool {
    false
}

#[cfg(not(windows))]
fn is_restricted_registered_root(root_path: &Path) -> bool {
    let normalized_root = normalize_path(root_path);
    let mut restricted_paths = vec![
        PathBuf::from("/"),
        PathBuf::from("/Applications"),
        PathBuf::from("/Library"),
        PathBuf::from("/System"),
        PathBuf::from("/Users"),
        PathBuf::from("/Volumes"),
    ];
    if let Some(home) = env::var_os("HOME") {
        let home = normalize_path(Path::new(&home));
        restricted_paths.push(home.clone());
        restricted_paths.push(home.join("Library"));
    }

    restricted_paths
        .into_iter()
        .any(|path| normalize_path(&path) == normalized_root)
}

#[cfg(windows)]
fn is_restricted_registered_root(root_path: &Path) -> bool {
    let normalized_root = normalize_path(root_path);
    if is_windows_drive_root(&normalized_root) {
        return true;
    }

    let mut restricted_paths = Vec::new();
    for variable in [
        "SystemRoot",
        "WINDIR",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "USERPROFILE",
    ] {
        if let Some(path) = env::var_os(variable) {
            restricted_paths.push(PathBuf::from(path));
        }
    }
    if let Some(system_drive) = env::var_os("SystemDrive") {
        restricted_paths.push(PathBuf::from(format!(
            r"{}\Users",
            trim_windows_root_separator(&system_drive.to_string_lossy())
        )));
    }
    if let (Some(home_drive), Some(home_path)) = (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH"))
    {
        let home_drive = home_drive.to_string_lossy();
        let home_path = home_path.to_string_lossy();
        let separator = if home_path.starts_with(['\\', '/']) {
            ""
        } else {
            r"\"
        };
        restricted_paths.push(PathBuf::from(format!(
            "{}{}{}",
            trim_windows_root_separator(&home_drive),
            separator,
            home_path
        )));
    }

    restricted_paths
        .into_iter()
        .any(|path| normalize_path(&path) == normalized_root)
}

#[cfg(windows)]
fn trim_windows_root_separator(path: &str) -> &str {
    path.trim_end_matches(['\\', '/'])
}

#[cfg(windows)]
fn is_windows_drive_root(path: &Path) -> bool {
    use std::path::Component;

    let mut components = path.components();
    matches!(components.next(), Some(Component::Prefix(_)))
        && matches!(components.next(), Some(Component::RootDir))
        && components.next().is_none()
}

fn is_trash_path(root_path: &Path) -> bool {
    let normalized_root = normalize_path(root_path);
    #[cfg(windows)]
    if normalized_root.components().any(|component| {
        component
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("$Recycle.Bin")
    }) {
        return true;
    }

    if normalized_root
        .components()
        .any(|component| component.as_os_str() == ".Trashes")
    {
        return true;
    }

    let Some(home) = env::var_os("HOME") else {
        return false;
    };
    let home_trash = normalize_path(Path::new(&home).join(".Trash").as_path());
    normalized_root == home_trash || normalized_root.starts_with(home_trash)
}

fn is_package_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "app" | "bundle" | "framework" | "plugin" | "appex"
            )
        })
}
