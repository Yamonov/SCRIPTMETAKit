use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, Read, Take},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    RootId, TimestampMillis,
    catalog::{
        FileIdentity, FileListSnapshot, RootError, RootRegistration, RootSnapshot, RootStatus,
    },
    core::{
        OperationCancellation, ParserOptions, ScriptMetaEditState, ScriptMetaItem,
        ScriptMetaItemRef, ScriptRuntimeKind, VersionOrdering, compare_versions,
        decode_script_text, has_script_metadata_block_with_options,
        parse_script_metadata_with_options,
    },
    formats::{
        compiled_osa, detect_script_file, is_script_package_path,
        scriptmeta_edit_capability_from_cached_metadata,
        scriptmeta_edit_capability_from_file_list_probe, scriptmeta_edit_capability_from_metadata,
    },
    now_timestamp_millis,
    scanner::{ExtensionPolicy, ScannerOptions},
    watcher::normalize_path,
};

use super::path_resolution::{
    PathKind, PathResolutionStatus, path_error_status, resolve_scannable_path,
};
use super::{
    file_list::{FileSystemEntry, file_identity, system_time_millis},
    root_preflight::{root_content_preflight_issue, root_location_issue},
};
use crate::formats::compiled_osa::CompiledOsaErrorKind;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CandidateCache {
    pub schema_version: u32,
    pub built_at: TimestampMillis,
    pub registered_roots: Vec<RegisteredRootSignature>,
    pub records: Vec<CandidateRecord>,
}

impl CandidateCache {
    pub const CURRENT_SCHEMA_VERSION: u32 = 5;

    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: Self::CURRENT_SCHEMA_VERSION,
            built_at: now_timestamp_millis(),
            registered_roots: Vec::new(),
            records: Vec::new(),
        }
    }

    #[must_use]
    pub fn is_current_schema(&self) -> bool {
        self.schema_version == Self::CURRENT_SCHEMA_VERSION
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RegisteredRootSignature {
    pub root_id: RootId,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CandidateRecord {
    pub root_id: RootId,
    pub root_path: Arc<PathBuf>,
    pub file_path: PathBuf,
    pub identity_path: PathBuf,
    #[serde(default)]
    pub path_kind: PathKind,
    #[serde(default)]
    pub resolution_status: PathResolutionStatus,
    #[serde(default)]
    pub resolution_message: Option<String>,
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
    pub scriptmeta_edit_state: ScriptMetaEditState,
    pub file_size: Option<u64>,
    pub content_modified_at: Option<TimestampMillis>,
    #[serde(default)]
    pub identity: Option<FileIdentity>,
    pub item: Option<ScriptMetaItemRef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataScanOutput {
    pub roots: Vec<RootSnapshot>,
    pub all_items: Vec<ScriptMetaItemRef>,
    pub file_items: Vec<ScriptMetaItemRef>,
    pub candidate_cache: CandidateCache,
    pub source_revision: Uuid,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct MetadataScanSources<'a> {
    pub previous_cache: Option<&'a CandidateCache>,
    pub dirty_directories_by_root: Option<&'a BTreeMap<RootId, Vec<PathBuf>>>,
    pub file_list_snapshots: Option<&'a [FileListSnapshot]>,
}

pub fn scan_metadata_roots<'a, I>(
    roots: I,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    previous_cache: Option<&CandidateCache>,
) -> MetadataScanOutput
where
    I: IntoIterator<Item = &'a RootRegistration>,
{
    scan_metadata_roots_scoped(roots, options, extensions, previous_cache, None)
}

pub fn scan_metadata_roots_scoped<'a, I>(
    roots: I,
    options: &ScannerOptions,
    extensions: &ExtensionPolicy,
    previous_cache: Option<&CandidateCache>,
    dirty_directories_by_root: Option<&BTreeMap<RootId, Vec<PathBuf>>>,
) -> MetadataScanOutput
where
    I: IntoIterator<Item = &'a RootRegistration>,
{
    let parser_options = ParserOptions {
        max_prefix_bytes: options.max_prefix_bytes,
        ..ParserOptions::default()
    };
    scan_metadata_roots_scoped_controlled(
        roots,
        options,
        &parser_options,
        extensions,
        previous_cache,
        dirty_directories_by_root,
        None,
    )
}

pub(crate) fn scan_metadata_roots_scoped_controlled<'a, I>(
    roots: I,
    options: &ScannerOptions,
    parser_options: &ParserOptions,
    extensions: &ExtensionPolicy,
    previous_cache: Option<&CandidateCache>,
    dirty_directories_by_root: Option<&BTreeMap<RootId, Vec<PathBuf>>>,
    cancellation: Option<&OperationCancellation>,
) -> MetadataScanOutput
where
    I: IntoIterator<Item = &'a RootRegistration>,
{
    scan_metadata_roots_scoped_with_file_lists_controlled(
        roots,
        options,
        parser_options,
        extensions,
        MetadataScanSources {
            previous_cache,
            dirty_directories_by_root,
            file_list_snapshots: None,
        },
        cancellation,
    )
}

pub(crate) fn scan_metadata_roots_scoped_with_file_lists_controlled<'a, I>(
    roots: I,
    options: &ScannerOptions,
    parser_options: &ParserOptions,
    extensions: &ExtensionPolicy,
    sources: MetadataScanSources<'_>,
    cancellation: Option<&OperationCancellation>,
) -> MetadataScanOutput
where
    I: IntoIterator<Item = &'a RootRegistration>,
{
    let roots: Vec<_> = roots.into_iter().collect();
    let reusable_records = reusable_records_by_identity_path(sources.previous_cache, options);
    let rules = MetadataScanRules {
        scanner: options,
        parser: parser_options,
        extensions,
    };
    let mut records = Vec::new();
    let mut root_snapshots = Vec::new();

    for mut output in scan_metadata_root_outputs(
        &roots,
        rules,
        sources.previous_cache,
        &reusable_records,
        sources.dirty_directories_by_root,
        sources.file_list_snapshots,
        cancellation,
    ) {
        if !matches!(output.root.status, RootStatus::Ready | RootStatus::Missing)
            && let Some(previous_cache) = sources
                .previous_cache
                .filter(|cache| cache.is_current_schema())
        {
            output.records = previous_metadata_records(previous_cache, &output.root.root_id);
            output.root.item_count = output
                .records
                .iter()
                .filter(|record| record.item.is_some())
                .count();
            output.root.is_dirty = true;
        }
        root_snapshots.push(output.root);
        records.extend(output.records);
    }

    let candidate_cache = CandidateCache {
        schema_version: CandidateCache::CURRENT_SCHEMA_VERSION,
        built_at: now_timestamp_millis(),
        registered_roots: registered_root_signatures(&roots),
        records,
    };

    let file_items = file_items_from_cache(&candidate_cache);
    let all_items = deduplicated_items(&file_items);

    MetadataScanOutput {
        roots: root_snapshots,
        all_items,
        file_items,
        candidate_cache,
        source_revision: Uuid::new_v4(),
    }
}

