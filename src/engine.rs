use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    RootId,
    catalog::{
        CacheInvalidationReason, CachePolicy, CacheScope, FileEntryChange, FileEntryChangeKind,
        FileListSnapshot, RefreshPolicy, RefreshRequest, RootPriority, RootPurpose,
        RootRegistration, RootSnapshot, RootStatus, ScanChangeSummary, ScanMode, ScanRequest,
        ScanResult, ScriptMetaCatalogSnapshot, ScriptMetaKitConfig, ScriptMetaKitEvent,
        UpdateCheckProgress, UpdateCheckProgressPhase, UpdateCheckRequest, UpdateCheckResult,
        UpdateFailure, UpdateStatus, path_based_root_id, unresolved_distribution,
    },
    core::{
        FileIssue, OperationCancellation, OperationSummary, ScriptMetaItem, ScriptMetaItemRef,
        ScriptMetaKitError, ScriptMetaKitResult,
    },
    now_timestamp_millis,
    resolver::{
        DistributionResolverOptions, HttpValidationCache, ResolvedItemUpdate, UpdateResolver,
        retry_after_hint_millis,
    },
    scanner::{
        CandidateCache, CandidateRecord, FileSystemEntry, MetadataScanSources, deduplicated_items,
        file_items_from_cache, registered_root_signatures,
        scan_file_list_root_transactional_controlled,
        scan_file_list_root_with_dirty_directories_controlled,
        scan_metadata_roots_scoped_with_file_lists_controlled,
        try_scan_file_list_root_with_owned_dirty_directories_controlled,
    },
    storage::CachePayload,
    watcher::{
        ChangeRoutingOptions, RawChangeBatch, WatchPlan, WatchPolicy, build_watch_plan,
        normalize_path, route_change_batch,
    },
};

#[derive(Debug)]
pub struct ScriptMetaKitEngine {
    config: ScriptMetaKitConfig,
    roots: Vec<RootRegistration>,
    root_groups: BTreeMap<String, BTreeMap<RootId, RootRegistration>>,
    visible_root_id: Option<RootId>,
    root_snapshots: BTreeMap<RootId, RootSnapshot>,
    file_list_snapshots: BTreeMap<RootId, Arc<FileListSnapshot>>,
    known_directory_paths: BTreeSet<PathBuf>,
    known_file_paths: BTreeSet<PathBuf>,
    catalog_snapshot: Option<Arc<ScriptMetaCatalogSnapshot>>,
    update_check_result: Option<Arc<UpdateCheckResult>>,
    dirty_roots: BTreeMap<RootId, DirtyRootState>,
    last_memory_cache_accessed_at: Option<crate::TimestampMillis>,
    memory_cache_export_allowed: bool,
    http_validation_cache: HttpValidationCache,
    cancellation: OperationCancellation,
}

impl Clone for ScriptMetaKitEngine {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            roots: self.roots.clone(),
            root_groups: self.root_groups.clone(),
            visible_root_id: self.visible_root_id.clone(),
            root_snapshots: self.root_snapshots.clone(),
            file_list_snapshots: self.file_list_snapshots.clone(),
            known_directory_paths: self.known_directory_paths.clone(),
            known_file_paths: self.known_file_paths.clone(),
            catalog_snapshot: self.catalog_snapshot.clone(),
            update_check_result: self.update_check_result.clone(),
            dirty_roots: self.dirty_roots.clone(),
            last_memory_cache_accessed_at: self.last_memory_cache_accessed_at,
            memory_cache_export_allowed: self.memory_cache_export_allowed,
            http_validation_cache: self.http_validation_cache.clone(),
            cancellation: OperationCancellation::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct DirtyRootState {
    dirty_directories: BTreeSet<PathBuf>,
    requires_full_rescan: bool,
}

#[derive(Clone)]
struct FileListScanJob {
    index: usize,
    root: RootRegistration,
    previous_snapshot: Option<Arc<FileListSnapshot>>,
    dirty_directories: Option<Vec<PathBuf>>,
    probe_script_headers: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UpdateCheckCacheMode {
    Replace,
    Merge,
}

#[derive(Clone, Copy)]
struct UpdateRetryPolicy {
    attempts: usize,
    initial_delay_millis: u64,
    backoff_multiplier: u32,
    max_delay_millis: u64,
}

impl ScriptMetaKitEngine {
    pub fn new(config: ScriptMetaKitConfig) -> ScriptMetaKitResult<Self> {
        validate_config(&config)?;
        Ok(Self {
            config,
            roots: Vec::new(),
            root_groups: BTreeMap::new(),
            visible_root_id: None,
            root_snapshots: BTreeMap::new(),
            file_list_snapshots: BTreeMap::new(),
            known_directory_paths: BTreeSet::new(),
            known_file_paths: BTreeSet::new(),
            catalog_snapshot: None,
            update_check_result: None,
            dirty_roots: BTreeMap::new(),
            last_memory_cache_accessed_at: None,
            memory_cache_export_allowed: true,
            http_validation_cache: HttpValidationCache::default(),
            cancellation: OperationCancellation::new(),
        })
    }

    #[must_use]
    pub fn config(&self) -> &ScriptMetaKitConfig {
        &self.config
    }

    #[must_use]
    pub fn config_mut(&mut self) -> &mut ScriptMetaKitConfig {
        &mut self.config
    }

    pub fn set_roots(
        &mut self,
        roots: Vec<RootRegistration>,
    ) -> ScriptMetaKitResult<Vec<ScriptMetaKitEvent>> {
        self.root_groups.clear();
        self.apply_roots(roots)
    }

    pub fn replace_root_group(
        &mut self,
        group_id: impl Into<String>,
        roots: Vec<RootRegistration>,
    ) -> ScriptMetaKitResult<Vec<ScriptMetaKitEvent>> {
        self.expire_idle_memory_cache();
        let group_id = group_id.into();
        if roots.is_empty() {
            self.root_groups.remove(&group_id);
        } else {
            self.root_groups.insert(group_id, root_map(roots)?);
        }
        self.apply_grouped_roots()
    }

    pub fn insert_roots_into_group(
        &mut self,
        group_id: impl Into<String>,
        roots: Vec<RootRegistration>,
    ) -> ScriptMetaKitResult<Vec<ScriptMetaKitEvent>> {
        self.expire_idle_memory_cache();
        let group = self.root_groups.entry(group_id.into()).or_default();
        for root in roots {
            group.insert(root.root_id.clone(), root);
        }
        self.apply_grouped_roots()
    }

    fn apply_grouped_roots(&mut self) -> ScriptMetaKitResult<Vec<ScriptMetaKitEvent>> {
        let mut merged_roots_by_id: BTreeMap<RootId, RootRegistration> = BTreeMap::new();
        for group_roots in self.root_groups.values() {
            for root in group_roots.values() {
                merged_roots_by_id
                    .entry(root.root_id.clone())
                    .and_modify(|existing| {
                        *existing = merged_root_registration(existing, root);
                    })
                    .or_insert_with(|| root.clone());
            }
        }
        self.apply_roots(merged_roots_by_id.into_values().collect())
    }

    fn apply_roots(
        &mut self,
        roots: Vec<RootRegistration>,
    ) -> ScriptMetaKitResult<Vec<ScriptMetaKitEvent>> {
        self.expire_idle_memory_cache();
        let previous_roots_by_id: BTreeMap<_, _> = self
            .roots
            .iter()
            .map(|root| (root.root_id.as_ref(), root))
            .collect();
        let previous_root_ids: BTreeSet<_> = self
            .roots
            .iter()
            .map(|root| root.root_id.as_ref())
            .collect();
        let next_root_ids: BTreeSet<_> = roots.iter().map(|root| root.root_id.as_ref()).collect();
        if next_root_ids.len() != roots.len() {
            return Err(ScriptMetaKitError::InvalidConfig(
                "root_id values must be unique".to_string(),
            ));
        }
        let mut events = Vec::new();
        for removed_id in previous_root_ids.difference(&next_root_ids) {
            self.root_snapshots.remove(*removed_id);
            self.file_list_snapshots.remove(*removed_id);
            self.dirty_roots.remove(*removed_id);
            events.push(ScriptMetaKitEvent::RootRemoved {
                root_id: (*removed_id).into(),
            });
        }

        let mut invalidate_catalog = previous_root_ids
            .difference(&next_root_ids)
            .next()
            .is_some();
        for root in &roots {
            if !previous_root_ids.contains(root.root_id.as_ref()) {
                events.push(ScriptMetaKitEvent::RootRegistered {
                    root_id: root.root_id.clone(),
                });
            }
            let changed = previous_roots_by_id
                .get(root.root_id.as_ref())
                .is_some_and(|previous| {
                    previous.path != root.path
                        || previous.purpose != root.purpose
                        || previous.cache_policy != root.cache_policy
                        || previous.refresh_policy != root.refresh_policy
                });
            if changed {
                self.file_list_snapshots.remove(&root.root_id);
                self.dirty_roots.remove(&root.root_id);
                self.root_snapshots.insert(
                    root.root_id.clone(),
                    RootSnapshot::new(root.root_id.clone(), root.path.clone()),
                );
                invalidate_catalog = true;
            } else {
                self.root_snapshots
                    .entry(root.root_id.clone())
                    .or_insert_with(|| RootSnapshot::new(root.root_id.clone(), root.path.clone()));
            }
        }

        self.roots = roots;
        self.rebuild_known_path_index();
        if invalidate_catalog {
            self.catalog_snapshot = None;
            self.update_check_result = None;
        }
        Ok(events)
    }

    #[must_use]
    pub fn roots(&self) -> &[RootRegistration] {
        &self.roots
    }

    pub fn set_root_paths<I, P>(
        &mut self,
        paths: I,
        purpose: RootPurpose,
    ) -> ScriptMetaKitResult<Vec<ScriptMetaKitEvent>>
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        let roots = paths
            .into_iter()
            .map(|path| {
                let path = path.into();
                let root_id = path_based_root_id(&path);
                let display_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                RootRegistration::user_initiated(root_id, path, purpose)
                    .with_display_name(display_name)
            })
            .collect();
        self.set_roots(roots)
    }

    pub fn scan_root_paths<I, P>(
        &mut self,
        paths: I,
        mode: ScanMode,
    ) -> ScriptMetaKitResult<ScanResult>
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.set_root_paths(paths, RootPurpose::from_scan_mode(mode))?;
        self.scan_roots(ScanRequest::all(mode))
    }

    pub fn set_visible_root(&mut self, root_id: Option<RootId>) {
        self.visible_root_id = root_id;
    }

    #[must_use]
    pub fn watch_plan(&self) -> WatchPlan {
        if !self.config.watcher.enabled {
            return self.watch_plan_with_delivery_options(WatchPlan::empty());
        }
        let plan = build_watch_plan(
            &self.roots,
            self.config.watcher.watch_policy,
            self.config.watcher.monitor_root_strategy,
            self.visible_root_id.as_ref(),
        );
        self.watch_plan_with_delivery_options(plan)
    }

    #[must_use]
    pub fn snapshot(&self, root_id: &RootId) -> Option<Arc<FileListSnapshot>> {
        self.file_list_snapshots.get(root_id).cloned()
    }

    #[must_use]
    pub fn snapshot_ref(&self, root_id: &RootId) -> Option<&FileListSnapshot> {
        self.file_list_snapshots.get(root_id).map(Arc::as_ref)
    }

    #[must_use]
    pub fn catalog_snapshot(&self) -> Option<Arc<ScriptMetaCatalogSnapshot>> {
        self.catalog_snapshot.clone()
    }

    #[must_use]
    pub fn catalog_snapshot_ref(&self) -> Option<&ScriptMetaCatalogSnapshot> {
        self.catalog_snapshot.as_deref()
    }

    #[must_use]
    pub fn update_check_result(&self) -> Option<&UpdateCheckResult> {
        self.update_check_result.as_deref()
    }

    pub fn scan_roots(&mut self, request: ScanRequest) -> ScriptMetaKitResult<ScanResult> {
        self.expire_idle_memory_cache();
        let _operation_scope = self.cancellation.begin_scope();
        self.scan_roots_inner(request, None)
    }

    pub fn scan_roots_and_check_updates(
        &mut self,
        request: ScanRequest,
        check_updates: bool,
    ) -> ScriptMetaKitResult<(ScanResult, Option<Arc<UpdateCheckResult>>)> {
        self.scan_roots_and_check_updates_inner(request, check_updates, None)
    }

    pub fn scan_roots_and_check_updates_with_progress(
        &mut self,
        request: ScanRequest,
        check_updates: bool,
        mut progress: impl FnMut(UpdateCheckProgress),
    ) -> ScriptMetaKitResult<(ScanResult, Option<Arc<UpdateCheckResult>>)> {
        self.scan_roots_and_check_updates_inner(request, check_updates, Some(&mut progress))
    }

