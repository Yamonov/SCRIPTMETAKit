use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    catalog::{RootError, RootStatus},
    scanner::{ExtensionPolicy, ScannerOptions},
    watcher::normalize_path,
};

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
) -> Option<(RootStatus, RootError)> {
    if let Some(issue) = root_location_issue(root_path, options) {
        return Some(issue);
    }

    if !options.root_preflight.reject_low_script_density_large_roots {
        return None;
    }

    let scan = scan_root_content(root_path, options, extensions);
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

#[derive(Default)]
struct RootContentScan {
    scanned_item_count: usize,
    scanned_file_count: usize,
    script_file_count: usize,
    reached_item_limit: bool,
    reached_time_limit: bool,
}

fn scan_root_content(
    root_path: &Path,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
) -> RootContentScan {
    let mut scan = RootContentScan::default();
    let started = Instant::now();
    let timeout = (options.root_preflight.max_duration_millis > 0)
        .then(|| Duration::from_millis(options.root_preflight.max_duration_millis));
    let mut pending_directories = vec![root_path.to_path_buf()];

    while let Some(directory) = pending_directories.pop() {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        scan_entries(
            entries,
            options,
            extensions,
            started,
            timeout,
            &mut pending_directories,
            &mut scan,
        );
        if scan.reached_item_limit || scan.reached_time_limit {
            break;
        }
    }

    scan
}

fn scan_entries(
    entries: fs::ReadDir,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    started: Instant,
    timeout: Option<Duration>,
    pending_directories: &mut Vec<PathBuf>,
    scan: &mut RootContentScan,
) {
    for entry in entries {
        if scan.scanned_item_count >= options.root_preflight.max_scanned_items {
            scan.reached_item_limit = true;
            break;
        }
        if let Some(timeout) = timeout
            && started.elapsed() >= timeout
        {
            scan.reached_time_limit = true;
            break;
        }

        let Ok(entry) = entry else {
            continue;
        };
        scan.scanned_item_count += 1;
        let path = entry.path();
        if should_skip_preflight_path(&path, options) {
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
        if extensions.contains_path(&path) {
            scan.script_file_count += 1;
        }
    }
}

fn should_skip_preflight_path(path: &Path, options: &ScannerOptions) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    if options.skip_hidden && name.as_encoded_bytes().starts_with(b".") {
        return true;
    }
    options.skip_packages && is_package_path(path)
}

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

fn is_trash_path(root_path: &Path) -> bool {
    let normalized_root = normalize_path(root_path);
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