#[derive(Clone, Copy)]
struct MetadataScanRules<'a> {
    scanner: &'a ScannerOptions,
    parser: &'a ParserOptions,
    extensions: &'a ExtensionPolicy,
}

#[derive(Debug)]
struct MetadataRootScanOutput {
    root: RootSnapshot,
    records: Vec<CandidateRecord>,
}

fn scan_metadata_root_outputs<'a>(
    roots: &[&'a RootRegistration],
    rules: MetadataScanRules<'a>,
    previous_cache: Option<&CandidateCache>,
    reusable_records: &BTreeMap<&'a Path, &'a CandidateRecord>,
    dirty_directories_by_root: Option<&BTreeMap<RootId, Vec<PathBuf>>>,
    file_list_snapshots: Option<&[FileListSnapshot]>,
    cancellation: Option<&OperationCancellation>,
) -> Vec<MetadataRootScanOutput> {
    if roots.is_empty() {
        return Vec::new();
    }

    let parallelism = metadata_scan_parallelism(roots.len());
    if parallelism <= 1 {
        return roots
            .iter()
            .map(|root| {
                let dirty_directories =
                    dirty_directories_by_root.and_then(|dirty| dirty.get(&root.root_id));
                scan_metadata_root_job(
                    root,
                    rules,
                    previous_cache,
                    reusable_records,
                    dirty_directories.map(Vec::as_slice),
                    file_list_snapshots,
                    cancellation,
                )
            })
            .collect();
    }

    let next_root_index = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    thread::scope(|scope| {
        for _ in 0..parallelism {
            let next_root_index = &next_root_index;
            let sender = sender.clone();
            scope.spawn(move || {
                loop {
                    let index = next_root_index.fetch_add(1, Ordering::Relaxed);
                    let Some(root) = roots.get(index) else {
                        break;
                    };
                    let dirty_directories =
                        dirty_directories_by_root.and_then(|dirty| dirty.get(&root.root_id));
                    let output = scan_metadata_root_job(
                        root,
                        rules,
                        previous_cache,
                        reusable_records,
                        dirty_directories.map(Vec::as_slice),
                        file_list_snapshots,
                        cancellation,
                    );
                    let _ = sender.send((index, output));
                }
            });
        }
        drop(sender);

        let mut outputs: Vec<_> = receiver.into_iter().collect();
        outputs.sort_by_key(|(index, _)| *index);
        outputs.into_iter().map(|(_, output)| output).collect()
    })
}

fn scan_metadata_root_job<'a>(
    root: &'a RootRegistration,
    rules: MetadataScanRules<'a>,
    previous_cache: Option<&CandidateCache>,
    reusable_records: &BTreeMap<&'a Path, &'a CandidateRecord>,
    dirty_directories: Option<&[PathBuf]>,
    file_list_snapshots: Option<&[FileListSnapshot]>,
    cancellation: Option<&OperationCancellation>,
) -> MetadataRootScanOutput {
    let shared_snapshot = file_list_snapshots.and_then(|snapshots| {
        snapshots
            .iter()
            .find(|snapshot| snapshot.root.root_id == root.root_id)
    });
    if let Some(snapshot) = shared_snapshot {
        if matches!(
            snapshot.root.status,
            RootStatus::Ready | RootStatus::Missing
        ) {
            return scan_metadata_root_from_file_list(
                root,
                rules,
                previous_cache,
                reusable_records,
                snapshot,
                dirty_directories,
                cancellation,
            );
        }
        let records = previous_cache
            .map(|cache| previous_metadata_records(cache, &root.root_id))
            .unwrap_or_default();
        let mut root_snapshot = snapshot.root.clone();
        root_snapshot.is_dirty = true;
        root_snapshot.item_count = records
            .iter()
            .filter(|record| record.item.is_some())
            .count();
        return MetadataRootScanOutput {
            root: root_snapshot,
            records,
        };
    }

    scan_metadata_root(
        root,
        rules,
        previous_cache,
        reusable_records,
        dirty_directories,
        cancellation,
    )
}