    fn scan_roots_and_check_updates_inner(
        &mut self,
        request: ScanRequest,
        check_updates: bool,
        progress: Option<&mut dyn FnMut(UpdateCheckProgress)>,
    ) -> ScriptMetaKitResult<(ScanResult, Option<Arc<UpdateCheckResult>>)> {
        self.expire_idle_memory_cache();
        let _operation_scope = self.cancellation.begin_scope();
        let scan_result = self.scan_roots_inner(request, None)?;
        if !check_updates || self.cancellation.is_cancelled() {
            return Ok((scan_result, None));
        }
        let items = scan_result
            .catalog_snapshot
            .as_ref()
            .map_or(&[][..], |snapshot| snapshot.file_items.as_slice());
        let update_result =
            self.check_updates_items(items, progress, UpdateCheckCacheMode::Replace)?;
        Ok((scan_result, Some(update_result)))
    }

    #[must_use]
    pub fn cached_scan_result(&mut self, request: ScanRequest) -> ScanResult {
        self.expire_idle_memory_cache();
        let selected_root_indices = self.selected_root_indices(&request.root_ids);
        let root_ids: Vec<_> = selected_root_indices
            .iter()
            .map(|index| self.roots[*index].root_id.clone())
            .collect();
        let mut roots = self.snapshots_for_roots(&root_ids);
        let file_list_snapshots = if request.mode.includes_file_list() {
            self.file_list_snapshots_for_roots(&root_ids)
        } else {
            Vec::new()
        };
        let catalog_snapshot = request
            .mode
            .includes_metadata()
            .then(|| self.catalog_snapshot.clone())
            .flatten();
        for root in &mut roots {
            let has_file_list = !request.mode.includes_file_list()
                || file_list_snapshots
                    .iter()
                    .any(|snapshot| snapshot.root.root_id == root.root_id);
            let has_catalog = !request.mode.includes_metadata()
                || catalog_snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot
                        .roots
                        .iter()
                        .any(|catalog_root| catalog_root.root_id == root.root_id)
                });
            if root.status == RootStatus::Ready && (!has_file_list || !has_catalog) {
                root.status = RootStatus::NotLoaded;
                root.is_dirty = true;
                root.item_count = 0;
                root.error = None;
            }
        }
        self.touch_memory_cache_if_needed();
        let operation = scan_operation_summary(&roots);
        let file_issues =
            collect_scan_file_issues(&roots, &file_list_snapshots, catalog_snapshot.as_deref());

        ScanResult {
            roots,
            file_list_snapshots,
            catalog_snapshot,
            operation,
            file_issues,
            update_check_result: request
                .mode
                .includes_metadata()
                .then(|| self.update_check_result.clone())
                .flatten(),
            change_summary: None,
            watch_change_batch: None,
        }
    }

    pub fn cancel_current_operation(&self) {
        self.cancellation.cancel();
    }

    #[must_use]
    pub fn cancellation_token(&self) -> OperationCancellation {
        self.cancellation.clone()
    }

    fn scan_roots_inner(
        &mut self,
        request: ScanRequest,
        dirty_scopes: Option<&BTreeMap<RootId, DirtyRootState>>,
    ) -> ScriptMetaKitResult<ScanResult> {
        let selected_root_indices = self.selected_root_indices(&request.root_ids);
        let file_list_root_indices = self
            .selected_root_indices_for_mode(&selected_root_indices, |purpose| {
                purpose.includes_file_list()
            });
        let metadata_root_indices = self
            .selected_root_indices_for_mode(&selected_root_indices, |purpose| {
                purpose.includes_metadata()
            });
        let root_ids: Vec<_> = selected_root_indices
            .iter()
            .map(|index| self.roots[*index].root_id.clone())
            .collect();
        let mut file_list_snapshots = Vec::new();
        let mut saw_previous_file_list = false;
        let mut previous_file_list_snapshots = BTreeMap::new();
        let mut change_summary = ScanChangeSummary::default();

        if request.mode.includes_file_list() {
            let defer_script_probe_to_metadata =
                request.mode.includes_metadata() && dirty_scopes.is_none();
            for output in self.scan_file_list_outputs(
                &file_list_root_indices,
                dirty_scopes,
                defer_script_probe_to_metadata,
            ) {
                let output_change_summary = output.change_summary;
                let root_snapshot = output.root;
                let root_id = root_snapshot.root_id.clone();
                let previous_snapshot = self.file_list_snapshots.get(&root_id).cloned();
                let snapshot = FileListSnapshot {
                    root: root_snapshot.clone(),
                    children: Some(output.children),
                    directory_states: output.directory_states,
                    truncated: output.truncated,
                };
                if let Some(summary) = output_change_summary {
                    saw_previous_file_list = true;
                    change_summary.extend(summary);
                } else if let Some(previous_snapshot) = previous_snapshot {
                    saw_previous_file_list = true;
                    previous_file_list_snapshots.insert(root_id.clone(), previous_snapshot);
                }
                self.root_snapshots.insert(root_id.clone(), root_snapshot);
                file_list_snapshots.push(snapshot);
            }
        }

        let catalog_snapshot = if request.mode.includes_metadata() {
            let output = {
                let previous_cache = self
                    .memory_cache_enabled()
                    .then(|| {
                        self.catalog_snapshot
                            .as_ref()
                            .map(|snapshot| &snapshot.candidate_cache)
                    })
                    .flatten();
                let dirty_directories = self.metadata_dirty_directories(dirty_scopes);
                let shared_file_list_snapshots = request
                    .mode
                    .includes_file_list()
                    .then_some(file_list_snapshots.as_slice());
                scan_metadata_roots_scoped_with_file_lists_controlled(
                    metadata_root_indices
                        .iter()
                        .map(|index| &self.roots[*index]),
                    &self.config.scanner,
                    &self.config.parser,
                    &self.config.supported_extensions,
                    MetadataScanSources {
                        previous_cache,
                        dirty_directories_by_root: dirty_directories.as_ref(),
                        file_list_snapshots: shared_file_list_snapshots,
                    },
                    Some(&self.cancellation),
                )
            };
            for root in &output.roots {
                self.root_snapshots
                    .insert(root.root_id.clone(), root.clone());
            }
            let refreshed_root_ids = metadata_root_indices
                .iter()
                .map(|index| self.roots[*index].root_id.clone())
                .collect::<BTreeSet<_>>();
            let scan_roots = output.roots;
            let scan_candidate_cache = output.candidate_cache;
            let previous_snapshot = self.catalog_snapshot.as_ref();
            let needs_partial_merge = previous_snapshot.is_some_and(|previous| {
                previous
                    .roots
                    .iter()
                    .any(|root| !refreshed_root_ids.contains(&root.root_id))
                    || previous.candidate_cache.records.iter().any(|record| {
                        !refreshed_root_ids.contains(&record.root_id)
                            && self
                                .root_by_id(&record.root_id)
                                .is_some_and(root_allows_memory_cache)
                    })
            });
            let (stored_roots, stored_candidate_cache, partial_snapshot_parts) =
                if needs_partial_merge {
                    let scan_file_items = file_items_from_cache(&scan_candidate_cache);
                    let scan_all_items = deduplicated_items(&scan_file_items);
                    let previous = previous_snapshot.expect("partial merge requires a snapshot");
                    (
                        merged_catalog_roots(
                            previous.as_ref(),
                            scan_roots.clone(),
                            &refreshed_root_ids,
                        ),
                        merged_candidate_cache(
                            previous.as_ref(),
                            scan_candidate_cache.clone(),
                            &refreshed_root_ids,
                            &self.roots,
                            root_allows_memory_cache,
                        ),
                        Some((
                            scan_roots,
                            scan_all_items,
                            scan_file_items,
                            scan_candidate_cache,
                        )),
                    )
                } else {
                    (scan_roots, scan_candidate_cache, None)
                };
            let stored_file_items = file_items_from_cache(&stored_candidate_cache);
            let stored_all_items = deduplicated_items(&stored_file_items);
            let update_check_result = self.preserved_update_result(
                self.catalog_snapshot.as_deref(),
                self.update_check_result.as_deref(),
                &stored_file_items,
            );
            let stored_snapshot = Arc::new(ScriptMetaCatalogSnapshot {
                source_revision: output.source_revision,
                roots: stored_roots,
                all_items: stored_all_items,
                file_items: stored_file_items,
                candidate_cache: stored_candidate_cache,
            });
            let snapshot = if let Some((roots, all_items, file_items, candidate_cache)) =
                partial_snapshot_parts
            {
                Arc::new(ScriptMetaCatalogSnapshot {
                    source_revision: output.source_revision,
                    roots,
                    all_items,
                    file_items,
                    candidate_cache,
                })
            } else {
                Arc::clone(&stored_snapshot)
            };
            if request.mode.includes_file_list() {
                apply_metadata_capabilities_to_file_list_snapshots(
                    &mut file_list_snapshots,
                    &stored_snapshot.candidate_cache.records,
                );
            }
            self.store_catalog_snapshot_if_allowed(&stored_snapshot, update_check_result);
            Some(snapshot)
        } else {
            None
        };

        if request.mode.includes_file_list()
            && !request.mode.includes_metadata()
            && let Some(snapshot) = self.catalog_snapshot.as_ref()
        {
            apply_metadata_capabilities_to_file_list_snapshots(
                &mut file_list_snapshots,
                &snapshot.candidate_cache.records,
            );
        }

        for snapshot in &file_list_snapshots {
            if let Some(previous_snapshot) =
                previous_file_list_snapshots.get(&snapshot.root.root_id)
            {
                change_summary.extend(diff_file_list_snapshot(
                    &snapshot.root.root_id,
                    previous_snapshot.as_ref(),
                    snapshot,
                ));
            }
        }
        let file_list_snapshots = if request.mode.includes_file_list() {
            file_list_snapshots
                .into_iter()
                .map(|snapshot| {
                    let root_id = snapshot.root.root_id.clone();
                    let snapshot = Arc::new(snapshot);
                    if self.should_store_file_list_snapshot(&root_id) {
                        self.file_list_snapshots
                            .insert(root_id, Arc::clone(&snapshot));
                    } else {
                        self.file_list_snapshots.remove(&root_id);
                    }
                    snapshot
                })
                .collect()
        } else {
            Vec::new()
        };
        self.rebuild_known_path_index();
        self.enforce_memory_node_limit();
        for root_id in &root_ids {
            let completed = self
                .root_snapshots
                .get(root_id)
                .is_some_and(|root| matches!(root.status, RootStatus::Ready | RootStatus::Missing));
            if completed {
                self.dirty_roots.remove(root_id);
            } else if self.dirty_roots.contains_key(root_id)
                && let Some(snapshot) = self.root_snapshots.get_mut(root_id)
            {
                snapshot.is_dirty = true;
            }
        }
        self.touch_memory_cache_if_needed();

        let roots = self.snapshots_for_roots(&root_ids);
        let operation = scan_operation_summary(&roots);
        let file_issues =
            collect_scan_file_issues(&roots, &file_list_snapshots, catalog_snapshot.as_deref());
        Ok(ScanResult {
            roots,
            file_list_snapshots,
            catalog_snapshot,
            operation,
            file_issues,
            update_check_result: request
                .mode
                .includes_metadata()
                .then(|| self.update_check_result.clone())
                .flatten(),
            change_summary: saw_previous_file_list.then_some(change_summary),
            watch_change_batch: None,
        })
    }

    pub fn mark_changed_paths(
        &mut self,
        batch: RawChangeBatch,
    ) -> ScriptMetaKitResult<Vec<ScriptMetaKitEvent>> {
        self.expire_idle_memory_cache();
        let known_directory_paths = self.known_directory_paths();
        let known_file_paths = self.known_file_paths();
        let mut change_batch = route_change_batch(
            &self.roots,
            batch,
            ChangeRoutingOptions {
                extensions: &self.config.supported_extensions,
                skip_hidden_paths: self.config.scanner.skip_hidden,
                skip_package_paths: self.config.scanner.skip_packages,
                known_directory_paths,
                known_file_paths,
                overflow_policy: self.config.watcher.overflow_policy,
                max_deferred_dirty_directories: self.config.cache.max_deferred_dirty_directories,
            },
        );
        let visible_root_id = self.visible_root_id.as_ref();
        change_batch.affected_roots.retain(|change| {
            self.root_by_id(&change.root_id)
                .is_some_and(|root| should_mark_dirty_for_file_event(root, visible_root_id))
        });
        if change_batch.affected_roots.is_empty() {
            if !change_batch.events.is_empty()
                || !change_batch.ignored_paths.is_empty()
                || !change_batch.rename_candidates.is_empty()
            {
                return Ok(vec![ScriptMetaKitEvent::ChangeDetected {
                    batch: change_batch,
                }]);
            }
            return Ok(Vec::new());
        }

        let now = now_timestamp_millis();
        let mut root_events = Vec::with_capacity(change_batch.affected_roots.len() + 1);

        for change in &change_batch.affected_roots {
            let root_path = self
                .root_by_id(&change.root_id)
                .map(|root| normalize_path(&root.path));
            let dirty_state = self.dirty_roots.entry(change.root_id.clone()).or_default();
            if change.requires_full_rescan {
                dirty_state.requires_full_rescan = true;
                dirty_state.dirty_directories.clear();
            }
            if dirty_state.requires_full_rescan {
                if let Some(root_path) = root_path {
                    dirty_state.dirty_directories.insert(root_path);
                }
            } else {
                dirty_state.dirty_directories.extend(
                    change
                        .dirty_directories
                        .iter()
                        .map(|path| normalize_path(path)),
                );
            }
            if let Some(snapshot) = self.root_snapshots.get_mut(&change.root_id) {
                snapshot.is_dirty = true;
                snapshot.last_event_at = Some(now);
                snapshot.status = if change.requires_full_rescan {
                    RootStatus::Overflowed
                } else {
                    RootStatus::Dirty
                };
            }
            root_events.push(ScriptMetaKitEvent::RootMarkedDirty {
                root_id: change.root_id.clone(),
            });
        }

        if change_batch.overflowed {
            root_events.push(ScriptMetaKitEvent::WatchOverflowed {
                affected_roots: change_batch
                    .affected_roots
                    .iter()
                    .map(|change| change.root_id.clone())
                    .collect(),
            });
        }

        let mut events = Vec::with_capacity(root_events.len() + 1);
        events.push(ScriptMetaKitEvent::ChangeDetected {
            batch: change_batch,
        });
        events.extend(root_events);
        Ok(events)
    }

    pub fn refresh_dirty_roots(
        &mut self,
        request: RefreshRequest,
    ) -> ScriptMetaKitResult<ScanResult> {
        self.expire_idle_memory_cache();
        let _operation_scope = self.cancellation.begin_scope();
        let dirty_scopes = self.dirty_roots.clone();
        let dirty_root_ids = dirty_scopes.keys().cloned().collect::<Vec<_>>();
        let all_root_ids: Vec<_> = self.roots.iter().map(|root| root.root_id.clone()).collect();
        if dirty_root_ids.is_empty() {
            let roots = self.snapshots_for_roots(&all_root_ids);
            let file_list_snapshots = self.file_list_snapshots_for_roots(&all_root_ids);
            let catalog_snapshot = request
                .mode
                .includes_metadata()
                .then(|| self.catalog_snapshot.clone())
                .flatten();
            let operation = scan_operation_summary(&roots);
            let file_issues =
                collect_scan_file_issues(&roots, &file_list_snapshots, catalog_snapshot.as_deref());
            return Ok(ScanResult {
                roots,
                file_list_snapshots,
                catalog_snapshot,
                operation,
                file_issues,
                update_check_result: request
                    .mode
                    .includes_metadata()
                    .then(|| self.update_check_result.clone())
                    .flatten(),
                change_summary: None,
                watch_change_batch: None,
            });
        }

        let partial_result = self.scan_roots_inner(
            ScanRequest {
                root_ids: dirty_root_ids,
                mode: request.mode,
            },
            Some(&dirty_scopes),
        )?;

        let roots = self.snapshots_for_roots(&all_root_ids);
        let file_list_snapshots = self.file_list_snapshots_for_roots(&all_root_ids);
        let catalog_snapshot = request
            .mode
            .includes_metadata()
            .then(|| self.catalog_snapshot.clone())
            .flatten();
        let operation = scan_operation_summary(&roots);
        let file_issues =
            collect_scan_file_issues(&roots, &file_list_snapshots, catalog_snapshot.as_deref());
        Ok(ScanResult {
            roots,
            file_list_snapshots,
            catalog_snapshot,
            operation,
            file_issues,
            update_check_result: request
                .mode
                .includes_metadata()
                .then(|| self.update_check_result.clone())
                .flatten(),
            change_summary: partial_result.change_summary,
            watch_change_batch: None,
        })
    }

    pub async fn check_updates(
        &mut self,
        request: UpdateCheckRequest,
    ) -> ScriptMetaKitResult<Arc<UpdateCheckResult>> {
        let _operation_scope = self.cancellation.begin_scope();
        self.check_updates_items(&request.items, None, UpdateCheckCacheMode::Replace)
    }

    pub async fn check_updates_with_progress(
        &mut self,
        request: UpdateCheckRequest,
        mut progress: impl FnMut(UpdateCheckProgress),
    ) -> ScriptMetaKitResult<Arc<UpdateCheckResult>> {
        let _operation_scope = self.cancellation.begin_scope();
        self.check_updates_items(
            &request.items,
            Some(&mut progress),
            UpdateCheckCacheMode::Replace,
        )
    }

    pub async fn check_updates_for_items(
        &mut self,
        items: &[ScriptMetaItemRef],
    ) -> ScriptMetaKitResult<Arc<UpdateCheckResult>> {
        let _operation_scope = self.cancellation.begin_scope();
        self.check_updates_items(items, None, UpdateCheckCacheMode::Replace)
    }

    pub async fn check_updates_for_items_with_progress(
        &mut self,
        items: &[ScriptMetaItemRef],
        mut progress: impl FnMut(UpdateCheckProgress),
    ) -> ScriptMetaKitResult<Arc<UpdateCheckResult>> {
        let _operation_scope = self.cancellation.begin_scope();
        self.check_updates_items(items, Some(&mut progress), UpdateCheckCacheMode::Replace)
    }

    pub async fn check_update_for_item(
        &mut self,
        item: ScriptMetaItemRef,
    ) -> ScriptMetaKitResult<Arc<UpdateCheckResult>> {
        let _operation_scope = self.cancellation.begin_scope();
        self.check_updates_items(&[item], None, UpdateCheckCacheMode::Merge)
    }

    pub async fn check_update_for_item_with_progress(
        &mut self,
        item: ScriptMetaItemRef,
        mut progress: impl FnMut(UpdateCheckProgress),
    ) -> ScriptMetaKitResult<Arc<UpdateCheckResult>> {
        let _operation_scope = self.cancellation.begin_scope();
        self.check_updates_items(&[item], Some(&mut progress), UpdateCheckCacheMode::Merge)
    }

    fn check_updates_items(
        &mut self,
        items: &[ScriptMetaItemRef],
        mut progress: Option<&mut dyn FnMut(UpdateCheckProgress)>,
        cache_mode: UpdateCheckCacheMode,
    ) -> ScriptMetaKitResult<Arc<UpdateCheckResult>> {
        if !self.config.update_check.enabled {
            return Err(ScriptMetaKitError::InvalidConfig(
                "update checking is disabled".to_string(),
            ));
        }
        let checked_at = now_timestamp_millis();
        let mut result = UpdateCheckResult {
            checked_at,
            operation: OperationSummary::default(),
            resolutions_by_item_id: BTreeMap::new(),
            failures_by_item_id: BTreeMap::new(),
            errors_by_item_id: BTreeMap::new(),
            statuses_by_item_id: BTreeMap::new(),
        };
        let total_items = items.len();

        let retry_attempts = self.config.update_check.retry_attempts;
        let retry_initial_delay_millis = self.config.update_check.retry_initial_delay_millis;
        let retry_backoff_multiplier = self.config.update_check.retry_backoff_multiplier;
        let max_retry_delay_millis = self.config.update_check.max_retry_delay_millis;
        let retry_policy = UpdateRetryPolicy {
            attempts: retry_attempts,
            initial_delay_millis: retry_initial_delay_millis,
            backoff_multiplier: retry_backoff_multiplier,
            max_delay_millis: max_retry_delay_millis,
        };
        let update_resolver = UpdateResolver::new_with_cancellation_retry_and_http_cache(
            DistributionResolverOptions {
                request_timeout_millis: self
                    .config
                    .update_check
                    .request_timeout_millis
                    .or(self.config.update_check.resource_timeout_millis),
                resource_timeout_millis: self.config.update_check.resource_timeout_millis,
                // Parsed/source singleflight state is operation-local. HTTP
                // validators and their bounded response bodies are supplied
                // separately by the engine for conditional revalidation.
                cache_enabled: true,
                ..DistributionResolverOptions::default()
            },
            self.cancellation.clone(),
            retry_attempts,
            self.http_validation_cache.clone(),
        )?;

        emit_update_progress(
            &mut progress,
            0,
            total_items,
            None,
            None,
            UpdateCheckProgressPhase::Started,
            || format!("Starting update check for {total_items} item(s)"),
        );

        let groups = update_work_groups(items);
        let group_parallelism = self
            .config
            .update_check
            .max_concurrent_meta_url_checks
            .max(1)
            .min(groups.len().max(1));
        let mut completed_items = 0usize;

        let next_group_index = AtomicUsize::new(0);
        let (sender, receiver) = mpsc::channel();
        thread::scope(|scope| {
            for _ in 0..group_parallelism {
                let next_group_index = &next_group_index;
                let groups = &groups;
                let sender = sender.clone();
                let update_resolver = update_resolver.clone();
                let cancellation = self.cancellation.clone();
                scope.spawn(move || {
                    loop {
                        if cancellation.is_cancelled() {
                            break;
                        }
                        let group_index = next_group_index.fetch_add(1, Ordering::Relaxed);
                        let Some(group) = groups.get(group_index) else {
                            break;
                        };
                        for &item_index in group {
                            if cancellation.is_cancelled() {
                                break;
                            }
                            let item = &items[item_index];
                            let item_id = item.item_id();
                            let script_id = item.script_id.clone();
                            let _ = sender.send(UpdateWorkEvent::Checking {
                                item_id: item_id.clone(),
                                script_id: script_id.clone(),
                            });
                            let resolved_result = resolve_update_item_with_retry(
                                &update_resolver,
                                item,
                                retry_policy,
                                &cancellation,
                                |error| {
                                    let _ = sender.send(UpdateWorkEvent::Retrying {
                                        item_id: item_id.clone(),
                                        script_id: script_id.clone(),
                                        error: error.to_string(),
                                    });
                                },
                            );
                            if cancellation.is_cancelled() {
                                break;
                            }
                            let _ = sender.send(UpdateWorkEvent::Finished {
                                item_index,
                                item_id,
                                script_id,
                                resolved_result: Box::new(resolved_result),
                            });
                        }
                    }
                });
            }
            drop(sender);

            for event in receiver {
                match event {
                    UpdateWorkEvent::Checking { item_id, script_id } => {
                        emit_update_progress(
                            &mut progress,
                            completed_items,
                            total_items,
                            Some(item_id.as_str()),
                            Some(script_id.as_str()),
                            UpdateCheckProgressPhase::Checking,
                            || format!("checking {script_id}"),
                        );
                    }
                    UpdateWorkEvent::Retrying {
                        item_id,
                        script_id,
                        error,
                    } => {
                        emit_update_progress(
                            &mut progress,
                            completed_items,
                            total_items,
                            Some(item_id.as_str()),
                            Some(script_id.as_str()),
                            UpdateCheckProgressPhase::Retrying,
                            || format!("retrying {script_id} after failure: {error}"),
                        );
                    }
                    UpdateWorkEvent::Finished {
                        item_index,
                        item_id,
                        script_id,
                        resolved_result,
                    } => {
                        let status = apply_update_item_result(
                            &mut result,
                            &items[item_index],
                            item_id.clone(),
                            checked_at,
                            *resolved_result,
                        );
                        completed_items += 1;
                        let failed = status == UpdateStatus::Failed;
                        emit_update_progress(
                            &mut progress,
                            completed_items,
                            total_items,
                            Some(item_id.as_str()),
                            Some(script_id.as_str()),
                            if failed {
                                UpdateCheckProgressPhase::FailedItem
                            } else {
                                UpdateCheckProgressPhase::FinishedItem
                            },
                            || {
                                format!(
                                    "{}/{} {} {}",
                                    completed_items,
                                    total_items,
                                    if failed { "failed" } else { "finished" },
                                    script_id
                                )
                            },
                        );
                    }
                }
            }
        });

        let was_cancelled = self.cancellation.is_cancelled();
        if was_cancelled {
            mark_unchecked_update_items_cancelled(&mut result, items, checked_at);
            completed_items = result.statuses_by_item_id.len();
        }
        let failed_items = result
            .statuses_by_item_id
            .values()
            .filter(|status| matches!(status, UpdateStatus::Failed | UpdateStatus::Cancelled))
            .count();
        result.operation = if was_cancelled {
            OperationSummary::cancelled(total_items, completed_items, failed_items)
        } else {
            OperationSummary::finished(total_items, completed_items, failed_items)
        };

        emit_update_progress(
            &mut progress,
            completed_items,
            total_items,
            None,
            None,
            if was_cancelled {
                UpdateCheckProgressPhase::Cancelled
            } else {
                UpdateCheckProgressPhase::Finished
            },
            || {
                if was_cancelled {
                    format!("Cancelled update check after {completed_items}/{total_items} item(s)")
                } else {
                    format!("Finished update check for {total_items} item(s)")
                }
            },
        );

        let result = Arc::new(result);
        if self.memory_cache_enabled() {
            self.store_update_check_result(items, Arc::clone(&result), cache_mode);
            self.touch_memory_cache_if_needed();
        }

        Ok(result)
    }

    fn store_update_check_result(
        &mut self,
        checked_items: &[ScriptMetaItemRef],
        result: Arc<UpdateCheckResult>,
        cache_mode: UpdateCheckCacheMode,
    ) {
        match cache_mode {
            UpdateCheckCacheMode::Replace => {
                self.update_check_result = Some(result);
            }
            UpdateCheckCacheMode::Merge => {
                let mut merged = self
                    .update_check_result
                    .take()
                    .map(|result| {
                        Arc::try_unwrap(result).unwrap_or_else(|shared| shared.as_ref().clone())
                    })
                    .unwrap_or_else(|| UpdateCheckResult {
                        checked_at: result.checked_at,
                        operation: OperationSummary::default(),
                        resolutions_by_item_id: BTreeMap::new(),
                        failures_by_item_id: BTreeMap::new(),
                        errors_by_item_id: BTreeMap::new(),
                        statuses_by_item_id: BTreeMap::new(),
                    });
                merged.checked_at = result.checked_at;
                for item_id in checked_items.iter().map(|item| item.item_id()) {
                    merged.resolutions_by_item_id.remove(&item_id);
                    merged.failures_by_item_id.remove(&item_id);
                    merged.errors_by_item_id.remove(&item_id);
                    merged.statuses_by_item_id.remove(&item_id);
                }
                merged
                    .resolutions_by_item_id
                    .extend(result.resolutions_by_item_id.clone());
                merged
                    .failures_by_item_id
                    .extend(result.failures_by_item_id.clone());
                merged
                    .errors_by_item_id
                    .extend(result.errors_by_item_id.clone());
                merged
                    .statuses_by_item_id
                    .extend(result.statuses_by_item_id.clone());
                self.update_check_result = Some(Arc::new(merged));
            }
        }
    }

    pub fn load_cache(
        &mut self,
        payload: CachePayload,
    ) -> ScriptMetaKitResult<Vec<ScriptMetaKitEvent>> {
        if !self.persistent_cache_enabled() {
            return Err(ScriptMetaKitError::InvalidConfig(
                "persistent cache is disabled".to_string(),
            ));
        }
        if !self.memory_cache_enabled() {
            return Err(ScriptMetaKitError::InvalidConfig(
                "memory cache is disabled; persistent cache cannot be loaded".to_string(),
            ));
        }
        let payload = payload.migrate()?;
        payload.validate_for_config(&self.config)?;
        match payload.scope {
            CacheScope::All => {
                let all_data = decode_all_cache_data(payload.data)?;
                if let Some(catalog) = all_data.catalog
                    && !catalog.is_null()
                {
                    self.load_catalog_cache_data(decode_catalog_cache_data(catalog)?)?;
                }
                self.load_file_list_cache_snapshots(all_data.file_list_snapshots)?;
                Ok(vec![ScriptMetaKitEvent::CacheLoaded {
                    scope: CacheScope::All,
                }])
            }
            CacheScope::Catalog => {
                self.load_catalog_cache_data(decode_catalog_cache_data(payload.data)?)?;
                Ok(vec![ScriptMetaKitEvent::CacheLoaded {
                    scope: CacheScope::Catalog,
                }])
            }
            CacheScope::FileList | CacheScope::Root => {
                let snapshots: BTreeMap<RootId, Arc<FileListSnapshot>> =
                    serde_json::from_value(payload.data)
                        .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
                self.load_file_list_cache_snapshots(snapshots)?;
                Ok(vec![ScriptMetaKitEvent::CacheLoaded {
                    scope: payload.scope,
                }])
            }
        }
    }

    pub fn export_cache(&self, scope: CacheScope) -> ScriptMetaKitResult<CachePayload> {
        if !self.persistent_cache_enabled() {
            return Err(ScriptMetaKitError::InvalidConfig(
                "persistent cache is disabled".to_string(),
            ));
        }
        if !self.memory_cache_export_allowed {
            return Err(ScriptMetaKitError::Cache(
                "memory cache was partially evicted; refusing to overwrite persistent cache"
                    .to_string(),
            ));
        }
        match scope {
            CacheScope::All => {
                let data = serde_json::to_value(AllCacheDataRef {
                    catalog: self.catalog_cache_data()?,
                    file_list_snapshots: &self.persistent_file_list_cache_snapshots(),
                })
                .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
                Ok(CachePayload::new_for_config(
                    CacheScope::All,
                    data,
                    &self.config,
                ))
            }
            CacheScope::Catalog => {
                let data = self.catalog_cache_data()?;
                Ok(CachePayload::new_for_config(
                    CacheScope::Catalog,
                    data,
                    &self.config,
                ))
            }
            CacheScope::FileList | CacheScope::Root => {
                let snapshots = self.persistent_file_list_cache_snapshots();
                let data = serde_json::to_value(&snapshots)
                    .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
                Ok(CachePayload::new_for_config(scope, data, &self.config))
            }
        }
    }

    fn load_catalog_cache_data(&mut self, data: CatalogCacheData) -> ScriptMetaKitResult<()> {
        let CatalogCacheData {
            mut catalog_snapshot,
            update_check_result,
        } = data;
        validate_catalog_cache_roots(
            &catalog_snapshot.candidate_cache,
            &self.roots,
            root_allows_persistent_catalog_cache,
        )?;
        let file_items = file_items_from_cache(&catalog_snapshot.candidate_cache);
        catalog_snapshot.all_items = deduplicated_items(&file_items);
        catalog_snapshot.file_items = file_items;
        let catalog_snapshot =
            self.catalog_snapshot_for_policy(&catalog_snapshot, root_allows_memory_cache);
        self.update_check_result = update_check_result
            .as_ref()
            .and_then(|result| filter_update_result_to_items(result, &catalog_snapshot.file_items));
        self.catalog_snapshot = Some(Arc::new(catalog_snapshot));
        self.enforce_memory_node_limit();
        self.rebuild_known_path_index();
        self.touch_memory_cache_if_needed();
        Ok(())
    }

    fn load_file_list_cache_snapshots(
        &mut self,
        snapshots: BTreeMap<RootId, Arc<FileListSnapshot>>,
    ) -> ScriptMetaKitResult<()> {
        for (root_id, snapshot) in snapshots {
            let Some(root) = self.root_by_id(&root_id) else {
                continue;
            };
            if !root_allows_persistent_file_list_cache(root)
                || normalize_path(&snapshot.root.path) != normalize_path(&root.path)
            {
                continue;
            }
            if self.should_store_file_list_snapshot(&root_id) {
                self.root_snapshots
                    .insert(root_id.clone(), snapshot.root.clone());
                self.file_list_snapshots.insert(root_id, snapshot);
            }
        }
        self.rebuild_known_path_index();
        self.enforce_memory_node_limit();
        self.touch_memory_cache_if_needed();
        Ok(())
    }

    fn catalog_cache_data(&self) -> ScriptMetaKitResult<serde_json::Value> {
        let data = if let Some(snapshot) = self.catalog_snapshot.as_ref() {
            let snapshot = self.catalog_snapshot_for_policy(
                snapshot.as_ref(),
                root_allows_persistent_catalog_cache,
            );
            let update_check_result = self
                .update_check_result
                .as_ref()
                .and_then(|result| filter_update_result_to_items(result, &snapshot.file_items));
            serde_json::to_value(CatalogCacheDataRef {
                catalog_snapshot: &snapshot,
                update_check_result: update_check_result.as_deref(),
            })
        } else {
            let persistent_roots = self
                .roots
                .iter()
                .filter(|root| root_allows_persistent_catalog_cache(root))
                .collect::<Vec<_>>();
            let snapshot = ScriptMetaCatalogSnapshot {
                source_revision: Uuid::new_v4(),
                roots: self
                    .root_snapshots
                    .values()
                    .filter(|snapshot| {
                        persistent_roots
                            .iter()
                            .any(|root| root.root_id == snapshot.root_id)
                    })
                    .cloned()
                    .collect(),
                all_items: Vec::new(),
                file_items: Vec::new(),
                candidate_cache: CandidateCache {
                    schema_version: CandidateCache::CURRENT_SCHEMA_VERSION,
                    built_at: now_timestamp_millis(),
                    registered_roots: registered_root_signatures(&persistent_roots),
                    records: Vec::new(),
                },
            };
            serde_json::to_value(CatalogCacheDataRef {
                catalog_snapshot: &snapshot,
                update_check_result: None,
            })
        }
        .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
        Ok(data)
    }

    fn persistent_file_list_cache_snapshots(&self) -> BTreeMap<RootId, Arc<FileListSnapshot>> {
        self.file_list_snapshots
            .iter()
            .filter(|(root_id, _)| {
                self.root_by_id(root_id)
                    .is_some_and(root_allows_persistent_file_list_cache)
            })
            .map(|(root_id, snapshot)| (root_id.clone(), Arc::clone(snapshot)))
            .collect::<BTreeMap<_, _>>()
    }

    pub fn invalidate_cache(
        &mut self,
        scope: CacheScope,
        reason: CacheInvalidationReason,
    ) -> Vec<ScriptMetaKitEvent> {
        match scope {
            CacheScope::All => {
                self.catalog_snapshot = None;
                self.update_check_result = None;
                self.file_list_snapshots.clear();
            }
            CacheScope::Catalog => {
                self.catalog_snapshot = None;
                self.update_check_result = None;
            }
            CacheScope::FileList | CacheScope::Root => self.file_list_snapshots.clear(),
        }
        self.rebuild_known_path_index();
        vec![ScriptMetaKitEvent::CacheInvalidated { scope, reason }]
    }

    fn scan_file_list_outputs(
        &mut self,
        selected_root_indices: &[usize],
        dirty_scopes: Option<&BTreeMap<RootId, DirtyRootState>>,
        defer_script_probe_to_metadata: bool,
    ) -> Vec<crate::scanner::DirectoryScanOutput> {
        if selected_root_indices.is_empty() {
            return Vec::new();
        }
        let roots: Vec<_> = selected_root_indices
            .iter()
            .map(|index| self.roots[*index].clone())
            .collect();
        let parallelism = bounded_parallelism(roots.len());
        let mut outputs_by_index = vec![None; roots.len()];
        let mut jobs = Vec::new();

        for (index, root) in roots.into_iter().enumerate() {
            let probe_script_headers =
                !(defer_script_probe_to_metadata && root.purpose.includes_metadata());
            let dirty_directories = dirty_scopes
                .and_then(|scopes| scopes.get(&root.root_id))
                .filter(|dirty_scope| {
                    !dirty_scope.requires_full_rescan && !dirty_scope.dirty_directories.is_empty()
                })
                .map(|dirty_scope| {
                    dirty_scope
                        .dirty_directories
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                });

            if let Some(dirty_directories) = dirty_directories.as_ref()
                && let Some(previous_snapshot) = self.file_list_snapshots.remove(&root.root_id)
            {
                match Arc::try_unwrap(previous_snapshot) {
                    Ok(previous_snapshot) => {
                        match try_scan_file_list_root_with_owned_dirty_directories_controlled(
                            &root.path,
                            &self.config.scanner,
                            &self.config.supported_extensions,
                            previous_snapshot,
                            dirty_directories,
                            Some(&self.cancellation),
                        ) {
                            Ok(output) => {
                                outputs_by_index[index] = Some(output);
                                continue;
                            }
                            Err(previous_snapshot) => {
                                let previous_snapshot = Arc::new(*previous_snapshot);
                                self.file_list_snapshots
                                    .insert(root.root_id.clone(), Arc::clone(&previous_snapshot));
                                jobs.push(FileListScanJob {
                                    index,
                                    root,
                                    previous_snapshot: Some(previous_snapshot),
                                    dirty_directories: None,
                                    probe_script_headers,
                                });
                                continue;
                            }
                        }
                    }
                    Err(previous_snapshot) => {
                        self.file_list_snapshots
                            .insert(root.root_id.clone(), Arc::clone(&previous_snapshot));
                        jobs.push(FileListScanJob {
                            index,
                            root,
                            previous_snapshot: Some(previous_snapshot),
                            dirty_directories: Some(dirty_directories.clone()),
                            probe_script_headers,
                        });
                        continue;
                    }
                }
            }

            let previous_snapshot = self.file_list_snapshots.get(&root.root_id).cloned();
            jobs.push(FileListScanJob {
                index,
                root,
                previous_snapshot,
                dirty_directories,
                probe_script_headers,
            });
        }

        if parallelism <= 1 {
            for job in jobs {
                let index = job.index;
                outputs_by_index[index] = Some(scan_file_list_job(
                    job,
                    &self.config.scanner,
                    &self.config.supported_extensions,
                    self.cancellation.clone(),
                ));
            }
            return outputs_by_index.into_iter().flatten().collect::<Vec<_>>();
        }

        let next_job_index = AtomicUsize::new(0);
        let (sender, receiver) = mpsc::channel();
        thread::scope(|scope| {
            for _ in 0..parallelism.min(jobs.len()) {
                let next_job_index = &next_job_index;
                let jobs = &jobs;
                let sender = sender.clone();
                let scanner_options = &self.config.scanner;
                let extensions = &self.config.supported_extensions;
                let cancellation = self.cancellation.clone();
                scope.spawn(move || {
                    loop {
                        let job_index = next_job_index.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = jobs.get(job_index).cloned() else {
                            break;
                        };
                        let index = job.index;
                        let output = scan_file_list_job(
                            job,
                            scanner_options,
                            extensions,
                            cancellation.clone(),
                        );
                        let _ = sender.send((index, output));
                    }
                });
            }
            drop(sender);

            for (index, output) in receiver {
                outputs_by_index[index] = Some(output);
            }
        });
        outputs_by_index.into_iter().flatten().collect::<Vec<_>>()
    }

    fn selected_root_indices(&self, root_ids: &[RootId]) -> Vec<usize> {
        if root_ids.is_empty() {
            return (0..self.roots.len()).collect();
        }

        let requested: BTreeSet<_> = root_ids.iter().map(|root_id| root_id.as_ref()).collect();
        self.roots
            .iter()
            .enumerate()
            .filter(|(_, root)| requested.contains(root.root_id.as_ref()))
            .map(|(index, _)| index)
            .collect()
    }

    fn selected_root_indices_for_mode(
        &self,
        selected_root_indices: &[usize],
        includes_mode: impl Fn(RootPurpose) -> bool,
    ) -> Vec<usize> {
        selected_root_indices
            .iter()
            .copied()
            .filter(|index| includes_mode(self.roots[*index].purpose))
            .collect()
    }

    fn root_by_id(&self, root_id: &RootId) -> Option<&RootRegistration> {
        self.roots.iter().find(|root| root.root_id == *root_id)
    }

    fn snapshots_for_roots(&self, root_ids: &[RootId]) -> Vec<RootSnapshot> {
        root_ids
            .iter()
            .filter_map(|root_id| self.root_snapshots.get(root_id).cloned())
            .collect()
    }

    fn file_list_snapshots_for_roots(&self, root_ids: &[RootId]) -> Vec<Arc<FileListSnapshot>> {
        root_ids
            .iter()
            .filter_map(|root_id| self.file_list_snapshots.get(root_id).cloned())
            .collect()
    }

    fn watch_plan_with_delivery_options(&self, mut plan: WatchPlan) -> WatchPlan {
        plan.debounce_delay_millis = self.config.watcher.debounce_delay_millis;
        plan.max_delivery_delay_millis = self.config.watcher.max_delivery_delay_millis;
        plan.native_event_latency_millis = self.config.watcher.native_event_latency_millis;
        plan.max_pending_paths = self.config.watcher.max_pending_paths;
        plan.supported_extensions = self.config.supported_extensions.clone();
        plan.skip_hidden_paths = self.config.scanner.skip_hidden;
        plan.skip_package_paths = self.config.scanner.skip_packages;
        plan
    }

    fn rebuild_known_path_index(&mut self) {
        let mut directories: BTreeSet<PathBuf> = self
            .roots
            .iter()
            .map(|root| normalize_path(&root.path))
            .collect();
        let mut files = BTreeSet::new();
        for snapshot in self.file_list_snapshots.values() {
            let children = snapshot.children.as_deref().unwrap_or_default();
            collect_directory_paths(children, &mut directories);
            collect_file_paths(children, &mut files);
        }
        self.known_directory_paths = directories;
        self.known_file_paths = files;
    }

    fn known_directory_paths(&self) -> &BTreeSet<PathBuf> {
        &self.known_directory_paths
    }

    fn known_file_paths(&self) -> &BTreeSet<PathBuf> {
        &self.known_file_paths
    }

    fn memory_cache_enabled(&self) -> bool {
        self.config.cache.enabled && self.config.cache.memory_cache
    }

    fn persistent_cache_enabled(&self) -> bool {
        self.config.cache.enabled && self.config.cache.persistent_cache
    }

    fn should_store_file_list_snapshot(&self, root_id: &RootId) -> bool {
        self.memory_cache_enabled()
            && self
                .root_by_id(root_id)
                .is_some_and(root_allows_memory_cache)
    }

    fn store_catalog_snapshot_if_allowed(
        &mut self,
        snapshot: &Arc<ScriptMetaCatalogSnapshot>,
        update_check_result: Option<Arc<UpdateCheckResult>>,
    ) {
        if !self.memory_cache_enabled() {
            self.catalog_snapshot = None;
            self.update_check_result = None;
            self.last_memory_cache_accessed_at = None;
            return;
        }

        if self.catalog_snapshot_matches_policy(snapshot.as_ref(), root_allows_memory_cache) {
            self.update_check_result = update_check_result
                .as_ref()
                .and_then(|result| filter_update_result_to_items(result, &snapshot.file_items));
            self.catalog_snapshot = Some(Arc::clone(snapshot));
        } else {
            let snapshot =
                self.catalog_snapshot_for_policy(snapshot.as_ref(), root_allows_memory_cache);
            self.update_check_result = update_check_result
                .as_ref()
                .and_then(|result| filter_update_result_to_items(result, &snapshot.file_items));
            self.catalog_snapshot = Some(Arc::new(snapshot));
        }
        self.enforce_memory_node_limit();
        self.touch_memory_cache_if_needed();
    }

    fn catalog_snapshot_matches_policy(
        &self,
        snapshot: &ScriptMetaCatalogSnapshot,
        allows_root: impl Fn(&RootRegistration) -> bool,
    ) -> bool {
        let allowed_root_ids = self
            .roots
            .iter()
            .filter(|root| allows_root(root))
            .map(|root| root.root_id.as_ref())
            .collect::<BTreeSet<_>>();
        snapshot
            .candidate_cache
            .records
            .iter()
            .all(|record| allowed_root_ids.contains(record.root_id.as_ref()))
            && snapshot
                .roots
                .iter()
                .all(|root| allowed_root_ids.contains(root.root_id.as_ref()))
    }

    fn catalog_snapshot_for_policy(
        &self,
        snapshot: &ScriptMetaCatalogSnapshot,
        allows_root: impl Fn(&RootRegistration) -> bool,
    ) -> ScriptMetaCatalogSnapshot {
        let allowed_root_ids: BTreeSet<_> = self
            .roots
            .iter()
            .filter(|root| allows_root(root))
            .map(|root| root.root_id.as_ref())
            .collect();
        let allowed_roots: Vec<_> = self.roots.iter().filter(|root| allows_root(root)).collect();
        let records = snapshot
            .candidate_cache
            .records
            .iter()
            .filter(|record| allowed_root_ids.contains(record.root_id.as_ref()))
            .cloned()
            .collect::<Vec<_>>();
        let candidate_cache = CandidateCache {
            schema_version: CandidateCache::CURRENT_SCHEMA_VERSION,
            built_at: snapshot.candidate_cache.built_at,
            registered_roots: registered_root_signatures(&allowed_roots),
            records,
        };
        let file_items = file_items_from_cache(&candidate_cache);
        let all_items = deduplicated_items(&file_items);
        ScriptMetaCatalogSnapshot {
            source_revision: snapshot.source_revision,
            roots: snapshot
                .roots
                .iter()
                .filter(|root| allowed_root_ids.contains(root.root_id.as_ref()))
                .cloned()
                .collect(),
            all_items,
            file_items,
            candidate_cache,
        }
    }

    fn metadata_dirty_directories(
        &self,
        dirty_scopes: Option<&BTreeMap<RootId, DirtyRootState>>,
    ) -> Option<BTreeMap<RootId, Vec<PathBuf>>> {
        let dirty_scopes = dirty_scopes?;
        let dirty_directories = dirty_scopes
            .iter()
            .filter(|(_, dirty)| !dirty.requires_full_rescan && !dirty.dirty_directories.is_empty())
            .map(|(root_id, dirty)| {
                (
                    root_id.clone(),
                    dirty.dirty_directories.iter().cloned().collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        (!dirty_directories.is_empty()).then_some(dirty_directories)
    }

    fn enforce_memory_node_limit(&mut self) {
        if !self.memory_cache_enabled() {
            self.clear_memory_cache();
            return;
        }
        let max_nodes = self.config.cache.max_memory_nodes;
        while max_nodes == 0 || self.memory_node_count() > max_nodes {
            let Some(root_id) = self.largest_memory_cache_root() else {
                break;
            };
            self.evict_memory_cache_root(&root_id);
            self.memory_cache_export_allowed = false;
        }
    }

    fn largest_memory_cache_root(&self) -> Option<RootId> {
        let mut counts = BTreeMap::<RootId, usize>::new();
        for (root_id, snapshot) in &self.file_list_snapshots {
            counts.insert(
                root_id.clone(),
                snapshot.children.as_deref().map_or(0, file_entry_count),
            );
        }
        if let Some(snapshot) = self.catalog_snapshot.as_ref() {
            for record in &snapshot.candidate_cache.records {
                *counts.entry(record.root_id.clone()).or_default() += 1;
            }
        }
        counts
            .into_iter()
            .max_by(|(lhs_id, lhs_count), (rhs_id, rhs_count)| {
                lhs_count.cmp(rhs_count).then_with(|| rhs_id.cmp(lhs_id))
            })
            .map(|(root_id, _)| root_id)
    }

    fn evict_memory_cache_root(&mut self, root_id: &RootId) {
        self.file_list_snapshots.remove(root_id);
        if let Some(previous) = self.catalog_snapshot.as_ref() {
            let mut candidate_cache = previous.candidate_cache.clone();
            candidate_cache
                .records
                .retain(|record| record.root_id != *root_id);
            let roots = previous
                .roots
                .iter()
                .filter(|root| root.root_id != *root_id)
                .cloned()
                .collect::<Vec<_>>();
            let file_items = file_items_from_cache(&candidate_cache);
            let all_items = deduplicated_items(&file_items);
            let update_check_result = self
                .update_check_result
                .as_ref()
                .and_then(|result| filter_update_result_to_items(result, &file_items));
            self.catalog_snapshot = (!roots.is_empty() || !candidate_cache.records.is_empty())
                .then(|| {
                    Arc::new(ScriptMetaCatalogSnapshot {
                        source_revision: previous.source_revision,
                        roots,
                        all_items,
                        file_items,
                        candidate_cache,
                    })
                });
            self.update_check_result = update_check_result;
        }
        self.dirty_roots
            .entry(root_id.clone())
            .or_default()
            .requires_full_rescan = true;
        self.rebuild_known_path_index();
    }

    fn memory_node_count(&self) -> usize {
        self.file_list_snapshots
            .values()
            .map(|snapshot| snapshot.children.as_deref().map_or(0, file_entry_count))
            .sum::<usize>()
            .saturating_add(
                self.catalog_snapshot
                    .as_ref()
                    .map_or(0, |snapshot| snapshot.candidate_cache.records.len()),
            )
    }

    fn clear_memory_cache(&mut self) {
        self.file_list_snapshots.clear();
        self.catalog_snapshot = None;
        self.update_check_result = None;
        self.last_memory_cache_accessed_at = None;
        self.http_validation_cache.clear();
        self.rebuild_known_path_index();
    }

    fn expire_idle_memory_cache(&mut self) {
        if !self.memory_cache_enabled() {
            self.clear_memory_cache();
            return;
        }
        let idle_lifetime = self.config.cache.idle_lifetime_millis;
        if idle_lifetime == 0 {
            self.clear_memory_cache();
            self.memory_cache_export_allowed = false;
            return;
        }
        let Some(last_accessed_at) = self.last_memory_cache_accessed_at else {
            return;
        };
        let now = now_timestamp_millis();
        if now.saturating_sub(last_accessed_at) >= idle_lifetime {
            self.clear_memory_cache();
            self.memory_cache_export_allowed = false;
        }
    }

    fn touch_memory_cache_if_needed(&mut self) {
        if self.memory_cache_enabled()
            && (self.catalog_snapshot.is_some()
                || !self.file_list_snapshots.is_empty()
                || self.update_check_result.is_some())
        {
            self.last_memory_cache_accessed_at = Some(now_timestamp_millis());
        } else {
            self.last_memory_cache_accessed_at = None;
        }
    }

    fn preserved_update_result(
        &self,
        previous: Option<&ScriptMetaCatalogSnapshot>,
        previous_result: Option<&UpdateCheckResult>,
        current_items: &[ScriptMetaItemRef],
    ) -> Option<Arc<UpdateCheckResult>> {
        if !self.memory_cache_enabled() || !self.config.cache.preserve_update_results {
            return None;
        }
        let previous = previous?;
        let previous_result = previous_result?;
        let previous_items_by_id: BTreeMap<_, _> = previous
            .file_items
            .iter()
            .map(|item| (item.item_id(), item))
            .collect();
        let preserved_item_ids: BTreeSet<_> = current_items
            .iter()
            .filter_map(|item| {
                let item_id = item.item_id();
                previous_items_by_id
                    .get(&item_id)
                    .is_some_and(|previous_item| *previous_item == item)
                    .then_some(item_id)
            })
            .collect();

        Some(Arc::new(UpdateCheckResult {
            checked_at: previous_result.checked_at,
            operation: previous_result.operation.clone(),
            resolutions_by_item_id: previous_result
                .resolutions_by_item_id
                .iter()
                .filter(|(item_id, _)| preserved_item_ids.contains(*item_id))
                .map(|(item_id, resolution)| (item_id.clone(), resolution.clone()))
                .collect(),
            failures_by_item_id: previous_result
                .failures_by_item_id
                .iter()
                .filter(|(item_id, _)| preserved_item_ids.contains(*item_id))
                .map(|(item_id, failure)| (item_id.clone(), failure.clone()))
                .collect(),
            errors_by_item_id: previous_result
                .errors_by_item_id
                .iter()
                .filter(|(item_id, _)| preserved_item_ids.contains(*item_id))
                .map(|(item_id, error)| (item_id.clone(), error.clone()))
                .collect(),
            statuses_by_item_id: previous_result
                .statuses_by_item_id
                .iter()
                .filter(|(item_id, _)| preserved_item_ids.contains(*item_id))
                .map(|(item_id, status)| (item_id.clone(), *status))
                .collect(),
        }))
    }
}

fn root_map(
    roots: Vec<RootRegistration>,
) -> ScriptMetaKitResult<BTreeMap<RootId, RootRegistration>> {
    let mut roots_by_id = BTreeMap::new();
    for root in roots {
        if roots_by_id.insert(root.root_id.clone(), root).is_some() {
            return Err(ScriptMetaKitError::InvalidConfig(
                "root_id values must be unique".to_string(),
            ));
        }
    }
    Ok(roots_by_id)
}

fn merged_root_registration(
    existing: &RootRegistration,
    next: &RootRegistration,
) -> RootRegistration {
    RootRegistration {
        root_id: next.root_id.clone(),
        path: merged_root_path(existing, next),
        display_name: merged_display_name(existing, next),
        purpose: merged_root_purpose(existing.purpose, next.purpose),
        watch_policy: merged_watch_policy(existing.watch_policy, next.watch_policy),
        cache_policy: merged_cache_policy(existing.cache_policy, next.cache_policy),
        refresh_policy: merged_refresh_policy(existing.refresh_policy, next.refresh_policy),
        priority: merged_root_priority(existing.priority, next.priority),
    }
}

fn merged_root_path(existing: &RootRegistration, next: &RootRegistration) -> PathBuf {
    if next.purpose.includes_file_list() {
        next.path.clone()
    } else {
        existing.path.clone()
    }
}

fn merged_display_name(existing: &RootRegistration, next: &RootRegistration) -> Option<String> {
    if next.purpose.includes_file_list() || existing.display_name.is_none() {
        next.display_name
            .clone()
            .or_else(|| existing.display_name.clone())
    } else {
        existing
            .display_name
            .clone()
            .or_else(|| next.display_name.clone())
    }
}

fn merged_root_purpose(lhs: RootPurpose, rhs: RootPurpose) -> RootPurpose {
    if lhs == rhs {
        return lhs;
    }
    if lhs == RootPurpose::FileListAndMetadata || rhs == RootPurpose::FileListAndMetadata {
        return RootPurpose::FileListAndMetadata;
    }
    if (lhs.includes_file_list() || rhs.includes_file_list())
        && (lhs.includes_metadata() || rhs.includes_metadata())
    {
        return RootPurpose::FileListAndMetadata;
    }
    rhs
}

fn merged_watch_policy(lhs: WatchPolicy, rhs: WatchPolicy) -> WatchPolicy {
    if lhs == WatchPolicy::AllRegistered || rhs == WatchPolicy::AllRegistered {
        return WatchPolicy::AllRegistered;
    }
    if lhs == WatchPolicy::VisibleOnly || rhs == WatchPolicy::VisibleOnly {
        return WatchPolicy::VisibleOnly;
    }
    if lhs == WatchPolicy::Manual || rhs == WatchPolicy::Manual {
        return WatchPolicy::Manual;
    }
    WatchPolicy::Disabled
}

fn merged_cache_policy(lhs: CachePolicy, rhs: CachePolicy) -> CachePolicy {
    if lhs == CachePolicy::MemoryAndPersistent || rhs == CachePolicy::MemoryAndPersistent {
        return CachePolicy::MemoryAndPersistent;
    }
    if lhs == CachePolicy::MemoryOnly || rhs == CachePolicy::MemoryOnly {
        return CachePolicy::MemoryOnly;
    }
    if lhs == CachePolicy::PersistentCatalogOnly || rhs == CachePolicy::PersistentCatalogOnly {
        return CachePolicy::PersistentCatalogOnly;
    }
    CachePolicy::Disabled
}

fn merged_refresh_policy(lhs: RefreshPolicy, rhs: RefreshPolicy) -> RefreshPolicy {
    if lhs == RefreshPolicy::OnFileEventDeferred || rhs == RefreshPolicy::OnFileEventDeferred {
        return RefreshPolicy::OnFileEventDeferred;
    }
    if lhs == RefreshPolicy::OnFileEvent || rhs == RefreshPolicy::OnFileEvent {
        return RefreshPolicy::OnFileEvent;
    }
    if lhs == RefreshPolicy::OnVisible || rhs == RefreshPolicy::OnVisible {
        return RefreshPolicy::OnVisible;
    }
    if lhs == RefreshPolicy::Scheduled || rhs == RefreshPolicy::Scheduled {
        return RefreshPolicy::Scheduled;
    }
    RefreshPolicy::ManualOnly
}

fn merged_root_priority(lhs: RootPriority, rhs: RootPriority) -> RootPriority {
    if lhs == RootPriority::UserInitiated || rhs == RootPriority::UserInitiated {
        return RootPriority::UserInitiated;
    }
    if lhs == RootPriority::VisibleWhenSelected || rhs == RootPriority::VisibleWhenSelected {
        return RootPriority::VisibleWhenSelected;
    }
    RootPriority::Background
}

fn root_allows_memory_cache(root: &RootRegistration) -> bool {
    matches!(
        root.cache_policy,
        CachePolicy::MemoryOnly | CachePolicy::MemoryAndPersistent
    )
}

fn merged_catalog_roots(
    previous: &ScriptMetaCatalogSnapshot,
    mut refreshed_roots: Vec<RootSnapshot>,
    refreshed_root_ids: &BTreeSet<RootId>,
) -> Vec<RootSnapshot> {
    let mut roots = previous
        .roots
        .iter()
        .filter(|root| !refreshed_root_ids.contains(&root.root_id))
        .cloned()
        .collect::<Vec<_>>();
    roots.append(&mut refreshed_roots);
    roots.sort_by(|lhs, rhs| lhs.root_id.cmp(&rhs.root_id));
    roots
}

fn merged_candidate_cache(
    previous: &ScriptMetaCatalogSnapshot,
    refreshed: CandidateCache,
    refreshed_root_ids: &BTreeSet<RootId>,
    roots: &[RootRegistration],
    allows_root: impl Fn(&RootRegistration) -> bool,
) -> CandidateCache {
    let CandidateCache {
        built_at,
        records: refreshed_records,
        ..
    } = refreshed;
    let mut records = previous
        .candidate_cache
        .records
        .iter()
        .filter(|record| !refreshed_root_ids.contains(&record.root_id))
        .cloned()
        .collect::<Vec<_>>();
    records.extend(refreshed_records);
    records.sort_by(|lhs, rhs| lhs.identity_path.cmp(&rhs.identity_path));
    let registered_roots = roots
        .iter()
        .filter(|root| allows_root(root))
        .collect::<Vec<_>>();
    CandidateCache {
        schema_version: CandidateCache::CURRENT_SCHEMA_VERSION,
        built_at,
        registered_roots: registered_root_signatures(&registered_roots),
        records,
    }
}

fn root_allows_persistent_catalog_cache(root: &RootRegistration) -> bool {
    matches!(
        root.cache_policy,
        CachePolicy::PersistentCatalogOnly | CachePolicy::MemoryAndPersistent
    )
}

fn root_allows_persistent_file_list_cache(root: &RootRegistration) -> bool {
    matches!(root.cache_policy, CachePolicy::MemoryAndPersistent)
}

fn validate_catalog_cache_roots(
    cache: &CandidateCache,
    roots: &[RootRegistration],
    allows_root: impl Fn(&RootRegistration) -> bool,
) -> ScriptMetaKitResult<()> {
    if !cache.is_current_schema() {
        return Err(ScriptMetaKitError::Cache(format!(
            "unsupported candidate cache schema version {}",
            cache.schema_version
        )));
    }
    let allowed_roots = roots
        .iter()
        .filter(|root| allows_root(root))
        .collect::<Vec<_>>();
    let expected = registered_root_signatures(&allowed_roots);
    if cache.registered_roots != expected {
        return Err(ScriptMetaKitError::Cache(
            "cache root signature does not match registered roots".to_string(),
        ));
    }
    Ok(())
}

fn should_mark_dirty_for_file_event(
    root: &RootRegistration,
    visible_root_id: Option<&RootId>,
) -> bool {
    match root.refresh_policy {
        RefreshPolicy::ManualOnly | RefreshPolicy::Scheduled => false,
        RefreshPolicy::OnVisible => visible_root_id == Some(&root.root_id),
        RefreshPolicy::OnFileEvent | RefreshPolicy::OnFileEventDeferred => true,
    }
}

fn filter_update_result_to_items(
    result: &UpdateCheckResult,
    items: &[ScriptMetaItemRef],
) -> Option<Arc<UpdateCheckResult>> {
    let item_ids = items
        .iter()
        .map(|item| item.item_id())
        .collect::<BTreeSet<_>>();
    let result = UpdateCheckResult {
        checked_at: result.checked_at,
        operation: result.operation.clone(),
        resolutions_by_item_id: result
            .resolutions_by_item_id
            .iter()
            .filter(|(item_id, _)| item_ids.contains(*item_id))
            .map(|(item_id, resolution)| (item_id.clone(), resolution.clone()))
            .collect(),
        failures_by_item_id: result
            .failures_by_item_id
            .iter()
            .filter(|(item_id, _)| item_ids.contains(*item_id))
            .map(|(item_id, failure)| (item_id.clone(), failure.clone()))
            .collect(),
        errors_by_item_id: result
            .errors_by_item_id
            .iter()
            .filter(|(item_id, _)| item_ids.contains(*item_id))
            .map(|(item_id, error)| (item_id.clone(), error.clone()))
            .collect(),
        statuses_by_item_id: result
            .statuses_by_item_id
            .iter()
            .filter(|(item_id, _)| item_ids.contains(*item_id))
            .map(|(item_id, status)| (item_id.clone(), *status))
            .collect(),
    };
    (!result.statuses_by_item_id.is_empty()
        || !result.resolutions_by_item_id.is_empty()
        || !result.failures_by_item_id.is_empty()
        || !result.errors_by_item_id.is_empty())
    .then(|| Arc::new(result))
}

fn scan_file_list_job(
    job: FileListScanJob,
    scanner_options: &crate::scanner::ScannerOptions,
    extensions: &crate::scanner::ExtensionPolicy,
    cancellation: OperationCancellation,
) -> crate::scanner::DirectoryScanOutput {
    if let Some(dirty_directories) = job.dirty_directories.as_ref() {
        return scan_file_list_root_with_dirty_directories_controlled(
            &job.root.root_id,
            &job.root.path,
            scanner_options,
            extensions,
            job.previous_snapshot.as_deref(),
            dirty_directories,
            Some(&cancellation),
        );
    }

    scan_file_list_root_transactional_controlled(
        &job.root.root_id,
        &job.root.path,
        scanner_options,
        extensions,
        job.previous_snapshot.as_deref(),
        Some(&cancellation),
        job.probe_script_headers,
    )
}

fn scan_operation_summary(roots: &[RootSnapshot]) -> OperationSummary {
    let total = roots.len();
    let failed = roots.iter().filter(|root| root.error.is_some()).count();
    let completed = roots
        .iter()
        .filter(|root| {
            matches!(
                root.status,
                RootStatus::Ready
                    | RootStatus::Missing
                    | RootStatus::Unreadable
                    | RootStatus::TimedOut
                    | RootStatus::Overflowed
                    | RootStatus::Cancelled
            )
        })
        .count();
    if roots
        .iter()
        .any(|root| root.status == RootStatus::Cancelled)
    {
        OperationSummary::cancelled(total, completed, failed)
    } else if roots.iter().any(|root| root.status == RootStatus::TimedOut) {
        OperationSummary::timed_out(total, completed, failed)
    } else if roots
        .iter()
        .any(|root| matches!(root.status, RootStatus::Overflowed | RootStatus::Unreadable))
    {
        OperationSummary::partial(total, completed, failed)
    } else {
        OperationSummary::finished(total, completed, failed)
    }
}

fn collect_scan_file_issues(
    roots: &[RootSnapshot],
    file_list_snapshots: &[Arc<FileListSnapshot>],
    catalog_snapshot: Option<&ScriptMetaCatalogSnapshot>,
) -> Vec<FileIssue> {
    let mut issues = Vec::new();
    for root in roots {
        if let Some(error) = root.error.as_ref() {
            issues.push(FileIssue {
                root_id: Some(root.root_id.to_string()),
                path: root.path.clone(),
                code: error.code.clone(),
                message: error.message.clone(),
                path_kind: None,
                resolution_status: Some(root_status_issue_code(root.status).to_string()),
                is_directory: true,
            });
        }
    }
    for snapshot in file_list_snapshots {
        if let Some(children) = snapshot.children.as_deref() {
            collect_file_entry_issues(&snapshot.root.root_id, children, &mut issues);
        }
    }
    if let Some(catalog) = catalog_snapshot {
        for record in &catalog.candidate_cache.records {
            collect_candidate_record_issue(record, &mut issues);
        }
    }
    issues.sort_by(|lhs, rhs| {
        lhs.root_id
            .cmp(&rhs.root_id)
            .then_with(|| lhs.path.cmp(&rhs.path))
            .then_with(|| lhs.code.cmp(&rhs.code))
    });
    issues.dedup_by(|lhs, rhs| {
        lhs.root_id == rhs.root_id && lhs.path == rhs.path && lhs.code == rhs.code
    });
    issues
}

fn collect_file_entry_issues(
    root_id: &RootId,
    entries: &[FileSystemEntry],
    output: &mut Vec<FileIssue>,
) {
    for entry in entries {
        if is_resolution_issue(entry.resolution_status) {
            output.push(FileIssue {
                root_id: Some(root_id.to_string()),
                path: entry.display_path.clone(),
                code: file_issue_code_for_resolution(entry.resolution_status).to_string(),
                message: entry
                    .resolution_message
                    .clone()
                    .unwrap_or_else(|| entry.resolution_status.as_str().to_string()),
                path_kind: Some(entry.path_kind.as_str().to_string()),
                resolution_status: Some(entry.resolution_status.as_str().to_string()),
                is_directory: entry.is_directory,
            });
        }
        if entry.is_file_locked {
            output.push(FileIssue {
                root_id: Some(root_id.to_string()),
                path: entry.display_path.clone(),
                code: "file_locked".to_string(),
                message: "file is locked".to_string(),
                path_kind: Some(entry.path_kind.as_str().to_string()),
                resolution_status: Some(entry.resolution_status.as_str().to_string()),
                is_directory: entry.is_directory,
            });
        }
        if entry.is_read_only {
            output.push(FileIssue {
                root_id: Some(root_id.to_string()),
                path: entry.display_path.clone(),
                code: "read_only".to_string(),
                message: "file or parent directory is read-only".to_string(),
                path_kind: Some(entry.path_kind.as_str().to_string()),
                resolution_status: Some(entry.resolution_status.as_str().to_string()),
                is_directory: entry.is_directory,
            });
        }
        collect_file_entry_issues(root_id, &entry.children, output);
    }
}

fn collect_candidate_record_issue(record: &CandidateRecord, output: &mut Vec<FileIssue>) {
    if is_resolution_issue(record.resolution_status) {
        output.push(FileIssue {
            root_id: Some(record.root_id.to_string()),
            path: record.file_path.clone(),
            code: file_issue_code_for_resolution(record.resolution_status).to_string(),
            message: record
                .resolution_message
                .clone()
                .unwrap_or_else(|| record.resolution_status.as_str().to_string()),
            path_kind: Some(record.path_kind.as_str().to_string()),
            resolution_status: Some(record.resolution_status.as_str().to_string()),
            is_directory: false,
        });
    }
}

fn is_resolution_issue(status: crate::PathResolutionStatus) -> bool {
    !matches!(
        status,
        crate::PathResolutionStatus::NotRequested | crate::PathResolutionStatus::Resolved
    )
}

fn file_issue_code_for_resolution(status: crate::PathResolutionStatus) -> &'static str {
    match status {
        crate::PathResolutionStatus::PermissionDenied => "permission_denied",
        crate::PathResolutionStatus::Unsupported => "unsupported_format",
        crate::PathResolutionStatus::Cycle => "cycle_detected",
        crate::PathResolutionStatus::Broken => "unreadable_file",
        crate::PathResolutionStatus::NotRequested | crate::PathResolutionStatus::Resolved => "none",
    }
}

fn root_status_issue_code(status: RootStatus) -> &'static str {
    match status {
        RootStatus::Missing => "missing",
        RootStatus::Unreadable => "unreadable",
        RootStatus::TimedOut => "timed_out",
        RootStatus::Overflowed => "overflowed",
        RootStatus::Cancelled => "cancelled",
        RootStatus::NotLoaded | RootStatus::Ready | RootStatus::Dirty | RootStatus::Loading => {
            "none"
        }
    }
}

