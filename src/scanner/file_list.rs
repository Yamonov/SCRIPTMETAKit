use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use serde::{Deserialize, Serialize};

use crate::{
    RootId,
    catalog::{
        DirectoryState, DirectoryStateMap, FileEntryChange, FileEntryChangeKind, FileIdentity,
        FileListSnapshot, RootError, RootSnapshot, RootStatus, ScanChangeSummary,
    },
    core::{OperationCancellation, ScriptMetaItemRef, ScriptRuntimeKind},
    formats::{
        ScriptFileInfo, detect_script_file, detect_script_file_from_bytes, is_script_package_path,
        needs_script_header_probe, script_header_probe_byte_limit,
        scriptmeta_edit_capability_from_file_list_probe,
    },
    now_timestamp_millis,
    scanner::{ExtensionPolicy, ScannerOptions},
    watcher::normalize_path,
};

use super::{
    path_resolution::{
        PathKind, PathResolutionStatus, ResolvedPath, path_error_status, resolve_scannable_path,
    },
    root_preflight::{RootContentPreflightTracker, root_location_issue},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileSystemEntry {
    pub display_path: PathBuf,
    pub resolved_path: PathBuf,
    #[serde(default)]
    pub path_kind: PathKind,
    #[serde(default)]
    pub resolution_status: PathResolutionStatus,
    #[serde(default)]
    pub resolution_message: Option<String>,
    pub is_directory: bool,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub content_modified_at: Option<u64>,
    #[serde(default)]
    pub identity: Option<FileIdentity>,
    #[serde(default)]
    pub runtime_kind: Option<ScriptRuntimeKind>,
    #[serde(default)]
    pub shebang: Option<String>,
    #[serde(default)]
    pub has_scriptmeta: bool,
    #[serde(default)]
    pub has_scriptmeta_edit_password: bool,
    #[serde(default)]
    pub is_file_locked: bool,
    #[serde(default)]
    pub is_read_only: bool,
    #[serde(default)]
    pub can_edit_scriptmeta: bool,
    #[serde(default)]
    pub can_append_scriptmeta: bool,
    #[serde(default)]
    pub scriptmeta_edit_state: crate::core::ScriptMetaEditState,
    #[serde(default)]
    pub scriptmeta_item: Option<ScriptMetaItemRef>,
    pub children: Vec<FileSystemEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryScanOutput {
    pub root: RootSnapshot,
    pub children: Vec<FileSystemEntry>,
    pub directory_states: DirectoryStateMap,
    pub truncated: bool,
    pub change_summary: Option<ScanChangeSummary>,
}

pub fn scan_file_list_root(
    root_id: &RootId,
    root_path: &Path,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
) -> DirectoryScanOutput {
    scan_file_list_root_controlled(root_id, root_path, options, extensions, None)
}

pub(crate) fn scan_file_list_root_controlled(
    root_id: &RootId,
    root_path: &Path,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    cancellation: Option<&OperationCancellation>,
) -> DirectoryScanOutput {
    scan_file_list_root_controlled_with_probe(
        root_id,
        root_path,
        options,
        extensions,
        cancellation,
        true,
    )
}

fn scan_file_list_root_controlled_with_probe(
    root_id: &RootId,
    root_path: &Path,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    cancellation: Option<&OperationCancellation>,
    probe_script_headers: bool,
) -> DirectoryScanOutput {
    let mut root = RootSnapshot::new(root_id.clone(), root_path.to_path_buf());
    let started = Instant::now();
    let timeout = options
        .scan_timeout_per_root_millis
        .map(Duration::from_millis);

    match root_path.try_exists() {
        Ok(true) => {}
        Ok(false) => return empty_root_output(root, RootStatus::Missing, missing_root_error()),
        Err(error) => {
            let (status, root_error) = root_io_error(&error, "root_path_check_failed");
            return empty_root_output(root, status, root_error);
        }
    }
    if let Some((status, root_error)) = root_location_issue(root_path, options) {
        return empty_root_output(root, status, root_error);
    }

    let mut state = FileListWalkState {
        options,
        extensions,
        timeout,
        started,
        ancestor_directories: BTreeSet::new(),
        visited_nodes: 0,
        scanned_nodes: 0,
        directory_states: DirectoryStateMap::new(),
        directory_read_errors: BTreeMap::new(),
        truncated: false,
        timed_out: false,
        cancelled: false,
        root_error: None,
        cancellation,
        probe_script_headers,
        preflight_tracker: Some(RootContentPreflightTracker::new(options, extensions)),
        preflight_issue: None,
    };

    let root_resolution = resolve_scannable_path(
        root_path.to_path_buf(),
        root_path.to_path_buf(),
        options,
        Some(extensions),
    );
    if root_resolution.is_unfollowed_symlink() {
        root.status = RootStatus::Unreadable;
        root.error = Some(RootError {
            code: "symlink_following_disabled".to_string(),
            message: "root is a symlink and symlink following is disabled".to_string(),
        });
        return DirectoryScanOutput {
            root,
            children: Vec::new(),
            directory_states: state.directory_states,
            truncated: false,
            change_summary: None,
        };
    }
    if let Some((status, root_error)) = root_location_issue(&root_resolution.resolved_path, options)
    {
        return empty_root_output(root, status, root_error);
    }
    let root_metadata = match fs::metadata(&root_resolution.resolved_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            let (status, root_error) = root_io_error(&error, "unresolved_root");
            root.status = status;
            root.error = Some(root_error);
            return DirectoryScanOutput {
                root,
                children: Vec::new(),
                directory_states: state.directory_states,
                truncated: state.truncated,
                change_summary: None,
            };
        }
    };
    if !root_metadata.is_dir() {
        root.status = RootStatus::Missing;
        root.error = Some(RootError {
            code: "not_directory".to_string(),
            message: "root path does not resolve to a directory".to_string(),
        });
        return DirectoryScanOutput {
            root,
            children: Vec::new(),
            directory_states: state.directory_states,
            truncated: state.truncated,
            change_summary: None,
        };
    }

    let children = scan_directory(root_path, &root_resolution.resolved_path, 0, &mut state)
        .unwrap_or_default();
    root.item_count = state.visited_nodes;
    root.last_loaded_at = Some(now_timestamp_millis());
    root.is_dirty = false;
    if let Some((status, error)) = state.preflight_issue {
        root.status = status;
        root.error = Some(error);
    } else if let Some(error) = state.root_error {
        root.status = RootStatus::Unreadable;
        root.error = Some(error);
    } else {
        root.status = if state.cancelled {
            root.error = Some(cancelled_root_error());
            RootStatus::Cancelled
        } else if state.timed_out {
            RootStatus::TimedOut
        } else if state.truncated {
            root.error = Some(scan_limit_root_error());
            RootStatus::Overflowed
        } else {
            RootStatus::Ready
        };
    }

    DirectoryScanOutput {
        root,
        children,
        directory_states: state.directory_states,
        truncated: state.truncated,
        change_summary: None,
    }
}

pub(crate) fn scan_file_list_root_transactional_controlled(
    root_id: &RootId,
    root_path: &Path,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    previous_snapshot: Option<&FileListSnapshot>,
    cancellation: Option<&OperationCancellation>,
    probe_script_headers: bool,
) -> DirectoryScanOutput {
    let mut output = scan_file_list_root_controlled_with_probe(
        root_id,
        root_path,
        options,
        extensions,
        cancellation,
        probe_script_headers,
    );
    let Some(previous_snapshot) = previous_snapshot else {
        return output;
    };
    if matches!(output.root.status, RootStatus::Ready | RootStatus::Missing) {
        return output;
    }

    output.children = previous_snapshot.children.clone().unwrap_or_default();
    output.directory_states = previous_snapshot.directory_states.clone();
    output.truncated = previous_snapshot.truncated;
    output.root.item_count = count_entries(&output.children);
    output.root.is_dirty = true;
    output.change_summary = Some(ScanChangeSummary::default());
    output
}

pub fn scan_file_list_root_with_dirty_directories(
    root_id: &RootId,
    root_path: &Path,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    previous_snapshot: Option<&FileListSnapshot>,
    dirty_directories: &[PathBuf],
) -> DirectoryScanOutput {
    scan_file_list_root_with_dirty_directories_controlled(
        root_id,
        root_path,
        options,
        extensions,
        previous_snapshot,
        dirty_directories,
        None,
    )
}

pub(crate) fn scan_file_list_root_with_dirty_directories_controlled(
    root_id: &RootId,
    root_path: &Path,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    previous_snapshot: Option<&FileListSnapshot>,
    dirty_directories: &[PathBuf],
    cancellation: Option<&OperationCancellation>,
) -> DirectoryScanOutput {
    let Some(previous_snapshot) = previous_snapshot else {
        return scan_file_list_root_controlled(
            root_id,
            root_path,
            options,
            extensions,
            cancellation,
        );
    };
    let Some(previous_children) = previous_snapshot.children.as_ref() else {
        return scan_file_list_root_controlled(
            root_id,
            root_path,
            options,
            extensions,
            cancellation,
        );
    };

    let root_resolution = resolve_scannable_path(
        root_path.to_path_buf(),
        root_path.to_path_buf(),
        options,
        Some(extensions),
    );
    let normalized_root_path = normalize_path(&root_resolution.resolved_path);
    let dirty_directories = normalize_dirty_directories(dirty_directories, &normalized_root_path);
    if requires_alias_reconciliation(previous_children, root_path, &normalized_root_path)
        || dirty_directories.is_empty()
        || dirty_directories
            .iter()
            .any(|directory| directory == &normalized_root_path)
    {
        return scan_file_list_root_transactional_controlled(
            root_id,
            root_path,
            options,
            extensions,
            Some(previous_snapshot),
            cancellation,
            true,
        );
    }

    let mut children = previous_children.clone();
    let mut directory_states = previous_snapshot.directory_states.clone();
    let mut truncated = previous_snapshot.truncated;
    let mut timed_out = false;
    let mut cancelled = false;
    let mut root_error = None;
    let mut scanned_nodes = 0;

    if dirty_directories
        .iter()
        .any(|directory| !directory_exists_in_entries(&children, directory))
    {
        return scan_file_list_root_transactional_controlled(
            root_id,
            root_path,
            options,
            extensions,
            Some(previous_snapshot),
            cancellation,
            true,
        );
    }
    let contexts = directory_scan_contexts(&children, &dirty_directories, &normalized_root_path);
    for context in &contexts {
        let dirty_directory = &context.resolved_path;
        match fs::metadata(dirty_directory) {
            Ok(metadata) if metadata.is_dir() => {
                let output = scan_file_list_directory(
                    context,
                    options,
                    extensions,
                    cancellation,
                    scanned_nodes,
                );
                scanned_nodes = output.scanned_nodes;
                if output.completed {
                    replace_directory_children(
                        &mut children,
                        &context.display_path,
                        output.children,
                        output.directory_error,
                    );
                    remove_directory_states_under(&mut directory_states, dirty_directory);
                    directory_states.extend(output.directory_states);
                }
                truncated |= output.truncated;
                timed_out |= output.timed_out;
                cancelled |= output.cancelled;
                if root_error.is_none() {
                    root_error = output.root_error;
                }
            }
            Ok(_) => {
                remove_directory_entry(&mut children, &context.display_path);
                remove_directory_states_under(&mut directory_states, dirty_directory);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                remove_directory_entry(&mut children, &context.display_path);
                remove_directory_states_under(&mut directory_states, dirty_directory);
            }
            Err(error) => {
                let (_, error) = root_io_error(&error, "dirty_directory_metadata_failed");
                root_error.get_or_insert(error);
            }
        }
    }

    if !options.include_empty_directories {
        prune_empty_directories(&mut children);
    }
    sort_entries(&mut children);

    let mut root = previous_snapshot.root.clone();
    root.item_count = count_entries(&children);
    root.last_loaded_at = Some(now_timestamp_millis());
    root.is_dirty = false;
    if let Some(error) = root_error {
        root.status = RootStatus::Unreadable;
        root.error = Some(error);
    } else if cancelled {
        root.status = RootStatus::Cancelled;
        root.error = Some(cancelled_root_error());
    } else {
        root.status = if timed_out {
            RootStatus::TimedOut
        } else if truncated {
            root.error = Some(scan_limit_root_error());
            RootStatus::Overflowed
        } else {
            RootStatus::Ready
        };
        if root.status == RootStatus::Ready {
            root.error = None;
        }
    }
    if root.status != RootStatus::Ready {
        children = previous_children.clone();
        directory_states = previous_snapshot.directory_states.clone();
        truncated = previous_snapshot.truncated;
        root.item_count = count_entries(&children);
        root.is_dirty = true;
    }

    DirectoryScanOutput {
        root,
        children,
        directory_states,
        truncated,
        change_summary: None,
    }
}

pub(crate) fn try_scan_file_list_root_with_owned_dirty_directories_controlled(
    root_path: &Path,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    mut previous_snapshot: FileListSnapshot,
    dirty_directories: &[PathBuf],
    cancellation: Option<&OperationCancellation>,
) -> Result<DirectoryScanOutput, Box<FileListSnapshot>> {
    let Some(previous_children) = previous_snapshot.children.as_ref() else {
        return Err(Box::new(previous_snapshot));
    };

    let root_resolution = resolve_scannable_path(
        root_path.to_path_buf(),
        root_path.to_path_buf(),
        options,
        Some(extensions),
    );
    let normalized_root_path = normalize_path(&root_resolution.resolved_path);
    let dirty_directories = normalize_dirty_directories(dirty_directories, &normalized_root_path);
    if requires_alias_reconciliation(previous_children, root_path, &normalized_root_path)
        || dirty_directories.is_empty()
        || dirty_directories
            .iter()
            .any(|directory| directory == &normalized_root_path)
    {
        return Err(Box::new(previous_snapshot));
    }
    if dirty_directories
        .iter()
        .any(|directory| !directory_exists_in_entries(previous_children, directory))
    {
        return Err(Box::new(previous_snapshot));
    }

    let previous_dirty_entries =
        clone_dirty_directory_entries(previous_children, &dirty_directories);
    let fallback_children = previous_children.clone();
    let fallback_directory_states = previous_snapshot.directory_states.clone();
    let fallback_truncated = previous_snapshot.truncated;
    let mut children = previous_snapshot.children.take().unwrap_or_default();
    let mut directory_states = previous_snapshot.directory_states;
    let mut truncated = previous_snapshot.truncated;
    let mut timed_out = false;
    let mut cancelled = false;
    let mut root_error = None;
    let mut scanned_nodes = 0;

    let contexts = directory_scan_contexts(&children, &dirty_directories, &normalized_root_path);
    for context in &contexts {
        let dirty_directory = &context.resolved_path;
        match fs::metadata(dirty_directory) {
            Ok(metadata) if metadata.is_dir() => {
                let output = scan_file_list_directory(
                    context,
                    options,
                    extensions,
                    cancellation,
                    scanned_nodes,
                );
                scanned_nodes = output.scanned_nodes;
                if output.completed {
                    replace_directory_children(
                        &mut children,
                        &context.display_path,
                        output.children,
                        output.directory_error,
                    );
                    remove_directory_states_under(&mut directory_states, dirty_directory);
                    directory_states.extend(output.directory_states);
                }
                truncated |= output.truncated;
                timed_out |= output.timed_out;
                cancelled |= output.cancelled;
                if root_error.is_none() {
                    root_error = output.root_error;
                }
            }
            Ok(_) => {
                remove_directory_entry(&mut children, &context.display_path);
                remove_directory_states_under(&mut directory_states, dirty_directory);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                remove_directory_entry(&mut children, &context.display_path);
                remove_directory_states_under(&mut directory_states, dirty_directory);
            }
            Err(error) => {
                let (_, error) = root_io_error(&error, "dirty_directory_metadata_failed");
                root_error.get_or_insert(error);
            }
        }
    }

    if !options.include_empty_directories {
        prune_empty_directories(&mut children);
    }
    sort_entries(&mut children);

    let mut root = previous_snapshot.root;
    root.last_loaded_at = Some(now_timestamp_millis());
    if let Some(error) = root_error {
        root.status = RootStatus::Unreadable;
        root.error = Some(error);
    } else if cancelled {
        root.status = RootStatus::Cancelled;
        root.error = Some(cancelled_root_error());
    } else {
        root.status = if timed_out {
            RootStatus::TimedOut
        } else if truncated {
            root.error = Some(scan_limit_root_error());
            RootStatus::Overflowed
        } else {
            RootStatus::Ready
        };
        if root.status == RootStatus::Ready {
            root.error = None;
        }
    }

    let change_summary = if root.status == RootStatus::Ready {
        root.is_dirty = false;
        let current_dirty_entries = clone_dirty_directory_entries(&children, &dirty_directories);
        diff_file_entry_sets(
            &root.root_id,
            &previous_dirty_entries,
            &current_dirty_entries,
        )
    } else {
        root.is_dirty = true;
        children = fallback_children;
        directory_states = fallback_directory_states;
        truncated = fallback_truncated;
        ScanChangeSummary::default()
    };
    root.item_count = count_entries(&children);

    Ok(DirectoryScanOutput {
        root,
        children,
        directory_states,
        truncated,
        change_summary: Some(change_summary),
    })
}

#[derive(Debug)]
struct PartialDirectoryScanOutput {
    children: Vec<FileSystemEntry>,
    directory_states: DirectoryStateMap,
    directory_error: Option<(PathResolutionStatus, String)>,
    truncated: bool,
    timed_out: bool,
    cancelled: bool,
    root_error: Option<RootError>,
    scanned_nodes: usize,
    completed: bool,
}

struct DirectoryScanContext {
    display_path: PathBuf,
    resolved_path: PathBuf,
    ancestor_directories: BTreeSet<PathBuf>,
    depth: usize,
}

fn scan_file_list_directory(
    context: &DirectoryScanContext,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    cancellation: Option<&OperationCancellation>,
    scanned_nodes: usize,
) -> PartialDirectoryScanOutput {
    let mut state = FileListWalkState {
        options,
        extensions,
        timeout: options
            .scan_timeout_per_root_millis
            .map(Duration::from_millis),
        started: Instant::now(),
        ancestor_directories: context.ancestor_directories.clone(),
        visited_nodes: 0,
        scanned_nodes,
        directory_states: DirectoryStateMap::new(),
        directory_read_errors: BTreeMap::new(),
        truncated: false,
        timed_out: false,
        cancelled: false,
        root_error: None,
        cancellation,
        probe_script_headers: true,
        preflight_tracker: None,
        preflight_issue: None,
    };
    let resolved_directory = &context.resolved_path;
    let children = scan_directory(
        &context.display_path,
        resolved_directory,
        context.depth,
        &mut state,
    );
    let directory_error = state.directory_read_errors.get(resolved_directory).cloned();
    PartialDirectoryScanOutput {
        completed: children.is_some(),
        children: children.unwrap_or_default(),
        directory_states: state.directory_states,
        directory_error,
        truncated: state.truncated,
        timed_out: state.timed_out,
        cancelled: state.cancelled,
        root_error: state.root_error,
        scanned_nodes: state.scanned_nodes,
    }
}

struct FileListWalkState<'a> {
    options: &'a ScannerOptions,
    extensions: &'a ExtensionPolicy,
    timeout: Option<Duration>,
    started: Instant,
    ancestor_directories: BTreeSet<PathBuf>,
    visited_nodes: usize,
    scanned_nodes: usize,
    directory_states: DirectoryStateMap,
    directory_read_errors: BTreeMap<PathBuf, (PathResolutionStatus, String)>,
    truncated: bool,
    timed_out: bool,
    cancelled: bool,
    root_error: Option<RootError>,
    cancellation: Option<&'a OperationCancellation>,
    probe_script_headers: bool,
    preflight_tracker: Option<RootContentPreflightTracker<'a>>,
    preflight_issue: Option<(RootStatus, RootError)>,
}