fn scan_metadata_root_from_file_list<'a>(
    root: &'a RootRegistration,
    rules: MetadataScanRules<'a>,
    previous_cache: Option<&CandidateCache>,
    reusable_records: &BTreeMap<&'a Path, &'a CandidateRecord>,
    file_list_snapshot: &FileListSnapshot,
    dirty_directories: Option<&[PathBuf]>,
    cancellation: Option<&OperationCancellation>,
) -> MetadataRootScanOutput {
    if file_list_snapshot.root.status == RootStatus::Missing {
        return MetadataRootScanOutput {
            root: file_list_snapshot.root.clone(),
            records: Vec::new(),
        };
    }

    let options = rules.scanner;
    let normalized_root_path = normalize_path(&root.path);
    let normalized_dirty_directories = dirty_directories
        .map(|directories| normalize_dirty_directories(directories, &normalized_root_path))
        .filter(|directories| !directories.is_empty());
    let dirty_refresh_context = normalized_dirty_directories
        .as_deref()
        .filter(|directories| {
            !directories
                .iter()
                .any(|directory| directory == &normalized_root_path)
        })
        .and_then(|directories| {
            previous_cache
                .filter(|cache| cache.is_current_schema())
                .map(|cache| (cache, directories))
        });
    let mut state = MetadataWalkState {
        root,
        root_path: Arc::new(root.path.clone()),
        options,
        parser_options: rules.parser,
        extensions: rules.extensions,
        reusable_records,
        allow_record_reuse: dirty_directories.is_none(),
        timeout: options
            .scan_timeout_per_root_millis
            .map(Duration::from_millis),
        started: Instant::now(),
        visited_directories: BTreeSet::new(),
        visited_nodes: 0,
        scanned_nodes: 0,
        limit_hit: None,
        timed_out: false,
        cancelled: false,
        root_error: None,
        cancellation,
    };
    let mut records = Vec::new();
    collect_metadata_from_file_list_entries(
        file_list_snapshot.children.as_deref().unwrap_or_default(),
        0,
        dirty_refresh_context.map(|(_, directories)| directories),
        &mut state,
        &mut records,
    );

    let mut snapshot = RootSnapshot::new(root.root_id.clone(), root.path.clone());
    snapshot.status = if state.cancelled {
        snapshot.error = Some(cancelled_root_error());
        RootStatus::Cancelled
    } else if let Some(limit_hit) = state.limit_hit {
        snapshot.error = Some(limit_hit.root_error(options));
        RootStatus::Overflowed
    } else if state.timed_out {
        RootStatus::TimedOut
    } else {
        RootStatus::Ready
    };
    if let Some((previous_cache, dirty_directories)) = dirty_refresh_context {
        if snapshot.status == RootStatus::Ready {
            records = merge_dirty_metadata_records(
                previous_cache,
                &root.root_id,
                dirty_directories,
                records,
            );
        } else {
            records = previous_metadata_records(previous_cache, &root.root_id);
        }
    }
    snapshot.is_dirty = dirty_refresh_context.is_some() && snapshot.status != RootStatus::Ready;
    snapshot.last_loaded_at = Some(now_timestamp_millis());
    snapshot.item_count = records
        .iter()
        .filter(|record| record.item.is_some())
        .count();

    MetadataRootScanOutput {
        root: snapshot,
        records,
    }
}

fn collect_metadata_from_file_list_entries(
    entries: &[FileSystemEntry],
    depth: usize,
    dirty_directories: Option<&[PathBuf]>,
    state: &mut MetadataWalkState<'_>,
    records: &mut Vec<CandidateRecord>,
) {
    for entry in entries {
        if !file_list_entry_intersects_directories(entry, dirty_directories) {
            continue;
        }
        if should_stop(depth, state) {
            return;
        }
        state.scanned_nodes = state.scanned_nodes.saturating_add(1);

        if entry.is_directory {
            collect_metadata_from_file_list_entries(
                &entry.children,
                depth + 1,
                dirty_directories,
                state,
                records,
            );
            continue;
        }
        if is_script_package_path(&entry.display_path)
            || is_script_package_path(&entry.resolved_path)
            || !state.extensions.contains_path(&entry.resolved_path)
        {
            continue;
        }

        state.visited_nodes = state.visited_nodes.saturating_add(1);
        if entry.file_size.is_none() || should_skip_resolution_error(entry.resolution_status) {
            records.push(candidate_error_record(
                &entry.display_path,
                &entry.resolved_path,
                entry.path_kind,
                entry.resolution_status,
                entry.resolution_message.clone(),
                state,
            ));
            continue;
        }

        let metadata = match fs::metadata(&entry.resolved_path) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => continue,
            Err(error) => {
                records.push(candidate_error_record(
                    &entry.display_path,
                    &entry.resolved_path,
                    entry.path_kind,
                    path_error_status(&error),
                    Some(error.to_string()),
                    state,
                ));
                continue;
            }
        };

        records.push(candidate_record_from_observation(
            &entry.display_path,
            &entry.resolved_path,
            entry.path_kind,
            entry.resolution_status,
            entry.resolution_message.clone(),
            Some(metadata.len()),
            metadata.modified().ok().and_then(system_time_millis),
            file_identity(&entry.resolved_path, &metadata),
            state,
        ));
    }
}

fn file_list_entry_intersects_directories(
    entry: &FileSystemEntry,
    dirty_directories: Option<&[PathBuf]>,
) -> bool {
    let Some(dirty_directories) = dirty_directories else {
        return true;
    };
    let resolved_path = normalize_path(&entry.resolved_path);
    let display_path = normalize_path(&entry.display_path);
    dirty_directories.iter().any(|directory| {
        resolved_path.starts_with(directory)
            || directory.starts_with(&resolved_path)
            || display_path.starts_with(directory)
            || directory.starts_with(&display_path)
    })
}