#[derive(Debug, Deserialize)]
struct CatalogCacheData {
    catalog_snapshot: ScriptMetaCatalogSnapshot,
    #[serde(default)]
    update_check_result: Option<UpdateCheckResult>,
}

#[derive(Debug, Serialize)]
struct CatalogCacheDataRef<'a> {
    catalog_snapshot: &'a ScriptMetaCatalogSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    update_check_result: Option<&'a UpdateCheckResult>,
}

#[derive(Debug, Deserialize)]
struct AllCacheData {
    #[serde(default)]
    catalog: Option<serde_json::Value>,
    #[serde(default)]
    file_list_snapshots: BTreeMap<RootId, Arc<FileListSnapshot>>,
}

#[derive(Debug, Serialize)]
struct AllCacheDataRef<'a> {
    catalog: serde_json::Value,
    file_list_snapshots: &'a BTreeMap<RootId, Arc<FileListSnapshot>>,
}

fn decode_all_cache_data(data: serde_json::Value) -> ScriptMetaKitResult<AllCacheData> {
    if data.get("catalog").is_some() || data.get("file_list_snapshots").is_some() {
        return serde_json::from_value(data)
            .map_err(|error| ScriptMetaKitError::Cache(error.to_string()));
    }

    Ok(AllCacheData {
        catalog: Some(data),
        file_list_snapshots: BTreeMap::new(),
    })
}