fn scan_directory(
    display_directory: &Path,
    source_directory: &Path,
    depth: usize,
    state: &mut FileListWalkState<'_>,
) -> Option<Vec<FileSystemEntry>> {
    if should_stop(depth, state) {
        return None;
    }

    let resolved_directory = normalize_path(source_directory);
    if !state
        .ancestor_directories
        .insert(resolved_directory.clone())
    {
        return Some(Vec::new());
    }

    // Sibling aliases may visit the same directory. Only a directory already
    // on this branch is a cycle; unwind on every normal/early-return path.
    let children = scan_directory_contents(
        display_directory,
        source_directory,
        &resolved_directory,
        depth,
        state,
    );
    state.ancestor_directories.remove(&resolved_directory);
    children
}

fn scan_directory_contents(
    display_directory: &Path,
    source_directory: &Path,
    resolved_directory: &Path,
    depth: usize,
    state: &mut FileListWalkState<'_>,
) -> Option<Vec<FileSystemEntry>> {
    let entries = match fs::read_dir(source_directory) {
        Ok(entries) => entries,
        Err(error) => {
            state.directory_read_errors.insert(
                resolved_directory.to_path_buf(),
                (path_error_status(&error), error.to_string()),
            );
            let (_, root_error) = root_io_error(&error, "read_directory_failed");
            state.root_error.get_or_insert(root_error);
            return Some(Vec::new());
        }
    };

    let mut children = Vec::new();
    for entry_result in entries {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(error) => {
                let (_, root_error) = root_io_error(&error, "read_directory_entry_failed");
                state.root_error.get_or_insert(root_error);
                continue;
            }
        };
        if should_stop(depth, state) {
            break;
        }
        state.scanned_nodes = state.scanned_nodes.saturating_add(1);

        let source_path = entry.path();
        let display_path = display_directory.join(entry.file_name());
        if let Some(issue) = state
            .preflight_tracker
            .as_mut()
            .and_then(|tracker| tracker.observe_entry(&entry, &display_path))
        {
            state.preflight_issue = Some(issue);
            break;
        }
        if should_skip_path(&display_path, state.options) {
            continue;
        }

        let resolved = resolve_scannable_path(
            display_path,
            source_path,
            state.options,
            Some(state.extensions),
        );
        if resolved.is_unfollowed_symlink() {
            children.push(resolution_error_entry(resolved));
            state.visited_nodes += 1;
            continue;
        }
        if should_include_resolution_error(&resolved) {
            children.push(resolution_error_entry(resolved));
            state.visited_nodes += 1;
            continue;
        }
        if state.options.skip_packages && is_package_path(&resolved.resolved_path) {
            continue;
        }

        let metadata = match fs::metadata(&resolved.resolved_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                let status = path_error_status(&error);
                if should_include_resolution_error(&resolved)
                    || should_include_metadata_error(&resolved, status, state.extensions)
                {
                    children.push(resolution_error_entry(
                        resolved.with_status(status, Some(error.to_string())),
                    ));
                    state.visited_nodes += 1;
                }
                continue;
            }
        };

        if is_script_package_path(&resolved.display_path)
            || is_script_package_path(&resolved.resolved_path)
        {
            let identity = file_identity(&resolved.resolved_path, &metadata);
            children.push(script_package_entry(resolved, &metadata, identity));
            state.visited_nodes += 1;
            continue;
        }

        if metadata.is_dir() {
            let resolved_directory = normalize_path(&resolved.resolved_path);
            let is_cycle = state.ancestor_directories.contains(&resolved_directory);
            let (resolved, nested_children) = if is_cycle {
                (
                    resolved.with_status(
                        PathResolutionStatus::Cycle,
                        Some("resolved path is an ancestor directory".to_string()),
                    ),
                    Vec::new(),
                )
            } else {
                let nested_children = scan_directory(
                    &resolved.display_path,
                    &resolved.resolved_path,
                    depth + 1,
                    state,
                )
                .unwrap_or_default();
                let resolved_directory = normalize_path(&resolved.resolved_path);
                if let Some((status, message)) =
                    state.directory_read_errors.get(&resolved_directory)
                {
                    let resolved = resolved.with_status(*status, Some(message.clone()));
                    (resolved, nested_children)
                } else {
                    (resolved, nested_children)
                }
            };
            if state.options.include_empty_directories
                || !nested_children.is_empty()
                || resolved.resolution_status == PathResolutionStatus::Cycle
                || resolved.resolution_status == PathResolutionStatus::PermissionDenied
                || resolved.resolution_status == PathResolutionStatus::Broken
            {
                let identity = file_identity(&resolved.resolved_path, &metadata);
                children.push(FileSystemEntry {
                    display_path: resolved.display_path,
                    resolved_path: resolved.resolved_path,
                    path_kind: resolved.path_kind,
                    resolution_status: resolved.resolution_status,
                    resolution_message: resolved.resolution_message,
                    is_directory: true,
                    file_size: None,
                    content_modified_at: None,
                    identity,
                    runtime_kind: None,
                    shebang: None,
                    has_scriptmeta: false,
                    has_scriptmeta_edit_password: false,
                    is_file_locked: false,
                    is_read_only: false,
                    can_edit_scriptmeta: false,
                    can_append_scriptmeta: false,
                    scriptmeta_edit_state: crate::core::ScriptMetaEditState::Unsupported,
                    scriptmeta_item: None,
                    children: nested_children,
                });
                state.visited_nodes += 1;
            }
            continue;
        }

        if metadata.is_file() && state.extensions.contains_path(&resolved.resolved_path) {
            let file_info = if state.probe_script_headers {
                detect_script_info_for_file_list(&resolved.resolved_path)
            } else {
                FileListScriptInfo {
                    script: detect_script_file(&resolved.resolved_path, None),
                    capability: scriptmeta_edit_capability_from_file_list_probe(
                        &resolved.resolved_path,
                        None,
                    ),
                }
            };
            let identity = file_identity(&resolved.resolved_path, &metadata);
            children.push(FileSystemEntry {
                display_path: resolved.display_path,
                resolved_path: resolved.resolved_path,
                path_kind: resolved.path_kind,
                resolution_status: resolved.resolution_status,
                resolution_message: resolved.resolution_message,
                is_directory: false,
                file_size: Some(metadata.len()),
                content_modified_at: metadata.modified().ok().and_then(system_time_millis),
                identity,
                runtime_kind: file_info.script.runtime_kind,
                shebang: file_info.script.shebang,
                has_scriptmeta: file_info.capability.has_scriptmeta,
                has_scriptmeta_edit_password: file_info.capability.has_scriptmeta_edit_password,
                is_file_locked: file_info.capability.is_file_locked,
                is_read_only: file_info.capability.is_read_only,
                can_edit_scriptmeta: file_info.capability.can_edit_scriptmeta,
                can_append_scriptmeta: file_info.capability.can_append_scriptmeta,
                scriptmeta_edit_state: file_info.capability.scriptmeta_edit_state,
                scriptmeta_item: None,
                children: Vec::new(),
            });
            state.visited_nodes += 1;
        }
    }

    children.sort_by(compare_file_list_entries);

    let fingerprint = child_fingerprint(&children);
    state.directory_states.insert(
        resolved_directory.to_string_lossy().into_owned(),
        DirectoryState {
            modification_time_millis: modification_time_millis(source_directory),
            child_count: children.len(),
            child_fingerprint: fingerprint,
            identity: fs::metadata(source_directory)
                .ok()
                .and_then(|metadata| file_identity(source_directory, &metadata)),
        },
    );

    Some(children)
}