fn scan_metadata_root<'a>(
    root: &'a RootRegistration,
    rules: MetadataScanRules<'a>,
    previous_cache: Option<&CandidateCache>,
    reusable_records: &BTreeMap<&'a Path, &'a CandidateRecord>,
    dirty_directories: Option<&[PathBuf]>,
    cancellation: Option<&OperationCancellation>,
) -> MetadataRootScanOutput {
    let options = rules.scanner;
    let extensions = rules.extensions;
    let started = Instant::now();
    let timeout = options
        .scan_timeout_per_root_millis
        .map(Duration::from_millis);
    let mut snapshot = RootSnapshot::new(root.root_id.clone(), root.path.clone());
    let mut records = Vec::new();
    let is_dirty_refresh = dirty_directories.is_some();

    match root.path.try_exists() {
        Ok(true) => {}
        Ok(false) => {
            snapshot.status = RootStatus::Missing;
            snapshot.error = Some(missing_root_error());
            return MetadataRootScanOutput {
                root: snapshot,
                records,
            };
        }
        Err(error) => {
            let (status, root_error) = root_io_error(&error, "root_path_check_failed");
            snapshot.status = status;
            snapshot.error = Some(root_error);
            return MetadataRootScanOutput {
                root: snapshot,
                records,
            };
        }
    }
    if let Some((status, root_error)) = root_location_issue(&root.path, options) {
        snapshot.status = status;
        snapshot.error = Some(root_error);
        return MetadataRootScanOutput {
            root: snapshot,
            records,
        };
    }

    let mut state = MetadataWalkState {
        root,
        root_path: Arc::new(root.path.clone()),
        options,
        parser_options: rules.parser,
        extensions,
        reusable_records,
        allow_record_reuse: !is_dirty_refresh,
        timeout,
        started,
        visited_directories: BTreeSet::new(),
        visited_nodes: 0,
        scanned_nodes: 0,
        limit_hit: None,
        timed_out: false,
        cancelled: false,
        root_error: None,
        cancellation,
    };

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
        return MetadataRootScanOutput {
            root: snapshot,
            records,
        };
    }
    let root_metadata = match fs::metadata(&root_resolution.resolved_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            let (status, root_error) = root_io_error(&error, "unresolved_root");
            snapshot.status = status;
            snapshot.error = Some(root_error);
            return MetadataRootScanOutput {
                root: snapshot,
                records,
            };
        }
    };
    if !root_metadata.is_dir() {
        snapshot.status = RootStatus::Missing;
        snapshot.error = Some(RootError {
            code: "not_directory".to_string(),
            message: "root path does not resolve to a directory".to_string(),
        });
        return MetadataRootScanOutput {
            root: snapshot,
            records,
        };
    }
    let normalized_root_path = normalize_path(&root_resolution.resolved_path);
    let normalized_dirty_directories = dirty_directories
        .map(|directories| normalize_dirty_directories(directories, &normalized_root_path))
        .filter(|directories| !directories.is_empty());

    let dirty_refresh_context = if let Some(dirty_directories) =
        normalized_dirty_directories.as_deref()
        && !dirty_directories
            .iter()
            .any(|directory| directory == &normalized_root_path)
        && let Some(previous_cache) = previous_cache.filter(|cache| cache.is_current_schema())
    {
        scan_dirty_directories(
            dirty_directories,
            &normalized_root_path,
            &mut state,
            &mut records,
        );
        Some((previous_cache, dirty_directories))
    } else {
        if let Some((status, root_error)) =
            root_content_preflight_issue(&root_resolution.resolved_path, options, extensions)
        {
            snapshot.status = status;
            snapshot.error = Some(root_error);
            return MetadataRootScanOutput {
                root: snapshot,
                records,
            };
        }
        scan_directory(
            &root.path,
            &root_resolution.resolved_path,
            0,
            &mut state,
            &mut records,
        );
        None
    };
    if let Some(error) = state.root_error {
        snapshot.status = RootStatus::Unreadable;
        snapshot.error = Some(error);
    } else {
        snapshot.status = if state.cancelled {
            snapshot.error = Some(cancelled_root_error());
            RootStatus::Cancelled
        } else if let Some(limit_hit) = state.limit_hit {
            snapshot.error = Some(limit_hit.root_error(state.options));
            RootStatus::Overflowed
        } else if state.timed_out {
            RootStatus::TimedOut
        } else {
            RootStatus::Ready
        };
    }
    if let Some((previous_cache, dirty_directories)) = dirty_refresh_context.as_ref() {
        if snapshot.status == RootStatus::Ready {
            records = merge_dirty_metadata_records(
                previous_cache,
                &root.root_id,
                dirty_directories,
                records,
            );
        } else {
            records = previous_metadata_records(previous_cache, &root.root_id);
        }
    }
    snapshot.is_dirty = dirty_refresh_context.is_some() && snapshot.status != RootStatus::Ready;
    snapshot.last_loaded_at = Some(now_timestamp_millis());
    snapshot.item_count = records
        .iter()
        .filter(|record| record.item.is_some())
        .count();

    MetadataRootScanOutput {
        root: snapshot,
        records,
    }
}

fn normalize_dirty_directories(directories: &[PathBuf], root_path: &Path) -> Vec<PathBuf> {
    let mut normalized = directories
        .iter()
        .map(|directory| normalize_path(directory))
        .filter(|directory| path_is_same_or_child(directory, root_path))
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn scan_dirty_directories(
    dirty_directories: &[PathBuf],
    root_path: &Path,
    state: &mut MetadataWalkState<'_>,
    records: &mut Vec<CandidateRecord>,
) {
    for dirty_directory in dirty_directories {
        if should_stop(relative_depth(root_path, dirty_directory), state) {
            return;
        }
        let metadata = match fs::metadata(dirty_directory) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                let (_, root_error) = root_io_error(&error, "dirty_directory_metadata_failed");
                state.root_error.get_or_insert(root_error);
                continue;
            }
        };
        if !metadata.is_dir() {
            continue;
        }
        scan_directory(
            dirty_directory,
            dirty_directory,
            relative_depth(root_path, dirty_directory),
            state,
            records,
        );
    }
}