fn decode_catalog_cache_data(data: serde_json::Value) -> ScriptMetaKitResult<CatalogCacheData> {
    if data.get("catalog_snapshot").is_some() {
        return serde_json::from_value(data)
            .map_err(|error| ScriptMetaKitError::Cache(error.to_string()));
    }

    let update_check_result = data
        .get("update_check_result")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
    let catalog_snapshot = serde_json::from_value(data)
        .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
    Ok(CatalogCacheData {
        catalog_snapshot,
        update_check_result,
    })
}

fn emit_update_progress(
    progress: &mut Option<&mut dyn FnMut(UpdateCheckProgress)>,
    completed_items: usize,
    total_items: usize,
    item_id: Option<&str>,
    script_id: Option<&str>,
    phase: UpdateCheckProgressPhase,
    message: impl FnOnce() -> String,
) {
    let Some(progress) = progress.as_deref_mut() else {
        return;
    };
    progress(UpdateCheckProgress {
        completed_items,
        total_items,
        item_id: item_id.map(str::to_owned),
        script_id: script_id.map(str::to_owned),
        phase,
        message: message(),
    });
}

enum UpdateWorkEvent {
    Checking {
        item_id: String,
        script_id: String,
    },
    Retrying {
        item_id: String,
        script_id: String,
        error: String,
    },
    Finished {
        item_index: usize,
        item_id: String,
        script_id: String,
        resolved_result: Box<ScriptMetaKitResult<ResolvedItemUpdate>>,
    },
}