fn should_stop(depth: usize, state: &mut FileListWalkState<'_>) -> bool {
    if state.preflight_issue.is_some() {
        return true;
    }
    if state
        .cancellation
        .is_some_and(OperationCancellation::is_cancelled)
    {
        state.cancelled = true;
        return true;
    }

    if depth > state.options.max_depth || state.scanned_nodes >= state.options.max_nodes_per_root {
        state.truncated = true;
        return true;
    }

    if let Some(timeout) = state.timeout
        && state.started.elapsed() >= timeout
    {
        state.timed_out = true;
        return true;
    }

    false
}

fn should_skip_path(path: &Path, options: &ScannerOptions) -> bool {
    if options.skip_hidden && is_hidden_path(path) {
        return true;
    }

    options.skip_packages && is_package_path(path)
}

fn is_hidden_path(path: &Path) -> bool {
    display_name(path).starts_with('.') || platform_path_is_hidden(path)
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

fn should_include_resolution_error(resolved: &ResolvedPath) -> bool {
    resolved.path_kind != PathKind::Normal
        && resolved.resolution_status != PathResolutionStatus::NotRequested
        && resolved.resolution_status != PathResolutionStatus::Resolved
}

fn should_include_metadata_error(
    resolved: &ResolvedPath,
    status: PathResolutionStatus,
    extensions: &ExtensionPolicy,
) -> bool {
    status == PathResolutionStatus::PermissionDenied
        && extensions.contains_path(&resolved.resolved_path)
}

fn resolution_error_entry(resolved: ResolvedPath) -> FileSystemEntry {
    FileSystemEntry {
        display_path: resolved.display_path,
        resolved_path: resolved.resolved_path,
        path_kind: resolved.path_kind,
        resolution_status: resolved.resolution_status,
        resolution_message: resolved.resolution_message,
        is_directory: false,
        file_size: None,
        content_modified_at: None,
        identity: None,
        runtime_kind: None,
        shebang: None,
        has_scriptmeta: false,
        has_scriptmeta_edit_password: false,
        is_file_locked: false,
        is_read_only: false,
        can_edit_scriptmeta: false,
        can_append_scriptmeta: false,
        scriptmeta_edit_state: crate::core::ScriptMetaEditState::Unknown,
        scriptmeta_item: None,
        children: Vec::new(),
    }
}

fn script_package_entry(
    resolved: ResolvedPath,
    metadata: &fs::Metadata,
    identity: Option<FileIdentity>,
) -> FileSystemEntry {
    FileSystemEntry {
        display_path: resolved.display_path,
        resolved_path: resolved.resolved_path,
        path_kind: resolved.path_kind,
        resolution_status: resolved.resolution_status,
        resolution_message: resolved.resolution_message,
        is_directory: false,
        file_size: None,
        content_modified_at: metadata.modified().ok().and_then(system_time_millis),
        identity,
        runtime_kind: Some(ScriptRuntimeKind::AppleScript),
        shebang: None,
        has_scriptmeta: false,
        has_scriptmeta_edit_password: false,
        is_file_locked: false,
        is_read_only: false,
        can_edit_scriptmeta: false,
        can_append_scriptmeta: false,
        scriptmeta_edit_state: crate::core::ScriptMetaEditState::Unsupported,
        scriptmeta_item: None,
        children: Vec::new(),
    }
}

fn empty_root_output(
    mut root: RootSnapshot,
    status: RootStatus,
    error: RootError,
) -> DirectoryScanOutput {
    root.status = status;
    root.error = Some(error);
    DirectoryScanOutput {
        root,
        children: Vec::new(),
        directory_states: DirectoryStateMap::new(),
        truncated: false,
        change_summary: None,
    }
}

fn missing_root_error() -> RootError {
    RootError {
        code: "missing".to_string(),
        message: "root path does not exist".to_string(),
    }
}

fn cancelled_root_error() -> RootError {
    RootError {
        code: "operation_cancelled".to_string(),
        message: "operation was cancelled".to_string(),
    }
}

fn scan_limit_root_error() -> RootError {
    RootError {
        code: "scan_limit_exceeded".to_string(),
        message: "file list scan reached a configured depth or node limit".to_string(),
    }
}

fn root_io_error(error: &io::Error, fallback_code: &'static str) -> (RootStatus, RootError) {
    let (status, code) = match error.kind() {
        io::ErrorKind::NotFound => (RootStatus::Missing, "missing"),
        io::ErrorKind::PermissionDenied => (RootStatus::Unreadable, "permission_denied"),
        _ => (RootStatus::Unreadable, fallback_code),
    };
    (
        status,
        RootError {
            code: code.to_string(),
            message: error.to_string(),
        },
    )
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

fn normalize_dirty_directories(directories: &[PathBuf], root_path: &Path) -> Vec<PathBuf> {
    let mut normalized = directories
        .iter()
        .map(|directory| normalize_path(directory))
        .filter(|directory| path_is_same_or_child(directory, root_path))
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    prune_redundant_dirty_directories(normalized)
}

fn prune_redundant_dirty_directories(directories: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut pruned: Vec<PathBuf> = Vec::with_capacity(directories.len());
    for directory in directories {
        if pruned
            .iter()
            .any(|parent| path_is_same_or_child(&directory, parent))
        {
            continue;
        }
        pruned.push(directory);
    }
    pruned
}

fn requires_alias_reconciliation(
    entries: &[FileSystemEntry],
    display_root: &Path,
    resolved_root: &Path,
) -> bool {
    fn visit(
        entries: &[FileSystemEntry],
        display_parent: &Path,
        ancestors: &mut BTreeSet<PathBuf>,
    ) -> bool {
        for entry in entries {
            if entry.display_path.parent() != Some(display_parent) {
                return true;
            }
            if !entry.is_directory {
                continue;
            }
            let is_ancestor = ancestors.contains(&entry.resolved_path);
            if entry.resolution_status == PathResolutionStatus::Cycle {
                if !is_ancestor || !entry.children.is_empty() {
                    return true;
                }
                continue;
            }
            if is_ancestor {
                return true;
            }
            ancestors.insert(entry.resolved_path.clone());
            let invalid = visit(&entry.children, &entry.display_path, ancestors);
            ancestors.remove(&entry.resolved_path);
            if invalid {
                return true;
            }
        }
        false
    }
    // Older cached trees may contain false Cycle markers or physical child
    // paths under a logical alias. Reconcile those once with the full scanner.
    visit(
        entries,
        display_root,
        &mut BTreeSet::from([resolved_root.to_path_buf()]),
    )
}

fn directory_scan_contexts(
    entries: &[FileSystemEntry],
    dirty_directories: &[PathBuf],
    resolved_root: &Path,
) -> Vec<DirectoryScanContext> {
    fn collect(
        entries: &[FileSystemEntry],
        dirty_directories: &[PathBuf],
        ancestors: &mut BTreeSet<PathBuf>,
        depth: usize,
        contexts: &mut Vec<DirectoryScanContext>,
    ) {
        for entry in entries {
            if !entry.is_directory || ancestors.contains(&entry.resolved_path) {
                continue;
            }
            if dirty_directories
                .iter()
                .any(|dirty| path_is_same_or_child(&entry.resolved_path, dirty))
            {
                contexts.push(DirectoryScanContext {
                    display_path: entry.display_path.clone(),
                    resolved_path: entry.resolved_path.clone(),
                    ancestor_directories: ancestors.clone(),
                    depth,
                });
                // This logical subtree is covered, but sibling aliases still
                // require their own scan with their own display path/ancestors.
                continue;
            }
            ancestors.insert(entry.resolved_path.clone());
            collect(
                &entry.children,
                dirty_directories,
                ancestors,
                depth + 1,
                contexts,
            );
            ancestors.remove(&entry.resolved_path);
        }
    }
    let mut contexts = Vec::new();
    collect(
        entries,
        dirty_directories,
        &mut BTreeSet::from([resolved_root.to_path_buf()]),
        1,
        &mut contexts,
    );
    contexts
}

fn directory_exists_in_entries(entries: &[FileSystemEntry], directory: &Path) -> bool {
    entries.iter().any(|entry| {
        entry.is_directory
            && (entry.resolved_path == directory
                || directory_exists_in_entries(&entry.children, directory))
    })
}

fn clone_dirty_directory_entries(
    entries: &[FileSystemEntry],
    dirty_directories: &[PathBuf],
) -> Vec<FileSystemEntry> {
    let mut selected = Vec::new();
    for entry in entries {
        if entry.is_directory
            && dirty_directories
                .iter()
                .any(|directory| path_is_same_or_child(&entry.resolved_path, directory))
        {
            selected.push(entry.clone());
        } else {
            selected.extend(clone_dirty_directory_entries(
                &entry.children,
                dirty_directories,
            ));
        }
    }
    selected
}

fn replace_directory_children(
    entries: &mut [FileSystemEntry],
    directory: &Path,
    children: Vec<FileSystemEntry>,
    directory_error: Option<(PathResolutionStatus, String)>,
) -> bool {
    let mut children = Some(children);
    replace_directory_children_inner(entries, directory, &mut children, directory_error.as_ref())
}

fn replace_directory_children_inner(
    entries: &mut [FileSystemEntry],
    directory: &Path,
    children: &mut Option<Vec<FileSystemEntry>>,
    directory_error: Option<&(PathResolutionStatus, String)>,
) -> bool {
    for entry in entries {
        if entry.is_directory && entry.display_path == directory {
            entry.children = children.take().unwrap_or_default();
            if let Some((status, message)) = directory_error {
                entry.resolution_status = *status;
                entry.resolution_message = Some(message.clone());
            } else if matches!(
                entry.resolution_status,
                PathResolutionStatus::PermissionDenied
                    | PathResolutionStatus::Broken
                    | PathResolutionStatus::Cycle
            ) {
                entry.resolution_status = PathResolutionStatus::Resolved;
                entry.resolution_message = None;
            }
            return true;
        }
        if replace_directory_children_inner(
            &mut entry.children,
            directory,
            children,
            directory_error,
        ) {
            return true;
        }
    }
    false
}

fn remove_directory_entry(entries: &mut Vec<FileSystemEntry>, directory: &Path) -> bool {
    let original_len = entries.len();
    entries.retain(|entry| !(entry.is_directory && entry.display_path == directory));
    if entries.len() != original_len {
        return true;
    }
    entries
        .iter_mut()
        .any(|entry| remove_directory_entry(&mut entry.children, directory))
}

fn prune_empty_directories(entries: &mut Vec<FileSystemEntry>) {
    for entry in entries.iter_mut() {
        prune_empty_directories(&mut entry.children);
    }
    entries.retain(|entry| {
        !entry.is_directory
            || !entry.children.is_empty()
            || entry.resolution_status == PathResolutionStatus::Cycle
            || entry.resolution_status == PathResolutionStatus::PermissionDenied
            || entry.resolution_status == PathResolutionStatus::Broken
    });
}

fn sort_entries(entries: &mut [FileSystemEntry]) {
    for entry in entries.iter_mut() {
        sort_entries(&mut entry.children);
    }
    entries.sort_by(compare_file_list_entries);
}

fn remove_directory_states_under(states: &mut DirectoryStateMap, directory: &Path) {
    states.retain(|key, _| !path_is_same_or_child(Path::new(key), directory));
}

fn count_entries(entries: &[FileSystemEntry]) -> usize {
    entries
        .iter()
        .map(|entry| 1usize.saturating_add(count_entries(&entry.children)))
        .sum()
}

fn diff_file_entry_sets(
    root_id: &RootId,
    previous_entries: &[FileSystemEntry],
    current_entries: &[FileSystemEntry],
) -> ScanChangeSummary {
    let mut previous_map = BTreeMap::new();
    let mut current_map = BTreeMap::new();
    collect_file_entries(previous_entries, &mut previous_map);
    collect_file_entries(current_entries, &mut current_map);

    let mut summary = ScanChangeSummary::default();
    for (path, current_entry) in &current_map {
        match previous_map.get(path) {
            Some(previous_entry) if file_entry_changed(previous_entry, current_entry) => {
                summary.modified_count += 1;
                summary.changes.push(FileEntryChange::from_entry(
                    root_id.clone(),
                    FileEntryChangeKind::Modified,
                    current_entry,
                ));
            }
            Some(_) => {}
            None => {
                summary.added_count += 1;
                summary.changes.push(FileEntryChange::from_entry(
                    root_id.clone(),
                    FileEntryChangeKind::Added,
                    current_entry,
                ));
            }
        }
    }

    for (path, previous_entry) in &previous_map {
        if current_map.contains_key(path) {
            continue;
        }
        summary.removed_count += 1;
        summary.changes.push(FileEntryChange::from_entry(
            root_id.clone(),
            FileEntryChangeKind::Removed,
            previous_entry,
        ));
    }
    summary
        .changes
        .sort_by(|lhs, rhs| lhs.resolved_path.cmp(&rhs.resolved_path));
    summary
}

fn collect_file_entries<'a>(
    entries: &'a [FileSystemEntry],
    output: &mut BTreeMap<&'a Path, &'a FileSystemEntry>,
) {
    for entry in entries {
        output.insert(entry.resolved_path.as_path(), entry);
        collect_file_entries(&entry.children, output);
    }
}

fn file_entry_changed(lhs: &FileSystemEntry, rhs: &FileSystemEntry) -> bool {
    lhs.is_directory != rhs.is_directory
        || lhs.path_kind != rhs.path_kind
        || lhs.resolution_status != rhs.resolution_status
        || lhs.resolution_message.as_deref() != rhs.resolution_message.as_deref()
        || lhs.file_size != rhs.file_size
        || lhs.content_modified_at != rhs.content_modified_at
        || lhs.identity != rhs.identity
        || lhs.runtime_kind != rhs.runtime_kind
        || lhs.shebang.as_deref() != rhs.shebang.as_deref()
        || lhs.has_scriptmeta != rhs.has_scriptmeta
        || lhs.has_scriptmeta_edit_password != rhs.has_scriptmeta_edit_password
        || lhs.is_file_locked != rhs.is_file_locked
        || lhs.is_read_only != rhs.is_read_only
        || lhs.can_edit_scriptmeta != rhs.can_edit_scriptmeta
        || lhs.can_append_scriptmeta != rhs.can_append_scriptmeta
        || lhs.scriptmeta_edit_state != rhs.scriptmeta_edit_state
}

fn path_is_same_or_child(path: &Path, parent: &Path) -> bool {
    path == parent || path.starts_with(parent)
}

fn display_name(path: &Path) -> Cow<'_, str> {
    path.file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or(Cow::Borrowed(""))
}