fn merge_dirty_metadata_records(
    previous_cache: &CandidateCache,
    root_id: &RootId,
    dirty_directories: &[PathBuf],
    mut refreshed_records: Vec<CandidateRecord>,
) -> Vec<CandidateRecord> {
    let mut records = previous_cache
        .records
        .iter()
        .filter(|record| record.root_id == *root_id)
        .filter(|record| {
            !dirty_directories.iter().any(|directory| {
                path_is_same_or_child(&record.identity_path, directory)
                    || path_is_same_or_child(&record.file_path, directory)
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    records.append(&mut refreshed_records);
    records.sort_by(|lhs, rhs| lhs.identity_path.cmp(&rhs.identity_path));
    records
}

fn previous_metadata_records(
    previous_cache: &CandidateCache,
    root_id: &RootId,
) -> Vec<CandidateRecord> {
    previous_cache
        .records
        .iter()
        .filter(|record| record.root_id == *root_id)
        .cloned()
        .collect()
}

fn relative_depth(root_path: &Path, path: &Path) -> usize {
    path.strip_prefix(root_path)
        .map(|relative| relative.components().count())
        .unwrap_or(0)
}

fn metadata_scan_parallelism(job_count: usize) -> usize {
    if job_count <= 1 {
        return job_count;
    }
    thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .max(1)
        .min(job_count)
}

struct MetadataWalkState<'a> {
    root: &'a RootRegistration,
    root_path: Arc<PathBuf>,
    options: &'a ScannerOptions,
    parser_options: &'a ParserOptions,
    extensions: &'a ExtensionPolicy,
    reusable_records: &'a BTreeMap<&'a Path, &'a CandidateRecord>,
    allow_record_reuse: bool,
    timeout: Option<Duration>,
    started: Instant,
    visited_directories: BTreeSet<PathBuf>,
    visited_nodes: usize,
    scanned_nodes: usize,
    limit_hit: Option<MetadataScanLimit>,
    timed_out: bool,
    cancelled: bool,
    root_error: Option<RootError>,
    cancellation: Option<&'a OperationCancellation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MetadataScanLimit {
    MaxDepth,
    MaxNodes,
}

impl MetadataScanLimit {
    fn root_error(self, options: &ScannerOptions) -> RootError {
        match self {
            Self::MaxDepth => RootError {
                code: "max_depth_exceeded".to_string(),
                message: format!("metadata scan reached max_depth ({})", options.max_depth),
            },
            Self::MaxNodes => RootError {
                code: "max_nodes_exceeded".to_string(),
                message: format!(
                    "metadata scan reached max_nodes_per_root ({})",
                    options.max_nodes_per_root
                ),
            },
        }
    }
}

fn scan_directory(
    display_directory: &Path,
    source_directory: &Path,
    depth: usize,
    state: &mut MetadataWalkState<'_>,
    records: &mut Vec<CandidateRecord>,
) {
    if should_stop(depth, state) {
        return;
    }

    let resolved_directory = normalize_path(source_directory);
    if !state.visited_directories.insert(resolved_directory) {
        return;
    }

    let entries = match fs::read_dir(source_directory) {
        Ok(entries) => entries,
        Err(error) => {
            if state.root_error.is_none() {
                let (_, root_error) = root_io_error(&error, "read_directory_failed");
                state.root_error = Some(root_error);
            }
            return;
        }
    };

    for entry_result in entries {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(error) => {
                if state.root_error.is_none() {
                    let (_, root_error) = root_io_error(&error, "read_directory_entry_failed");
                    state.root_error = Some(root_error);
                }
                continue;
            }
        };
        if should_stop(depth, state) {
            return;
        }
        state.scanned_nodes = state.scanned_nodes.saturating_add(1);

        let source_path = entry.path();
        let display_path = display_directory.join(entry.file_name());
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
            continue;
        }
        if is_script_package_path(&resolved.display_path)
            || is_script_package_path(&resolved.resolved_path)
        {
            continue;
        }
        if should_skip_resolution_error(resolved.resolution_status) {
            if state.extensions.contains_path(&resolved.resolved_path) {
                state.visited_nodes += 1;
                records.push(candidate_error_record(
                    &resolved.display_path,
                    &resolved.resolved_path,
                    resolved.path_kind,
                    resolved.resolution_status,
                    resolved.resolution_message,
                    state,
                ));
            }
            continue;
        }
        if state.options.skip_packages && is_package_path(&resolved.resolved_path) {
            continue;
        }

        let metadata = match fs::metadata(&resolved.resolved_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                let status = path_error_status(&error);
                if status == PathResolutionStatus::PermissionDenied {
                    state.visited_nodes += 1;
                    if state.extensions.contains_path(&resolved.resolved_path) {
                        records.push(candidate_error_record(
                            &resolved.display_path,
                            &resolved.resolved_path,
                            resolved.path_kind,
                            status,
                            Some(error.to_string()),
                            state,
                        ));
                    }
                }
                continue;
            }
        };

        if metadata.is_dir() {
            let resolved_directory = normalize_path(&resolved.resolved_path);
            if !state.visited_directories.contains(&resolved_directory) {
                scan_directory(
                    &resolved.display_path,
                    &resolved.resolved_path,
                    depth + 1,
                    state,
                    records,
                );
            }
            continue;
        }

        if !metadata.is_file() || !state.extensions.contains_path(&resolved.resolved_path) {
            continue;
        }

        state.visited_nodes += 1;
        records.push(candidate_record(
            &resolved.display_path,
            &resolved.resolved_path,
            resolved.path_kind,
            resolved.resolution_status,
            resolved.resolution_message.clone(),
            &metadata,
            state,
        ));
    }
}

fn should_skip_resolution_error(status: PathResolutionStatus) -> bool {
    status != PathResolutionStatus::NotRequested && status != PathResolutionStatus::Resolved
}

fn candidate_record(
    file_path: &Path,
    identity_path: &Path,
    path_kind: PathKind,
    resolution_status: PathResolutionStatus,
    resolution_message: Option<String>,
    metadata: &fs::Metadata,
    state: &MetadataWalkState<'_>,
) -> CandidateRecord {
    candidate_record_from_observation(
        file_path,
        identity_path,
        path_kind,
        resolution_status,
        resolution_message,
        Some(metadata.len()),
        metadata.modified().ok().and_then(system_time_millis),
        file_identity(identity_path, metadata),
        state,
    )
}