fn update_work_groups(items: &[ScriptMetaItemRef]) -> Vec<Vec<usize>> {
    let mut groups = Vec::<Vec<usize>>::new();
    let mut meta_url_group_indices = BTreeMap::<String, usize>::new();

    for (index, item) in items.iter().enumerate() {
        if item.is_update_checkable()
            && let Some(meta_url) = item.meta_url.as_ref()
        {
            let key = meta_url.as_str().to_string();
            if let Some(group_index) = meta_url_group_indices.get(&key) {
                groups[*group_index].push(index);
            } else {
                let group_index = groups.len();
                meta_url_group_indices.insert(key, group_index);
                groups.push(vec![index]);
            }
        } else {
            groups.push(vec![index]);
        }
    }

    groups
}

fn resolve_update_item_with_retry(
    update_resolver: &UpdateResolver,
    item: &ScriptMetaItem,
    policy: UpdateRetryPolicy,
    cancellation: &OperationCancellation,
    mut retry: impl FnMut(&ScriptMetaKitError),
) -> ScriptMetaKitResult<ResolvedItemUpdate> {
    let mut attempt = 0usize;
    loop {
        if cancellation.is_cancelled() {
            return Err(ScriptMetaKitError::Timeout(
                "update resolution was cancelled".to_string(),
            ));
        }
        if let Some(meta_url) = item.meta_url.as_ref()
            && let Some(error) = update_resolver.terminal_source_failure(meta_url)?
        {
            return Err(error.as_ref().clone());
        }
        match update_resolver.resolve_item(item) {
            Ok(resolved) => return Ok(resolved),
            Err(error) => {
                if attempt >= policy.attempts
                    || cancellation.is_cancelled()
                    || !is_retryable_update_error(&error)
                    || update_resolver.is_terminal_source_failure(&error)?
                {
                    return Err(error);
                }
                retry(&error);
                let delay = update_retry_delay(
                    policy.initial_delay_millis,
                    policy.backoff_multiplier,
                    attempt,
                );
                let server_delay = retry_after_hint_millis(&error)
                    .map(Duration::from_millis)
                    .unwrap_or_default();
                let delay = delay.max(server_delay);
                if delay > Duration::from_millis(policy.max_delay_millis) {
                    return Err(error);
                }
                attempt += 1;
                if cancellation.wait_for_cancellation(delay) {
                    return Err(ScriptMetaKitError::Timeout(
                        "update resolution was cancelled".to_string(),
                    ));
                }
            }
        }
    }
}