fn compare_file_list_entries(lhs: &FileSystemEntry, rhs: &FileSystemEntry) -> Ordering {
    rhs.is_directory.cmp(&lhs.is_directory).then_with(|| {
        compare_display_names(
            &display_name(&lhs.display_path),
            &display_name(&rhs.display_path),
        )
    })
}

fn compare_display_names(lhs: &str, rhs: &str) -> Ordering {
    let ordering = platform_display_name_compare(lhs, rhs);
    if ordering == Ordering::Equal {
        lhs.cmp(rhs)
    } else {
        ordering
    }
}

#[cfg(target_os = "macos")]
fn platform_display_name_compare(lhs: &str, rhs: &str) -> Ordering {
    use objc2_foundation::NSString;

    let lhs = NSString::from_str(lhs);
    let rhs = NSString::from_str(rhs);
    lhs.localizedStandardCompare(&rhs).into()
}

#[cfg(not(target_os = "macos"))]
fn platform_display_name_compare(lhs: &str, rhs: &str) -> Ordering {
    portable_display_name_compare(lhs, rhs)
}

#[cfg(not(target_os = "macos"))]
fn portable_display_name_compare(lhs: &str, rhs: &str) -> Ordering {
    let mut lhs_chars = lhs.chars().peekable();
    let mut rhs_chars = rhs.chars().peekable();

    loop {
        match (lhs_chars.peek().copied(), rhs_chars.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(lhs_char), Some(rhs_char))
                if lhs_char.is_ascii_digit() && rhs_char.is_ascii_digit() =>
            {
                let lhs_number = take_ascii_number(&mut lhs_chars);
                let rhs_number = take_ascii_number(&mut rhs_chars);
                let ordering = compare_ascii_numbers(&lhs_number, &rhs_number);
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(lhs_char), Some(rhs_char)) => {
                lhs_chars.next();
                rhs_chars.next();
                let ordering = lhs_char
                    .to_lowercase()
                    .cmp(rhs_char.to_lowercase())
                    .then_with(|| lhs_char.cmp(&rhs_char));
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn take_ascii_number(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut value = String::new();
    while let Some(char) = chars.peek().copied() {
        if !char.is_ascii_digit() {
            break;
        }
        value.push(char);
        chars.next();
    }
    value
}

#[cfg(not(target_os = "macos"))]
fn compare_ascii_numbers(lhs: &str, rhs: &str) -> Ordering {
    let lhs_trimmed = lhs.trim_start_matches('0');
    let rhs_trimmed = rhs.trim_start_matches('0');
    let lhs_significant = if lhs_trimmed.is_empty() {
        "0"
    } else {
        lhs_trimmed
    };
    let rhs_significant = if rhs_trimmed.is_empty() {
        "0"
    } else {
        rhs_trimmed
    };

    lhs_significant
        .len()
        .cmp(&rhs_significant.len())
        .then_with(|| lhs_significant.cmp(rhs_significant))
        .then_with(|| lhs.len().cmp(&rhs.len()))
}

struct FileListScriptInfo {
    script: ScriptFileInfo,
    capability: crate::core::ScriptMetaEditCapability,
}

fn detect_script_info_for_file_list(path: &Path) -> FileListScriptInfo {
    if !needs_script_header_probe(path) {
        return FileListScriptInfo {
            script: detect_script_file(path, None),
            capability: scriptmeta_edit_capability_from_file_list_probe(path, None),
        };
    }

    let Ok(file) = File::open(path) else {
        return FileListScriptInfo {
            script: detect_script_file(path, None),
            capability: scriptmeta_edit_capability_from_file_list_probe(path, None),
        };
    };

    let mut buffer = Vec::with_capacity(script_header_probe_byte_limit(path));
    if file
        .take(script_header_probe_byte_limit(path) as u64)
        .read_to_end(&mut buffer)
        .is_err()
    {
        buffer.clear();
    }
    let prefix_bytes = buffer.as_slice();
    FileListScriptInfo {
        script: detect_script_file_from_bytes(path, Some(prefix_bytes)),
        capability: scriptmeta_edit_capability_from_file_list_probe(path, Some(prefix_bytes)),
    }
}

fn child_fingerprint(children: &[FileSystemEntry]) -> u64 {
    let mut hash = 14_695_981_039_346_656_037u64;
    for child in children {
        for byte in display_name(&child.display_path).as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(1_099_511_628_211);
        }
        hash ^= u64::from(child.is_directory);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
}

fn modification_time_millis(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_millis)
}

pub(crate) fn file_identity(path: &Path, metadata: &fs::Metadata) -> Option<FileIdentity> {
    let content_modified_at = metadata.modified().ok().and_then(system_time_millis);
    let (volume_id, file_id) = platform_file_identity(metadata);
    let stable_id = match (&volume_id, &file_id) {
        (Some(volume_id), Some(file_id)) => format!("{volume_id}:{file_id}"),
        _ => format!(
            "path:{}:{}:{}",
            normalize_path(path).to_string_lossy(),
            metadata.len(),
            content_modified_at.unwrap_or_default()
        ),
    };
    Some(FileIdentity {
        stable_id,
        volume_id,
        file_id,
        file_size: Some(metadata.len()),
        content_modified_at,
    })
}

#[cfg(unix)]
fn platform_file_identity(metadata: &fs::Metadata) -> (Option<String>, Option<String>) {
    use std::os::unix::fs::MetadataExt;

    (
        Some(metadata.dev().to_string()),
        Some(metadata.ino().to_string()),
    )
}

#[cfg(not(unix))]
fn platform_file_identity(_metadata: &fs::Metadata) -> (Option<String>, Option<String>) {
    (None, None)
}

pub(crate) fn system_time_millis(time: SystemTime) -> Option<u64> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn entry(name: &str, is_directory: bool) -> FileSystemEntry {
        FileSystemEntry {
            display_path: PathBuf::from(name),
            resolved_path: PathBuf::from(name),
            path_kind: PathKind::Normal,
            resolution_status: PathResolutionStatus::Resolved,
            resolution_message: None,
            is_directory,
            file_size: None,
            content_modified_at: None,
            identity: None,
            runtime_kind: None,
            shebang: None,
            has_scriptmeta: false,
            has_scriptmeta_edit_password: false,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: false,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: crate::core::ScriptMetaEditState::Unknown,
            scriptmeta_item: None,
            children: Vec::new(),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn display_name_order_matches_macos_localized_standard_compare() {
        let mut names = [
            "ActionHelperScripts",
            "Import-Export-Guides-Photoshop",
            "KM",
            "Photoshop",
            "Scripts",
            "_GIT",
            "test",
            "Illustrator に移動",
            "InDesignで表示する",
            "StarLayerToggle",
            "opacity_Enhancer",
            "ゴミ取りカーブtoggle",
            "テクスチャ塗り",
            "ノイズテクスチャレイヤー",
            "選択範囲をガイドに",
        ];

        names.sort_by(|lhs, rhs| compare_display_names(lhs, rhs));

        assert_eq!(
            names,
            [
                "_GIT",
                "ActionHelperScripts",
                "Illustrator に移動",
                "Import-Export-Guides-Photoshop",
                "InDesignで表示する",
                "KM",
                "opacity_Enhancer",
                "Photoshop",
                "Scripts",
                "StarLayerToggle",
                "test",
                "ゴミ取りカーブtoggle",
                "テクスチャ塗り",
                "ノイズテクスチャレイヤー",
                "選択範囲をガイドに",
            ]
        );
    }

    #[test]
    fn file_list_entries_keep_directories_before_files() {
        let mut entries = [
            entry("b.jsx", false),
            entry("a-folder", true),
            entry("a.jsx", false),
            entry("_folder", true),
        ];

        entries.sort_by(compare_file_list_entries);

        let ordered: Vec<_> = entries
            .iter()
            .map(|entry| display_name(&entry.display_path).into_owned())
            .collect();
        assert_eq!(ordered, ["_folder", "a-folder", "a.jsx", "b.jsx"]);
    }

    #[test]
    fn package_detection_is_case_insensitive() {
        assert!(is_package_path(Path::new("Example.APP")));
        assert!(is_package_path(Path::new("Example.Framework")));
    }

    #[test]
    fn file_list_node_limit_counts_non_script_entries() {
        let directory = tempfile::tempdir().expect("tempdir");
        fs::write(directory.path().join("one.txt"), "one").expect("write one");
        fs::write(directory.path().join("two.txt"), "two").expect("write two");

        let options = ScannerOptions {
            max_nodes_per_root: 1,
            ..Default::default()
        };
        let output = scan_file_list_root(
            &RootId::from("root"),
            directory.path(),
            &options,
            &ExtensionPolicy::script_default(),
        );

        assert!(output.truncated);
        assert_eq!(output.root.item_count, 0);
        assert!(output.children.is_empty());
    }
}