#[allow(clippy::too_many_arguments)]
fn candidate_record_from_observation(
    file_path: &Path,
    identity_path: &Path,
    path_kind: PathKind,
    resolution_status: PathResolutionStatus,
    resolution_message: Option<String>,
    file_size: Option<u64>,
    content_modified_at: Option<TimestampMillis>,
    identity: Option<FileIdentity>,
    state: &MetadataWalkState<'_>,
) -> CandidateRecord {
    if let Some(record) = state.reusable_records.get(identity_path).filter(|record| {
        state.allow_record_reuse
            && state.options.reuse_unchanged_records
            && record.file_size == file_size
            && record.content_modified_at == content_modified_at
            && match (&record.identity, &identity) {
                (Some(previous), Some(current)) => previous == current,
                _ => true,
            }
    }) {
        let reused_capability = scriptmeta_edit_capability_from_cached_metadata(
            identity_path,
            record.scriptmeta_edit_state,
            record.has_scriptmeta,
            record.has_scriptmeta_edit_password,
        );
        return CandidateRecord {
            root_id: state.root.root_id.clone(),
            root_path: Arc::clone(&state.root_path),
            file_path: file_path.to_path_buf(),
            identity_path: identity_path.to_path_buf(),
            path_kind,
            resolution_status,
            resolution_message,
            runtime_kind: record.runtime_kind,
            shebang: record.shebang.clone(),
            has_scriptmeta: record.has_scriptmeta,
            has_scriptmeta_edit_password: record.has_scriptmeta_edit_password,
            is_file_locked: reused_capability.is_file_locked,
            is_read_only: reused_capability.is_read_only,
            can_edit_scriptmeta: reused_capability.can_edit_scriptmeta,
            can_append_scriptmeta: reused_capability.can_append_scriptmeta,
            scriptmeta_edit_state: reused_capability.scriptmeta_edit_state,
            file_size,
            content_modified_at,
            identity,
            item: record.item.as_ref().map(|item| {
                if can_reuse_script_item_ref(
                    item,
                    state.root,
                    file_path,
                    identity_path,
                    reused_capability,
                ) {
                    Arc::clone(item)
                } else {
                    Arc::new(
                        item.with_location(
                            state.root.root_id.clone(),
                            file_path.to_path_buf(),
                            identity_path.to_path_buf(),
                        )
                        .with_scriptmeta_edit_capability(reused_capability),
                    )
                }
            }),
        };
    }

    let source_result = read_script_source(
        identity_path,
        state.parser_options.max_prefix_bytes,
        file_size.unwrap_or_default(),
        compiled_osa_timeout_for_state(state),
        state.options.decompile_compiled_osa_during_scan,
    );
    let (
        source_text,
        runtime_kind_override,
        metadata_state_known,
        resolution_status,
        resolution_message,
    ) = match source_result {
        ScriptSourceReadResult::Read(source) => (
            Some(source.text),
            source.runtime_kind,
            true,
            resolution_status,
            resolution_message,
        ),
        ScriptSourceReadResult::Skipped => {
            (None, None, false, resolution_status, resolution_message)
        }
        ScriptSourceReadResult::Failed(error) => (
            None,
            None,
            true,
            error.resolution_status,
            Some(error.message),
        ),
    };
    let script_info = detect_script_file(identity_path, source_text.as_deref());
    let runtime_kind = runtime_kind_override.or(script_info.runtime_kind);
    let has_scriptmeta = source_text
        .as_deref()
        .is_some_and(|text| has_script_metadata_block_with_options(text, state.parser_options));
    let parsed_metadata = source_text
        .as_deref()
        .and_then(|text| parse_script_metadata_with_options(text, state.parser_options).ok());
    let has_scriptmeta_edit_password = parsed_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.edit_password_sha256.is_some());
    let capability = if metadata_state_known {
        scriptmeta_edit_capability_from_metadata(
            identity_path,
            source_text.as_deref().map(str::as_bytes),
            has_scriptmeta,
            has_scriptmeta_edit_password,
        )
    } else {
        scriptmeta_edit_capability_from_file_list_probe(identity_path, None)
    };
    let item = source_text.and(parsed_metadata).map(|metadata| {
        Arc::new(
            ScriptMetaItem::from_metadata(
                state.root.root_id.clone(),
                file_path.to_path_buf(),
                identity_path.to_path_buf(),
                metadata,
            )
            .with_script_file_info(runtime_kind, script_info.shebang.clone())
            .with_scriptmeta_edit_capability(capability),
        )
    });

    CandidateRecord {
        root_id: state.root.root_id.clone(),
        root_path: Arc::clone(&state.root_path),
        file_path: file_path.to_path_buf(),
        identity_path: identity_path.to_path_buf(),
        path_kind,
        resolution_status,
        resolution_message,
        runtime_kind,
        shebang: script_info.shebang,
        has_scriptmeta,
        has_scriptmeta_edit_password,
        is_file_locked: capability.is_file_locked,
        is_read_only: capability.is_read_only,
        can_edit_scriptmeta: capability.can_edit_scriptmeta,
        can_append_scriptmeta: capability.can_append_scriptmeta,
        scriptmeta_edit_state: capability.scriptmeta_edit_state,
        file_size,
        content_modified_at,
        identity,
        item,
    }
}

fn candidate_error_record(
    file_path: &Path,
    identity_path: &Path,
    path_kind: PathKind,
    resolution_status: PathResolutionStatus,
    resolution_message: Option<String>,
    state: &MetadataWalkState<'_>,
) -> CandidateRecord {
    CandidateRecord {
        root_id: state.root.root_id.clone(),
        root_path: Arc::clone(&state.root_path),
        file_path: file_path.to_path_buf(),
        identity_path: identity_path.to_path_buf(),
        path_kind,
        resolution_status,
        resolution_message,
        runtime_kind: None,
        shebang: None,
        has_scriptmeta: false,
        has_scriptmeta_edit_password: false,
        is_file_locked: false,
        is_read_only: false,
        can_edit_scriptmeta: false,
        can_append_scriptmeta: false,
        scriptmeta_edit_state: ScriptMetaEditState::Unknown,
        file_size: None,
        content_modified_at: None,
        identity: None,
        item: None,
    }
}

fn read_prefix(path: &Path, max_bytes: usize, file_size: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut limited: Take<&mut File> = file.by_ref().take(max_bytes as u64);
    let capacity = usize::try_from(file_size)
        .unwrap_or(max_bytes)
        .min(max_bytes);
    let mut buffer = Vec::with_capacity(capacity);
    limited.read_to_end(&mut buffer)?;
    Ok(decode_script_text(&buffer))
}

struct ScriptSourceRead {
    text: String,
    runtime_kind: Option<ScriptRuntimeKind>,
}

struct ScriptSourceReadError {
    resolution_status: PathResolutionStatus,
    message: String,
}

enum ScriptSourceReadResult {
    Read(ScriptSourceRead),
    Skipped,
    Failed(ScriptSourceReadError),
}

fn read_script_source(
    path: &Path,
    max_bytes: usize,
    file_size: u64,
    compiled_osa_timeout: Duration,
    decompile_compiled_osa: bool,
) -> ScriptSourceReadResult {
    if compiled_osa::is_compiled_osa_path(path) {
        match compiled_osa::extract_compiled_osa_metadata_fast(path, max_bytes, file_size) {
            Ok(Some(snippet)) => {
                return ScriptSourceReadResult::Read(ScriptSourceRead {
                    text: snippet.text,
                    runtime_kind: snippet.language_hint,
                });
            }
            Ok(None) => {}
            Err(error) => {
                return ScriptSourceReadResult::Failed(ScriptSourceReadError {
                    resolution_status: compiled_osa_resolution_status(error.kind),
                    message: error.message,
                });
            }
        }

        if !decompile_compiled_osa {
            return ScriptSourceReadResult::Skipped;
        }

        return match compiled_osa::decompile_compiled_osa_source(path, Some(compiled_osa_timeout)) {
            Ok(source) => ScriptSourceReadResult::Read(ScriptSourceRead {
                text: source.source,
                runtime_kind: source.language_hint,
            }),
            Err(error) if compiled_osa_error_allows_text_fallback(error.kind) => {
                read_text_prefix_source(path, max_bytes, file_size)
            }
            Err(error) => ScriptSourceReadResult::Failed(ScriptSourceReadError {
                resolution_status: compiled_osa_resolution_status(error.kind),
                message: error.message,
            }),
        };
    }

    read_text_prefix_source(path, max_bytes, file_size)
}