fn update_retry_delay(
    initial_delay_millis: u64,
    backoff_multiplier: u32,
    retry_index: usize,
) -> Duration {
    let multiplier = u64::from(backoff_multiplier.max(1));
    let mut delay_millis = initial_delay_millis;
    for _ in 0..retry_index {
        delay_millis = delay_millis.saturating_mul(multiplier);
    }
    Duration::from_millis(delay_millis)
}

fn is_retryable_update_error(error: &ScriptMetaKitError) -> bool {
    matches!(
        error,
        ScriptMetaKitError::Url(_) | ScriptMetaKitError::Io { .. } | ScriptMetaKitError::Timeout(_)
    )
}

fn apply_update_item_result(
    result: &mut UpdateCheckResult,
    item: &ScriptMetaItem,
    item_id: String,
    checked_at: crate::TimestampMillis,
    resolved_result: ScriptMetaKitResult<ResolvedItemUpdate>,
) -> UpdateStatus {
    match resolved_result {
        Ok(resolved) => {
            let resolved_status = resolved.status;
            if let Some(resolution) = resolved.resolution {
                if resolved_status == UpdateStatus::Failed {
                    let failure = crate::catalog::UpdateFailure::unresolved_distribution(
                        item_id.clone(),
                        item,
                        &resolution,
                    );
                    result
                        .errors_by_item_id
                        .insert(item_id.clone(), failure.message.clone());
                    result.failures_by_item_id.insert(item_id.clone(), failure);
                }
                result
                    .resolutions_by_item_id
                    .insert(item_id.clone(), resolution);
            }
            if let Some(error) = resolved.error {
                let failure = crate::catalog::UpdateFailure::from_message(
                    item_id.clone(),
                    item,
                    resolved.checked_at,
                    "update_check_failed",
                    error,
                    None,
                );
                result
                    .errors_by_item_id
                    .insert(item_id.clone(), failure.message.clone());
                result.failures_by_item_id.insert(item_id.clone(), failure);
            }
            result.statuses_by_item_id.insert(item_id, resolved_status);
            resolved_status
        }
        Err(error) => {
            let message = error.to_string();
            let Some(meta_url) = item.meta_url.clone() else {
                result
                    .statuses_by_item_id
                    .insert(item_id, UpdateStatus::NotCheckable);
                return UpdateStatus::NotCheckable;
            };
            let failure = crate::catalog::UpdateFailure::from_error(
                item_id.clone(),
                item,
                checked_at,
                &error,
            );
            result.resolutions_by_item_id.insert(
                item_id.clone(),
                unresolved_distribution(meta_url, checked_at, failure.message.clone()),
            );
            result.errors_by_item_id.insert(item_id.clone(), message);
            result.failures_by_item_id.insert(item_id.clone(), failure);
            result
                .statuses_by_item_id
                .insert(item_id, UpdateStatus::Failed);
            UpdateStatus::Failed
        }
    }
}

fn mark_unchecked_update_items_cancelled(
    result: &mut UpdateCheckResult,
    items: &[ScriptMetaItemRef],
    checked_at: crate::TimestampMillis,
) {
    for item in items {
        let item_id = item.item_id();
        if result.statuses_by_item_id.contains_key(&item_id) {
            continue;
        }
        let failure = UpdateFailure::from_message(
            item_id.clone(),
            item,
            checked_at,
            "operation_cancelled",
            "update check was cancelled before this item was processed",
            item.meta_url.clone(),
        );
        result
            .errors_by_item_id
            .insert(item_id.clone(), failure.message.clone());
        result.failures_by_item_id.insert(item_id.clone(), failure);
        result
            .statuses_by_item_id
            .insert(item_id, UpdateStatus::Cancelled);
    }
}

fn bounded_parallelism(job_count: usize) -> usize {
    if job_count <= 1 {
        return job_count;
    }
    thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .max(1)
        .min(job_count)
}

fn validate_config(config: &ScriptMetaKitConfig) -> ScriptMetaKitResult<()> {
    if config.app_id.trim().is_empty() {
        return Err(ScriptMetaKitError::InvalidConfig(
            "app_id must not be empty".to_string(),
        ));
    }
    if config.cache_namespace.trim().is_empty() {
        return Err(ScriptMetaKitError::InvalidConfig(
            "cache_namespace must not be empty".to_string(),
        ));
    }
    Ok(())
}