fn read_text_prefix_source(
    path: &Path,
    max_bytes: usize,
    file_size: u64,
) -> ScriptSourceReadResult {
    match read_prefix(path, max_bytes, file_size) {
        Ok(text) => ScriptSourceReadResult::Read(ScriptSourceRead {
            text,
            runtime_kind: None,
        }),
        Err(error) => ScriptSourceReadResult::Failed(ScriptSourceReadError {
            resolution_status: path_error_status(&error),
            message: error.to_string(),
        }),
    }
}

fn compiled_osa_error_allows_text_fallback(kind: CompiledOsaErrorKind) -> bool {
    matches!(kind, CompiledOsaErrorKind::NotOsaScript)
}

fn compiled_osa_timeout_for_state(state: &MetadataWalkState<'_>) -> Duration {
    let default_timeout = Duration::from_millis(3_000);
    let Some(scan_timeout) = state.timeout else {
        return default_timeout;
    };
    scan_timeout
        .saturating_sub(state.started.elapsed())
        .min(default_timeout)
}

fn compiled_osa_resolution_status(kind: CompiledOsaErrorKind) -> PathResolutionStatus {
    match kind {
        CompiledOsaErrorKind::PermissionDenied => PathResolutionStatus::PermissionDenied,
        CompiledOsaErrorKind::SourceUnavailable
        | CompiledOsaErrorKind::NotOsaScript
        | CompiledOsaErrorKind::UnsupportedPlatform
        | CompiledOsaErrorKind::ToolUnavailable => PathResolutionStatus::Unsupported,
        CompiledOsaErrorKind::Timeout
        | CompiledOsaErrorKind::ProcessFailed
        | CompiledOsaErrorKind::Io => PathResolutionStatus::Broken,
    }
}

fn should_stop(depth: usize, state: &mut MetadataWalkState<'_>) -> bool {
    if state
        .cancellation
        .is_some_and(OperationCancellation::is_cancelled)
    {
        state.cancelled = true;
        return true;
    }

    if depth > state.options.max_depth {
        state.limit_hit = Some(MetadataScanLimit::MaxDepth);
        return true;
    }

    if state.scanned_nodes >= state.options.max_nodes_per_root {
        state.limit_hit = Some(MetadataScanLimit::MaxNodes);
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
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    if options.skip_hidden && name.starts_with('.') {
        return true;
    }

    options.skip_packages && is_package_path(path)
}

fn is_package_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("app" | "bundle" | "framework" | "plugin" | "appex")
    )
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

fn reusable_records_by_identity_path<'a>(
    previous_cache: Option<&'a CandidateCache>,
    options: &ScannerOptions,
) -> BTreeMap<&'a Path, &'a CandidateRecord> {
    if !options.reuse_unchanged_records {
        return BTreeMap::new();
    }

    previous_cache
        .filter(|cache| cache.is_current_schema())
        .map(|cache| {
            cache
                .records
                .iter()
                .map(|record| (record.identity_path.as_path(), record))
                .collect()
        })
        .unwrap_or_default()
}

fn can_reuse_script_item_ref(
    item: &ScriptMetaItem,
    root: &RootRegistration,
    file_path: &Path,
    identity_path: &Path,
    capability: crate::core::ScriptMetaEditCapability,
) -> bool {
    item.root_id == root.root_id
        && item.file_path == file_path
        && item.identity_path == identity_path
        && item.has_scriptmeta == capability.has_scriptmeta
        && item.has_scriptmeta_edit_password == capability.has_scriptmeta_edit_password
        && item.is_file_locked == capability.is_file_locked
        && item.is_read_only == capability.is_read_only
        && item.can_edit_scriptmeta == capability.can_edit_scriptmeta
        && item.can_append_scriptmeta == capability.can_append_scriptmeta
        && item.scriptmeta_edit_state == capability.scriptmeta_edit_state
}

fn path_is_same_or_child(path: &Path, parent: &Path) -> bool {
    path == parent || path.starts_with(parent)
}

pub(crate) fn registered_root_signatures(
    roots: &[&RootRegistration],
) -> Vec<RegisteredRootSignature> {
    let mut signatures = roots
        .iter()
        .map(|root| RegisteredRootSignature {
            root_id: root.root_id.clone(),
            path: normalize_path(&root.path),
        })
        .collect::<Vec<_>>();
    signatures.sort_by(|lhs, rhs| {
        lhs.root_id
            .cmp(&rhs.root_id)
            .then_with(|| lhs.path.cmp(&rhs.path))
    });
    signatures
}

pub(crate) fn file_items_from_cache(cache: &CandidateCache) -> Vec<ScriptMetaItemRef> {
    let mut items: Vec<_> = cache
        .records
        .iter()
        .filter_map(|record| record.item.clone())
        .collect();
    items.sort_by(|lhs, rhs| {
        lhs.root_id
            .cmp(&rhs.root_id)
            .then_with(|| lhs.file_path.cmp(&rhs.file_path))
            .then_with(|| lhs.identity_path.cmp(&rhs.identity_path))
    });
    items
}

pub(crate) fn deduplicated_items(items: &[ScriptMetaItemRef]) -> Vec<ScriptMetaItemRef> {
    let mut best_by_script_id: BTreeMap<&str, &ScriptMetaItemRef> = BTreeMap::new();
    for item in items {
        best_by_script_id
            .entry(item.script_id.as_str())
            .and_modify(|current| {
                if should_replace_item(current, item) {
                    *current = item;
                }
            })
            .or_insert(item);
    }

    let mut deduplicated: Vec<_> = best_by_script_id.into_values().cloned().collect();
    deduplicated.sort_by(|lhs, rhs| lhs.file_path.cmp(&rhs.file_path));
    deduplicated
}