fn diff_file_list_snapshot(
    root_id: &RootId,
    previous: &FileListSnapshot,
    current: &FileListSnapshot,
) -> ScanChangeSummary {
    let mut previous_entries = BTreeMap::new();
    let mut current_entries = BTreeMap::new();
    collect_file_entries(
        previous.children.as_deref().unwrap_or_default(),
        &mut previous_entries,
    );
    collect_file_entries(
        current.children.as_deref().unwrap_or_default(),
        &mut current_entries,
    );

    let mut summary = ScanChangeSummary::default();
    for (path, current_entry) in &current_entries {
        match previous_entries.get(path) {
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

    for (path, previous_entry) in &previous_entries {
        if current_entries.contains_key(path) {
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

fn file_entry_count(entries: &[FileSystemEntry]) -> usize {
    entries
        .iter()
        .map(|entry| 1usize.saturating_add(file_entry_count(&entry.children)))
        .sum()
}

fn collect_directory_paths(entries: &[FileSystemEntry], output: &mut BTreeSet<PathBuf>) {
    for entry in entries {
        if entry.is_directory {
            output.insert(entry.resolved_path.clone());
        }
        collect_directory_paths(&entry.children, output);
    }
}

fn collect_file_paths(entries: &[FileSystemEntry], output: &mut BTreeSet<PathBuf>) {
    for entry in entries {
        if entry.is_directory {
            collect_file_paths(&entry.children, output);
        } else {
            output.insert(entry.resolved_path.clone());
        }
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
        || lhs.scriptmeta_item != rhs.scriptmeta_item
}

fn apply_metadata_capabilities_to_file_list_snapshots(
    snapshots: &mut [FileListSnapshot],
    records: &[CandidateRecord],
) {
    let capability_by_display_path: BTreeMap<_, _> = records
        .iter()
        .map(|record| {
            (
                (record.root_id.as_ref(), record.file_path.as_path()),
                record,
            )
        })
        .collect();
    let capability_by_identity_path: BTreeMap<_, _> = records
        .iter()
        .map(|record| {
            (
                (record.root_id.as_ref(), record.identity_path.as_path()),
                record,
            )
        })
        .collect();

    for snapshot in snapshots {
        if let Some(children) = snapshot.children.as_mut() {
            apply_metadata_capabilities_to_entries(
                snapshot.root.root_id.as_ref(),
                children,
                &capability_by_display_path,
                &capability_by_identity_path,
            );
        }
    }
}

fn apply_metadata_capabilities_to_entries(
    root_id: &str,
    entries: &mut [FileSystemEntry],
    by_display_path: &BTreeMap<(&str, &std::path::Path), &CandidateRecord>,
    by_identity_path: &BTreeMap<(&str, &std::path::Path), &CandidateRecord>,
) {
    for entry in entries {
        if let Some(record) = by_display_path
            .get(&(root_id, entry.display_path.as_path()))
            .or_else(|| by_identity_path.get(&(root_id, entry.resolved_path.as_path())))
        {
            entry.has_scriptmeta = record.has_scriptmeta;
            entry.runtime_kind = record.runtime_kind;
            entry.shebang.clone_from(&record.shebang);
            entry.has_scriptmeta_edit_password = record.has_scriptmeta_edit_password;
            entry.is_file_locked = record.is_file_locked;
            entry.is_read_only = record.is_read_only;
            entry.can_edit_scriptmeta = record.can_edit_scriptmeta;
            entry.can_append_scriptmeta = record.can_append_scriptmeta;
            entry.scriptmeta_edit_state = record.scriptmeta_edit_state;
            entry.scriptmeta_item = record.item.as_ref().map(Arc::clone);
        }
        apply_metadata_capabilities_to_entries(
            root_id,
            &mut entry.children,
            by_display_path,
            by_identity_path,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, path::PathBuf, sync::Arc};

    use url::Url;

    use super::{
        apply_metadata_capabilities_to_file_list_snapshots, decode_all_cache_data,
        merged_root_purpose, update_retry_delay, update_work_groups,
    };
    use crate::{
        RootId,
        catalog::{
            CachePolicy, FileListSnapshot, RefreshPolicy, RootPriority, RootPurpose,
            RootRegistration, RootSnapshot,
        },
        core::{ScriptMetaEditState, ScriptMetaItem},
        scanner::{CandidateRecord, FileSystemEntry, PathKind, PathResolutionStatus},
        watcher::WatchPolicy,
    };

    #[test]
    fn root_groups_recompute_merged_roots_when_group_is_replaced() {
        let mut engine = super::ScriptMetaKitEngine::new(Default::default()).expect("engine");
        let metadata_root = RootRegistration {
            root_id: RootId::from("shared"),
            path: PathBuf::from("/tmp/root"),
            display_name: Some("Metadata".to_string()),
            purpose: RootPurpose::MetadataCatalog,
            watch_policy: WatchPolicy::Disabled,
            cache_policy: CachePolicy::PersistentCatalogOnly,
            refresh_policy: RefreshPolicy::ManualOnly,
            priority: RootPriority::Background,
        };
        let file_list_root = RootRegistration {
            purpose: RootPurpose::FileList,
            display_name: Some("Files".to_string()),
            watch_policy: WatchPolicy::AllRegistered,
            cache_policy: CachePolicy::MemoryAndPersistent,
            refresh_policy: RefreshPolicy::OnFileEventDeferred,
            priority: RootPriority::UserInitiated,
            ..metadata_root.clone()
        };

        engine
            .replace_root_group("metadata", vec![metadata_root.clone()])
            .expect("metadata roots");
        engine
            .insert_roots_into_group("file-list", vec![file_list_root])
            .expect("file list roots");

        assert_eq!(engine.roots().len(), 1);
        let merged = &engine.roots()[0];
        assert_eq!(merged.purpose, RootPurpose::FileListAndMetadata);
        assert_eq!(merged.display_name.as_deref(), Some("Files"));
        assert_eq!(merged.watch_policy, WatchPolicy::AllRegistered);
        assert_eq!(merged.cache_policy, CachePolicy::MemoryAndPersistent);
        assert_eq!(merged.refresh_policy, RefreshPolicy::OnFileEventDeferred);
        assert_eq!(merged.priority, RootPriority::UserInitiated);

        engine
            .replace_root_group("file-list", Vec::new())
            .expect("remove file list roots");

        assert_eq!(engine.roots(), &[metadata_root]);
    }

    #[test]
    fn root_purpose_merge_preserves_file_list_and_update_metadata_in_both_orders() {
        assert_eq!(
            merged_root_purpose(RootPurpose::FileList, RootPurpose::UpdateCheck),
            RootPurpose::FileListAndMetadata
        );
        assert_eq!(
            merged_root_purpose(RootPurpose::UpdateCheck, RootPurpose::FileList),
            RootPurpose::FileListAndMetadata
        );
    }

    #[test]
    fn groups_update_work_by_meta_url_only_for_checkable_items() {
        let items = vec![
            script_item("one", Some("file:///tmp/shared.txt"), Some("1.0.0")),
            script_item("two", Some("file:///tmp/shared.txt"), Some("1.0.0")),
            script_item("missing-version", Some("file:///tmp/shared.txt"), None),
            script_item("missing-url", None, Some("1.0.0")),
            script_item("other", Some("file:///tmp/other.txt"), Some("1.0.0")),
        ];

        assert_eq!(
            update_work_groups(&items),
            vec![vec![0, 1], vec![2], vec![3], vec![4]]
        );
    }

    #[test]
    fn update_retry_delay_uses_bounded_integer_backoff() {
        assert_eq!(update_retry_delay(500, 3, 0).as_millis(), 500);
        assert_eq!(update_retry_delay(500, 3, 1).as_millis(), 1_500);
        assert_eq!(update_retry_delay(500, 0, 2).as_millis(), 500);
        assert_eq!(
            update_retry_delay(u64::MAX, u32::MAX, 4).as_millis(),
            u128::from(u64::MAX)
        );
    }

    fn script_item(
        script_id: &str,
        meta_url: Option<&str>,
        version: Option<&str>,
    ) -> Arc<ScriptMetaItem> {
        let file_path = PathBuf::from(format!("/tmp/{script_id}.jsx"));
        Arc::new(ScriptMetaItem {
            root_id: RootId::from("root"),
            file_path: file_path.clone(),
            identity_path: file_path,
            runtime_kind: None,
            shebang: None,
            script_id: script_id.to_string(),
            version: version.map(str::to_string),
            description: None,
            target_app: None,
            min_target_version: None,
            meta_url: meta_url.map(|url| Url::parse(url).expect("valid test URL")),
            name: None,
            author: None,
            release_date: None,
            edit_password_sha256: None,
            has_scriptmeta: true,
            has_scriptmeta_edit_password: false,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: true,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: ScriptMetaEditState::Editable,
        })
    }

    #[test]
    fn attaches_scriptmeta_item_to_matching_file_entry() {
        let root_id = RootId::from("root");
        let file_path = PathBuf::from("/tmp/root/sample.jsx");
        let item = script_item_at_path("sample", &root_id, &file_path);
        let record = CandidateRecord {
            root_id: root_id.clone(),
            root_path: Arc::new(PathBuf::from("/tmp/root")),
            file_path: file_path.clone(),
            identity_path: file_path.clone(),
            path_kind: PathKind::Normal,
            resolution_status: PathResolutionStatus::Resolved,
            resolution_message: None,
            runtime_kind: None,
            shebang: None,
            has_scriptmeta: true,
            has_scriptmeta_edit_password: false,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: true,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: ScriptMetaEditState::Editable,
            file_size: Some(42),
            content_modified_at: Some(1_000),
            identity: None,
            item: Some(Arc::clone(&item)),
        };
        let mut snapshots = vec![FileListSnapshot {
            root: RootSnapshot::new(root_id.clone(), PathBuf::from("/tmp/root")),
            children: Some(vec![file_entry(&file_path)]),
            directory_states: Default::default(),
            truncated: false,
        }];

        apply_metadata_capabilities_to_file_list_snapshots(&mut snapshots, &[record]);

        let entry = snapshots[0]
            .children
            .as_ref()
            .and_then(|children| children.first())
            .expect("file entry");
        assert_eq!(
            entry
                .scriptmeta_item
                .as_ref()
                .map(|item| item.script_id.as_str()),
            Some("sample")
        );
        assert!(entry.has_scriptmeta);
        assert!(entry.can_edit_scriptmeta);
    }

    #[test]
    fn decodes_all_cache_with_file_list_snapshots_only() {
        let data = serde_json::json!({
            "file_list_snapshots": {}
        });

        let cache = decode_all_cache_data(data).expect("decode all cache");

        assert!(cache.catalog.is_none());
        assert!(cache.file_list_snapshots.is_empty());
    }

    fn file_entry(path: &Path) -> FileSystemEntry {
        FileSystemEntry {
            display_path: path.to_path_buf(),
            resolved_path: path.to_path_buf(),
            path_kind: PathKind::Normal,
            resolution_status: PathResolutionStatus::Resolved,
            resolution_message: None,
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
            scriptmeta_edit_state: ScriptMetaEditState::Unknown,
            scriptmeta_item: None,
            children: Vec::new(),
        }
    }

    fn script_item_at_path(
        script_id: &str,
        root_id: &RootId,
        file_path: &Path,
    ) -> Arc<ScriptMetaItem> {
        Arc::new(ScriptMetaItem {
            root_id: root_id.clone(),
            file_path: file_path.to_path_buf(),
            identity_path: file_path.to_path_buf(),
            runtime_kind: None,
            shebang: None,
            script_id: script_id.to_string(),
            version: Some("1.0.0".to_string()),
            description: None,
            target_app: None,
            min_target_version: None,
            meta_url: None,
            name: Some("Sample Script".to_string()),
            author: None,
            release_date: None,
            edit_password_sha256: None,
            has_scriptmeta: true,
            has_scriptmeta_edit_password: false,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: true,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: ScriptMetaEditState::Editable,
        })
    }
}