fn should_replace_item(current: &ScriptMetaItem, candidate: &ScriptMetaItem) -> bool {
    match (&current.version, &candidate.version) {
        (None, Some(_)) => true,
        (Some(_), None) => false,
        (Some(current_version), Some(candidate_version)) => {
            match compare_versions(current_version, candidate_version) {
                VersionOrdering::Less => true,
                VersionOrdering::Equal => candidate.file_path < current.file_path,
                VersionOrdering::Greater => false,
            }
        }
        (None, None) => candidate.file_path < current.file_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::file_list::{
        scan_file_list_root, scan_file_list_root_with_dirty_directories,
    };
    use std::fs;

    #[test]
    fn shared_file_list_traversal_matches_independent_metadata_scan() {
        let directory = tempfile::tempdir().expect("tempdir");
        let nested = directory.path().join("Nested");
        fs::create_dir(&nested).expect("nested directory");
        fs::write(
            nested.join("Shared.jsx"),
            "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.shared-walk\n// Version=1.2.3\n// SCRIPTMETA-END\n",
        )
        .expect("script");
        fs::write(directory.path().join("ignored.txt"), "ignored").expect("ignored file");

        let root =
            RootRegistration::file_list_and_metadata(RootId::from("shared-root"), directory.path());
        let options = ScannerOptions::default();
        let parser_options = ParserOptions::default();
        let extensions = ExtensionPolicy::default();
        let independent = scan_metadata_roots_scoped_controlled(
            [&root],
            &options,
            &parser_options,
            &extensions,
            None,
            None,
            None,
        );

        let file_list = scan_file_list_root(&root.root_id, &root.path, &options, &extensions);
        let file_list_snapshot = FileListSnapshot {
            root: file_list.root,
            children: Some(file_list.children),
            directory_states: file_list.directory_states,
            truncated: file_list.truncated,
        };
        let shared = scan_metadata_roots_scoped_with_file_lists_controlled(
            [&root],
            &options,
            &parser_options,
            &extensions,
            MetadataScanSources {
                previous_cache: None,
                dirty_directories_by_root: None,
                file_list_snapshots: Some(std::slice::from_ref(&file_list_snapshot)),
            },
            None,
        );

        assert_eq!(shared.roots.len(), independent.roots.len());
        assert_eq!(shared.roots[0].root_id, independent.roots[0].root_id);
        assert_eq!(shared.roots[0].path, independent.roots[0].path);
        assert_eq!(shared.roots[0].status, independent.roots[0].status);
        assert_eq!(shared.roots[0].item_count, independent.roots[0].item_count);
        assert_eq!(shared.roots[0].error, independent.roots[0].error);
        assert_eq!(shared.file_items, independent.file_items);
        assert_eq!(shared.all_items, independent.all_items);
        assert_eq!(
            shared.candidate_cache.records,
            independent.candidate_cache.records
        );
    }

    #[test]
    fn shared_dirty_file_list_traversal_matches_independent_metadata_refresh() {
        let directory = tempfile::tempdir().expect("tempdir");
        let nested = directory.path().join("Nested");
        fs::create_dir(&nested).expect("nested directory");
        let script = nested.join("Dirty.jsx");
        fs::write(
            &script,
            "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.shared-dirty\n// Version=1.0.0\n// SCRIPTMETA-END\n",
        )
        .expect("initial script");

        let root = RootRegistration::file_list_and_metadata(
            RootId::from("shared-dirty-root"),
            directory.path(),
        );
        let options = ScannerOptions::default();
        let parser_options = ParserOptions::default();
        let extensions = ExtensionPolicy::default();
        let initial_metadata = scan_metadata_roots_scoped_controlled(
            [&root],
            &options,
            &parser_options,
            &extensions,
            None,
            None,
            None,
        );
        let initial_file_list =
            scan_file_list_root(&root.root_id, &root.path, &options, &extensions);
        let initial_file_list = FileListSnapshot {
            root: initial_file_list.root,
            children: Some(initial_file_list.children),
            directory_states: initial_file_list.directory_states,
            truncated: initial_file_list.truncated,
        };

        fs::write(
            &script,
            "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.shared-dirty\n// Version=2.0.0\n// SCRIPTMETA-END\n",
        )
        .expect("updated script");
        let dirty_directories = vec![nested];
        let refreshed_file_list = scan_file_list_root_with_dirty_directories(
            &root.root_id,
            &root.path,
            &options,
            &extensions,
            Some(&initial_file_list),
            &dirty_directories,
        );
        let refreshed_file_list = FileListSnapshot {
            root: refreshed_file_list.root,
            children: Some(refreshed_file_list.children),
            directory_states: refreshed_file_list.directory_states,
            truncated: refreshed_file_list.truncated,
        };
        let dirty_by_root = BTreeMap::from([(root.root_id.clone(), dirty_directories)]);

        let independent = scan_metadata_roots_scoped_controlled(
            [&root],
            &options,
            &parser_options,
            &extensions,
            Some(&initial_metadata.candidate_cache),
            Some(&dirty_by_root),
            None,
        );
        let shared = scan_metadata_roots_scoped_with_file_lists_controlled(
            [&root],
            &options,
            &parser_options,
            &extensions,
            MetadataScanSources {
                previous_cache: Some(&initial_metadata.candidate_cache),
                dirty_directories_by_root: Some(&dirty_by_root),
                file_list_snapshots: Some(std::slice::from_ref(&refreshed_file_list)),
            },
            None,
        );

        assert_eq!(shared.roots[0].status, independent.roots[0].status);
        assert_eq!(shared.roots[0].item_count, independent.roots[0].item_count);
        assert_eq!(shared.file_items, independent.file_items);
        assert_eq!(shared.all_items, independent.all_items);
        assert_eq!(
            shared.candidate_cache.records,
            independent.candidate_cache.records
        );
    }

    #[test]
    fn metadata_node_limit_counts_non_script_entries() {
        let directory = tempfile::tempdir().expect("tempdir");
        fs::write(directory.path().join("one.txt"), "one").expect("write one");
        fs::write(directory.path().join("two.txt"), "two").expect("write two");
        let root = RootRegistration::file_list_and_metadata(RootId::from("root"), directory.path());

        let options = ScannerOptions {
            max_nodes_per_root: 1,
            ..Default::default()
        };
        let output =
            scan_metadata_roots([&root], &options, &ExtensionPolicy::script_default(), None);

        assert_eq!(output.roots[0].status, RootStatus::Overflowed);
        assert_eq!(output.roots[0].item_count, 0);
        assert!(output.candidate_cache.records.is_empty());
    }
}
