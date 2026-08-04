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
        SnapshotRevision, UpdateCheckProgress, UpdateCheckProgressPhase, UpdateCheckRequest,
        UpdateCheckResult, UpdateFailure, UpdateStatus, path_based_root_id,
        unresolved_distribution,
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
        CandidateCache, CandidateRecord, FileSystemEntry, MetadataScanSources, PathKind,
        PathResolutionStatus, deduplicated_items, file_items_from_cache,
        registered_root_signatures, resolve_registered_path,
        scan_file_list_root_transactional_controlled,
        scan_file_list_root_with_dirty_directories_controlled,
        scan_metadata_roots_scoped_with_file_lists_controlled,
        try_scan_file_list_root_with_owned_dirty_directories_controlled,
    },
    storage::CachePayload,
    watcher::{
        ChangeRoutingOptions, RawChangeBatch, WatchPlan, WatchPolicy,
        build_watch_plan_with_resolved_targets, normalize_path, route_change_batch,
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
    resolved_watch_targets: BTreeMap<RootId, BTreeSet<PathBuf>>,
    resolved_watch_sources: BTreeMap<RootId, BTreeSet<PathBuf>>,
    catalog_snapshot: Option<Arc<ScriptMetaCatalogSnapshot>>,
    update_check_result: Option<Arc<UpdateCheckResult>>,
    dirty_roots: BTreeMap<RootId, DirtyRootState>,
    last_memory_cache_accessed_at: Option<crate::TimestampMillis>,
    evicted_file_list_roots: BTreeSet<RootId>,
    evicted_catalog_roots: BTreeSet<RootId>,
    persisted_file_list_revisions: BTreeMap<RootId, SnapshotRevision>,
    catalog_persistence_is_current: bool,
    pending_file_list_persistence: BTreeMap<RootId, Arc<FileListSnapshot>>,
    cache_unavailable_revisions: BTreeMap<RootId, SnapshotRevision>,
    pending_catalog_persistence: Option<Arc<ScriptMetaCatalogSnapshot>>,
    pending_update_check_persistence: Option<Arc<UpdateCheckResult>>,
    invalidate_persistent_file_list_on_next_export: bool,
    invalidate_persistent_catalog_on_next_export: bool,
    http_validation_cache: HttpValidationCache,
    cancellation: OperationCancellation,
    workspace_epoch: String,
    next_revision_sequence: u64,
}

impl Clone for ScriptMetaKitEngine {
    fn clone(&self) -> Self {
        let mut cloned = self.clone_preserving_identity();
        cloned.cancellation = OperationCancellation::new();
        cloned.workspace_epoch = Uuid::new_v4().to_string();
        cloned.next_revision_sequence = 1;
        cloned.reissue_all_revisions();
        cloned
    }
}

impl ScriptMetaKitEngine {
    fn clone_preserving_identity(&self) -> Self {
        Self {
            config: self.config.clone(),
            roots: self.roots.clone(),
            root_groups: self.root_groups.clone(),
            visible_root_id: self.visible_root_id.clone(),
            root_snapshots: self.root_snapshots.clone(),
            file_list_snapshots: self.file_list_snapshots.clone(),
            known_directory_paths: self.known_directory_paths.clone(),
            known_file_paths: self.known_file_paths.clone(),
            resolved_watch_targets: self.resolved_watch_targets.clone(),
            resolved_watch_sources: self.resolved_watch_sources.clone(),
            catalog_snapshot: self.catalog_snapshot.clone(),
            update_check_result: self.update_check_result.clone(),
            dirty_roots: self.dirty_roots.clone(),
            last_memory_cache_accessed_at: self.last_memory_cache_accessed_at,
            evicted_file_list_roots: self.evicted_file_list_roots.clone(),
            evicted_catalog_roots: self.evicted_catalog_roots.clone(),
            persisted_file_list_revisions: self.persisted_file_list_revisions.clone(),
            catalog_persistence_is_current: self.catalog_persistence_is_current,
            pending_file_list_persistence: self.pending_file_list_persistence.clone(),
            cache_unavailable_revisions: self.cache_unavailable_revisions.clone(),
            pending_catalog_persistence: self.pending_catalog_persistence.clone(),
            pending_update_check_persistence: self.pending_update_check_persistence.clone(),
            invalidate_persistent_file_list_on_next_export: self
                .invalidate_persistent_file_list_on_next_export,
            invalidate_persistent_catalog_on_next_export: self
                .invalidate_persistent_catalog_on_next_export,
            http_validation_cache: self.http_validation_cache.clone(),
            cancellation: self.cancellation.clone(),
            workspace_epoch: self.workspace_epoch.clone(),
            next_revision_sequence: self.next_revision_sequence,
        }
    }

    #[doc(hidden)]
    #[must_use]
    pub fn clone_for_reconfiguration(&self) -> Self {
        self.clone_preserving_identity()
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
            resolved_watch_targets: BTreeMap::new(),
            resolved_watch_sources: BTreeMap::new(),
            catalog_snapshot: None,
            update_check_result: None,
            dirty_roots: BTreeMap::new(),
            last_memory_cache_accessed_at: None,
            evicted_file_list_roots: BTreeSet::new(),
            evicted_catalog_roots: BTreeSet::new(),
            persisted_file_list_revisions: BTreeMap::new(),
            catalog_persistence_is_current: false,
            pending_file_list_persistence: BTreeMap::new(),
            cache_unavailable_revisions: BTreeMap::new(),
            pending_catalog_persistence: None,
            pending_update_check_persistence: None,
            invalidate_persistent_file_list_on_next_export: false,
            invalidate_persistent_catalog_on_next_export: false,
            http_validation_cache: HttpValidationCache::default(),
            cancellation: OperationCancellation::new(),
            workspace_epoch: Uuid::new_v4().to_string(),
            next_revision_sequence: 1,
        })
    }

    fn issue_revision(&mut self) -> SnapshotRevision {
        if self.next_revision_sequence == u64::MAX {
            self.workspace_epoch = Uuid::new_v4().to_string();
            self.next_revision_sequence = 1;
        }
        let revision = SnapshotRevision {
            workspace_epoch: self.workspace_epoch.clone(),
            sequence: self.next_revision_sequence,
        };
        self.next_revision_sequence += 1;
        revision
    }

    fn reissue_all_revisions(&mut self) {
        let mut root_revisions = BTreeMap::<RootId, SnapshotRevision>::new();
        let mut root_ids = self.root_snapshots.keys().cloned().collect::<BTreeSet<_>>();
        root_ids.extend(
            self.catalog_snapshot
                .iter()
                .flat_map(|snapshot| snapshot.roots.iter().map(|root| root.root_id.clone())),
        );
        root_ids.extend(
            self.pending_catalog_persistence
                .iter()
                .flat_map(|snapshot| snapshot.roots.iter().map(|root| root.root_id.clone())),
        );
        for root_id in root_ids {
            root_revisions.insert(root_id, self.issue_revision());
        }
        for (root_id, root) in &mut self.root_snapshots {
            if let Some(revision) = root_revisions.get(root_id) {
                root.state_revision.clone_from(revision);
            }
        }

        let mut content_revisions = BTreeMap::<RootId, SnapshotRevision>::new();
        let persisted_content_root_ids = self
            .file_list_snapshots
            .iter()
            .chain(self.pending_file_list_persistence.iter())
            .filter(|(root_id, snapshot)| {
                self.persisted_file_list_revisions
                    .get(*root_id)
                    .is_some_and(|revision| revision == &snapshot.content_revision)
            })
            .map(|(root_id, _)| root_id.clone())
            .collect::<BTreeSet<_>>();
        let content_root_ids = self
            .file_list_snapshots
            .iter()
            .chain(self.pending_file_list_persistence.iter())
            .filter(|(_, snapshot)| snapshot.children.is_some())
            .map(|(root_id, _)| root_id.clone())
            .collect::<BTreeSet<_>>();
        for root_id in content_root_ids {
            content_revisions.insert(root_id, self.issue_revision());
        }
        for root_id in persisted_content_root_ids {
            if let Some(revision) = content_revisions.get(&root_id) {
                self.persisted_file_list_revisions
                    .insert(root_id, revision.clone());
            }
        }
        for snapshots in [
            &mut self.file_list_snapshots,
            &mut self.pending_file_list_persistence,
        ] {
            for (root_id, snapshot) in snapshots {
                let snapshot = Arc::make_mut(snapshot);
                snapshot.content_revision = snapshot
                    .children
                    .as_ref()
                    .and_then(|_| content_revisions.get(root_id).cloned())
                    .unwrap_or_default();
                if let Some(revision) = root_revisions.get(root_id) {
                    snapshot.root.state_revision.clone_from(revision);
                }
            }
        }
        let unavailable_root_ids = self
            .cache_unavailable_revisions
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for root_id in unavailable_root_ids {
            let revision = self.issue_revision();
            self.cache_unavailable_revisions.insert(root_id, revision);
        }
        for catalog in [
            &mut self.catalog_snapshot,
            &mut self.pending_catalog_persistence,
        ]
        .into_iter()
        .flatten()
        {
            for root in &mut Arc::make_mut(catalog).roots {
                if let Some(revision) = root_revisions.get(&root.root_id) {
                    root.state_revision.clone_from(revision);
                }
            }
        }
    }

    #[must_use]
    pub fn config(&self) -> &ScriptMetaKitConfig {
        &self.config
    }

    #[must_use]
    pub fn config_mut(&mut self) -> &mut ScriptMetaKitConfig {
        &mut self.config
    }

    pub fn set_resolve_macos_alias(&mut self, enabled: bool) {
        if self.config.scanner.resolve_macos_alias == enabled {
            return;
        }
        self.config.scanner.resolve_macos_alias = enabled;
        self.resolved_watch_targets.clear();
        self.resolved_watch_sources.clear();
        self.rebuild_known_path_index();
    }

    pub fn set_roots(
        &mut self,
        roots: Vec<RootRegistration>,
    ) -> ScriptMetaKitResult<Vec<ScriptMetaKitEvent>> {
        let events = self.apply_roots(roots)?;
        self.root_groups.clear();
        Ok(events)
    }

    pub fn replace_root_group(
        &mut self,
        group_id: impl Into<String>,
        roots: Vec<RootRegistration>,
    ) -> ScriptMetaKitResult<Vec<ScriptMetaKitEvent>> {
        self.expire_idle_memory_cache();
        let group_id = group_id.into();
        let mut candidate_groups = self.root_groups.clone();
        if roots.is_empty() {
            candidate_groups.remove(&group_id);
        } else {
            candidate_groups.insert(group_id, root_map(roots)?);
        }
        let merged_roots = merged_roots_from_groups(&candidate_groups)?;
        let events = self.apply_roots(merged_roots)?;
        self.root_groups = candidate_groups;
        Ok(events)
    }

    pub fn insert_roots_into_group(
        &mut self,
        group_id: impl Into<String>,
        roots: Vec<RootRegistration>,
    ) -> ScriptMetaKitResult<Vec<ScriptMetaKitEvent>> {
        self.expire_idle_memory_cache();
        let mut candidate_groups = self.root_groups.clone();
        let group = candidate_groups.entry(group_id.into()).or_default();
        for root in roots {
            group.insert(root.root_id.clone(), root);
        }
        let merged_roots = merged_roots_from_groups(&candidate_groups)?;
        let events = self.apply_roots(merged_roots)?;
        self.root_groups = candidate_groups;
        Ok(events)
    }

    fn apply_roots(
        &mut self,
        mut roots: Vec<RootRegistration>,
    ) -> ScriptMetaKitResult<Vec<ScriptMetaKitEvent>> {
        self.expire_idle_memory_cache();
        for root in &mut roots {
            if root.refresh_policy == RefreshPolicy::OnFileEventDeferred {
                root.refresh_policy = RefreshPolicy::OnFileEvent;
            }
        }
        let previous_roots_by_id: BTreeMap<_, _> = self
            .roots
            .iter()
            .map(|root| (root.root_id.clone(), root.clone()))
            .collect();
        let previous_root_ids: BTreeSet<_> =
            self.roots.iter().map(|root| root.root_id.clone()).collect();
        let next_root_ids: BTreeSet<_> = roots.iter().map(|root| root.root_id.clone()).collect();
        if next_root_ids.len() != roots.len() {
            return Err(ScriptMetaKitError::InvalidConfig(
                "root_id values must be unique".to_string(),
            ));
        }
        let mut events = Vec::new();
        for removed_id in previous_root_ids.difference(&next_root_ids) {
            self.root_snapshots.remove(removed_id);
            self.file_list_snapshots.remove(removed_id);
            self.dirty_roots.remove(removed_id);
            self.evicted_file_list_roots.remove(removed_id);
            self.evicted_catalog_roots.remove(removed_id);
            self.persisted_file_list_revisions.remove(removed_id);
            self.pending_file_list_persistence.remove(removed_id);
            self.cache_unavailable_revisions.remove(removed_id);
            self.resolved_watch_targets.remove(removed_id);
            self.resolved_watch_sources.remove(removed_id);
            events.push(ScriptMetaKitEvent::RootRemoved {
                root_id: removed_id.clone(),
            });
        }

        let mut invalidated_catalog_root_ids = previous_root_ids
            .difference(&next_root_ids)
            .filter(|root_id| {
                previous_roots_by_id
                    .get(*root_id)
                    .is_some_and(|root| root.purpose.includes_metadata())
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        for root in &roots {
            if !previous_root_ids.contains(&root.root_id) {
                events.push(ScriptMetaKitEvent::RootRegistered {
                    root_id: root.root_id.clone(),
                });
            }
            let previous = previous_roots_by_id.get(&root.root_id);
            let cache_availability_changed =
                previous.is_some_and(|previous| previous.cache_policy != root.cache_policy);
            if cache_availability_changed {
                self.cache_unavailable_revisions.remove(&root.root_id);
            }
            let file_list_content_changed = previous.is_some_and(|previous| {
                previous.purpose.includes_file_list() != root.purpose.includes_file_list()
                    || ((previous.purpose.includes_file_list()
                        || root.purpose.includes_file_list())
                        && normalize_path(&previous.path) != normalize_path(&root.path))
            });
            let metadata_content_changed = previous.is_some_and(|previous| {
                previous.purpose.includes_metadata() != root.purpose.includes_metadata()
                    || ((previous.purpose.includes_metadata() || root.purpose.includes_metadata())
                        && normalize_path(&previous.path) != normalize_path(&root.path))
            });
            if file_list_content_changed {
                self.file_list_snapshots.remove(&root.root_id);
                self.dirty_roots.remove(&root.root_id);
                self.evicted_file_list_roots.remove(&root.root_id);
                self.persisted_file_list_revisions.remove(&root.root_id);
                self.pending_file_list_persistence.remove(&root.root_id);
                self.cache_unavailable_revisions.remove(&root.root_id);
                self.resolved_watch_targets.remove(&root.root_id);
                self.resolved_watch_sources.remove(&root.root_id);
            }
            if metadata_content_changed {
                self.evicted_catalog_roots.remove(&root.root_id);
                invalidated_catalog_root_ids.insert(root.root_id.clone());
            }
            if file_list_content_changed || metadata_content_changed {
                let mut snapshot = RootSnapshot::new(root.root_id.clone(), root.path.clone());
                snapshot.state_revision = self.issue_revision();
                self.root_snapshots.insert(root.root_id.clone(), snapshot);
            } else {
                if !self.root_snapshots.contains_key(&root.root_id) {
                    let mut snapshot = RootSnapshot::new(root.root_id.clone(), root.path.clone());
                    snapshot.state_revision = self.issue_revision();
                    self.root_snapshots.insert(root.root_id.clone(), snapshot);
                } else if cache_availability_changed {
                    let revision = self.issue_revision();
                    if let Some(snapshot) = self.root_snapshots.get_mut(&root.root_id) {
                        snapshot.state_revision = revision;
                    }
                }
            }
        }

        if !self.catalog_persistence_is_current
            && self.pending_catalog_persistence.is_none()
            && let Some(snapshot) = self.catalog_snapshot.as_ref()
        {
            self.pending_catalog_persistence = Some(Arc::clone(snapshot));
            self.pending_update_check_persistence = self.update_check_result.clone();
        }
        self.roots = roots;
        self.reproject_caches_after_root_change(&invalidated_catalog_root_ids);
        self.rebuild_known_path_index();
        Ok(events)
    }

    fn reproject_caches_after_root_change(
        &mut self,
        invalidated_catalog_root_ids: &BTreeSet<RootId>,
    ) {
        let roots_without_memory_file_list = self
            .roots
            .iter()
            .filter(|root| !root_allows_memory_cache(root))
            .map(|root| root.root_id.clone())
            .collect::<Vec<_>>();
        for root_id in roots_without_memory_file_list {
            if let Some(snapshot) = self.file_list_snapshots.remove(&root_id) {
                if self
                    .root_by_id(&root_id)
                    .is_some_and(root_allows_persistent_file_list_cache)
                    && self.persisted_file_list_revisions.get(&root_id)
                        != Some(&snapshot.content_revision)
                {
                    self.pending_file_list_persistence
                        .insert(root_id.clone(), snapshot);
                }
                self.ensure_cache_unavailable_revision(&root_id);
            }
        }

        if let Some(snapshot) = self.catalog_snapshot.clone() {
            let affected_root_ids = snapshot
                .roots
                .iter()
                .map(|root| root.root_id.clone())
                .collect::<Vec<_>>();
            let projected = self.catalog_snapshot_for_policy_excluding(
                snapshot.as_ref(),
                root_allows_memory_cache,
                invalidated_catalog_root_ids,
            );
            self.update_check_result = self
                .update_check_result
                .as_ref()
                .and_then(|result| filter_update_result_to_items(result, &projected.file_items));
            self.catalog_snapshot = (!projected.candidate_cache.registered_roots.is_empty())
                .then(|| Arc::new(projected));
            for root_id in affected_root_ids {
                self.refresh_cache_unavailable_revision(&root_id);
            }
        }

        if let Some(snapshot) = self.pending_catalog_persistence.clone() {
            let projected = self.catalog_snapshot_for_policy_excluding(
                snapshot.as_ref(),
                root_allows_persistent_catalog_cache,
                invalidated_catalog_root_ids,
            );
            self.pending_update_check_persistence = self
                .pending_update_check_persistence
                .as_ref()
                .and_then(|result| filter_update_result_to_items(result, &projected.file_items));
            self.pending_catalog_persistence =
                (!projected.candidate_cache.registered_roots.is_empty())
                    .then(|| Arc::new(projected));
        }

        if !invalidated_catalog_root_ids.is_empty() {
            self.catalog_persistence_is_current = false;
        }
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
        let plan = build_watch_plan_with_resolved_targets(
            &self.roots,
            self.config.watcher.watch_policy,
            self.config.watcher.monitor_root_strategy,
            self.visible_root_id.as_ref(),
            &self.resolved_watch_targets,
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

    #[must_use]
    pub fn preflight_root(&mut self, root: RootRegistration) -> ScanResult {
        let _operation_scope = self.cancellation.begin_scope();
        let mut root = crate::scanner::preflight_root_registration(
            &root,
            &self.config.scanner,
            &self.config.supported_extensions,
            Some(&self.cancellation),
        );
        root.state_revision = self.issue_revision();
        let roots = vec![root];
        let operation = scan_operation_summary(&roots);
        let file_issues = collect_scan_file_issues(&roots, &[], None);
        ScanResult {
            roots,
            file_list_snapshots: Vec::new(),
            catalog_snapshot: None,
            operation,
            file_issues,
            update_check_result: None,
            change_summary: None,
            watch_change_batch: None,
            watch_reconciliation: false,
            watch_covers_all_roots: false,
        }
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
            let purpose = self
                .root_by_id(&root.root_id)
                .map_or(RootPurpose::FileListAndMetadata, |root| root.purpose);
            let expects_file_list =
                request.mode.includes_file_list() && purpose.includes_file_list();
            let expects_catalog = request.mode.includes_metadata() && purpose.includes_metadata();
            let has_file_list = !expects_file_list
                || file_list_snapshots
                    .iter()
                    .any(|snapshot| snapshot.root.root_id == root.root_id);
            let has_catalog = !expects_catalog
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
                root.state_revision = self.ensure_cache_unavailable_revision(&root.root_id);
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
            watch_reconciliation: false,
            watch_covers_all_roots: false,
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
                let mut root_snapshot = output.root;
                let root_id = root_snapshot.root_id.clone();
                let previous_root = self.root_snapshots.get(&root_id).cloned();
                root_snapshot.state_revision = previous_root
                    .as_ref()
                    .filter(|previous| root_state_equal(previous, &root_snapshot))
                    .map(|previous| previous.state_revision.clone())
                    .filter(SnapshotRevision::is_available)
                    .unwrap_or_else(|| self.issue_revision());
                let previous_snapshot = self.file_list_snapshots.get(&root_id).cloned();
                let mut snapshot = FileListSnapshot {
                    root: root_snapshot.clone(),
                    children: Some(output.children),
                    directory_states: output.directory_states,
                    truncated: output.truncated,
                    content_revision: SnapshotRevision::default(),
                };
                if root_snapshot.status != RootStatus::Ready {
                    if let Some(previous) = previous_snapshot
                        .as_ref()
                        .filter(|value| value.children.is_some())
                    {
                        snapshot.children.clone_from(&previous.children);
                        snapshot
                            .directory_states
                            .clone_from(&previous.directory_states);
                        snapshot.truncated = previous.truncated;
                        snapshot
                            .content_revision
                            .clone_from(&previous.content_revision);
                    } else {
                        snapshot.children = None;
                        snapshot.content_revision = SnapshotRevision::default();
                    }
                }
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
            let mut output = {
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
                        visible_root_id: self.visible_root_id.as_ref(),
                    },
                    Some(&self.cancellation),
                )
            };
            for root in &mut output.roots {
                root.state_revision = self
                    .root_snapshots
                    .get(&root.root_id)
                    .filter(|previous| root_state_equal(previous, root))
                    .map(|previous| previous.state_revision.clone())
                    .filter(SnapshotRevision::is_available)
                    .unwrap_or_else(|| self.issue_revision());
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
                .map(|mut snapshot| {
                    let root_id = snapshot.root.root_id.clone();
                    snapshot.content_revision = if snapshot.children.is_none() {
                        SnapshotRevision::default()
                    } else {
                        self.file_list_snapshots
                            .get(&root_id)
                            .filter(|previous| file_list_content_equal(previous, &snapshot))
                            .map(|previous| previous.content_revision.clone())
                            .filter(SnapshotRevision::is_available)
                            .unwrap_or_else(|| self.issue_revision())
                    };
                    let snapshot = Arc::new(snapshot);
                    if self.should_store_file_list_snapshot(&root_id) {
                        if snapshot.children.is_some() {
                            self.pending_file_list_persistence.remove(&root_id);
                        }
                        self.file_list_snapshots
                            .insert(root_id.clone(), Arc::clone(&snapshot));
                        self.refresh_cache_unavailable_revision(&root_id);
                    } else {
                        self.file_list_snapshots.remove(&root_id);
                        self.ensure_cache_unavailable_revision(&root_id);
                    }
                    snapshot
                })
                .collect()
        } else {
            Vec::new()
        };
        self.refresh_resolved_watch_paths(&file_list_snapshots);
        if request.mode.includes_file_list() {
            for snapshot in &file_list_snapshots {
                if snapshot.root.status == RootStatus::Ready && snapshot.children.is_some() {
                    self.evicted_file_list_roots.remove(&snapshot.root.root_id);
                }
            }
        }
        if request.mode.includes_metadata()
            && let Some(snapshot) = catalog_snapshot.as_ref()
        {
            for root in &snapshot.roots {
                if root.status == RootStatus::Ready {
                    self.evicted_catalog_roots.remove(&root.root_id);
                }
            }
        }
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
            watch_reconciliation: false,
            watch_covers_all_roots: false,
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
                resolved_targets_by_root: &self.resolved_watch_targets,
                resolved_sources_by_root: &self.resolved_watch_sources,
                scanner_options: &self.config.scanner,
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
            let revision = self.issue_revision();
            if let Some(snapshot) = self.root_snapshots.get_mut(&change.root_id) {
                snapshot.is_dirty = true;
                snapshot.last_event_at = Some(now);
                snapshot.status = if change.requires_full_rescan {
                    RootStatus::Overflowed
                } else {
                    RootStatus::Dirty
                };
                snapshot.state_revision = revision;
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
                watch_reconciliation: false,
                watch_covers_all_roots: false,
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
            watch_reconciliation: false,
            watch_covers_all_roots: false,
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
        self.catalog_persistence_is_current = false;
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
        let mut payload = payload.migrate()?;
        payload.validate_for_config(&self.config)?;
        payload.scope = canonical_cache_scope(payload.scope);
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
        self.export_cache_merged(scope, None)
    }

    pub fn export_cache_merged(
        &self,
        scope: CacheScope,
        existing: Option<CachePayload>,
    ) -> ScriptMetaKitResult<CachePayload> {
        let scope = canonical_cache_scope(scope);
        let existing = existing
            .map(CachePayload::migrate)
            .transpose()?
            .map(|mut payload| {
                payload.validate_for_config(&self.config)?;
                payload.scope = canonical_cache_scope(payload.scope);
                if payload.scope != scope {
                    return Err(ScriptMetaKitError::Cache(
                        "existing cache scope does not match requested scope".to_string(),
                    ));
                }
                Ok(payload)
            })
            .transpose()?;
        if existing.is_none() {
            self.ensure_standalone_cache_export_is_complete(scope)?;
        }
        match scope {
            CacheScope::All => {
                let existing = existing
                    .map(|payload| decode_all_cache_data(payload.data))
                    .transpose()?;
                let file_list_snapshots = self.merged_persistent_file_list_cache_snapshots(
                    if self.invalidate_persistent_file_list_on_next_export {
                        None
                    } else {
                        existing.as_ref().map(|data| &data.file_list_snapshots)
                    },
                );
                let catalog = self.merged_catalog_cache_data(
                    if self.invalidate_persistent_catalog_on_next_export {
                        None
                    } else {
                        existing.and_then(|data| data.catalog)
                    },
                )?;
                let data = serde_json::to_value(AllCacheDataRef {
                    catalog,
                    file_list_snapshots: &file_list_snapshots,
                })
                .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
                Ok(CachePayload::new_for_config(
                    CacheScope::All,
                    data,
                    &self.config,
                ))
            }
            CacheScope::Catalog => {
                let data = self.merged_catalog_cache_data(
                    if self.invalidate_persistent_catalog_on_next_export {
                        None
                    } else {
                        existing.map(|payload| payload.data)
                    },
                )?;
                Ok(CachePayload::new_for_config(
                    CacheScope::Catalog,
                    data,
                    &self.config,
                ))
            }
            CacheScope::FileList | CacheScope::Root => {
                let existing_snapshots = existing
                    .map(|payload| {
                        serde_json::from_value::<BTreeMap<RootId, Arc<FileListSnapshot>>>(
                            payload.data,
                        )
                        .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))
                    })
                    .transpose()?;
                let snapshots = self.merged_persistent_file_list_cache_snapshots(
                    if self.invalidate_persistent_file_list_on_next_export {
                        None
                    } else {
                        existing_snapshots.as_ref()
                    },
                );
                let data = serde_json::to_value(&snapshots)
                    .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
                Ok(CachePayload::new_for_config(scope, data, &self.config))
            }
        }
    }

    fn ensure_standalone_cache_export_is_complete(
        &self,
        scope: CacheScope,
    ) -> ScriptMetaKitResult<()> {
        if matches!(
            scope,
            CacheScope::All | CacheScope::FileList | CacheScope::Root
        ) {
            let available = self
                .persistent_file_list_cache_snapshots()
                .into_keys()
                .collect::<BTreeSet<_>>();
            let missing = self.evicted_file_list_roots.iter().any(|root_id| {
                self.root_by_id(root_id)
                    .is_some_and(root_allows_persistent_file_list_cache)
                    && !available.contains(root_id)
            });
            if missing {
                return Err(ScriptMetaKitError::Cache(
                    "file-list memory is incomplete and no durable cache was supplied for merge"
                        .to_string(),
                ));
            }
        }
        if matches!(scope, CacheScope::All | CacheScope::Catalog) {
            let mut available = self
                .catalog_snapshot
                .iter()
                .flat_map(|snapshot| snapshot.roots.iter().map(|root| root.root_id.clone()))
                .collect::<BTreeSet<_>>();
            available.extend(
                self.pending_catalog_persistence
                    .iter()
                    .flat_map(|snapshot| snapshot.roots.iter().map(|root| root.root_id.clone())),
            );
            let missing = self.evicted_catalog_roots.iter().any(|root_id| {
                self.root_by_id(root_id)
                    .is_some_and(root_allows_persistent_catalog_cache)
                    && !available.contains(root_id)
            });
            if missing {
                return Err(ScriptMetaKitError::Cache(
                    "catalog memory is incomplete and no durable cache was supplied for merge"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn mark_cache_persisted(&mut self, scope: CacheScope) {
        match scope {
            CacheScope::All => {
                self.mark_file_list_cache_persisted();
                self.pending_file_list_persistence.clear();
                self.pending_catalog_persistence = None;
                self.pending_update_check_persistence = None;
                self.catalog_persistence_is_current = true;
                self.invalidate_persistent_file_list_on_next_export = false;
                self.invalidate_persistent_catalog_on_next_export = false;
            }
            CacheScope::Catalog => {
                self.pending_catalog_persistence = None;
                self.pending_update_check_persistence = None;
                self.catalog_persistence_is_current = true;
                self.invalidate_persistent_catalog_on_next_export = false;
            }
            CacheScope::FileList | CacheScope::Root => {
                self.mark_file_list_cache_persisted();
                self.pending_file_list_persistence.clear();
                self.invalidate_persistent_file_list_on_next_export = false;
            }
        }
    }

    fn mark_file_list_cache_persisted(&mut self) {
        let revisions = self
            .persistent_file_list_cache_snapshots()
            .into_iter()
            .filter_map(|(root_id, snapshot)| {
                snapshot
                    .content_revision
                    .is_available()
                    .then_some((root_id, snapshot.content_revision.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let allowed_root_ids = self
            .roots
            .iter()
            .filter(|root| root_allows_persistent_file_list_cache(root))
            .map(|root| root.root_id.clone())
            .collect::<BTreeSet<_>>();
        self.persisted_file_list_revisions
            .retain(|root_id, _| allowed_root_ids.contains(root_id));
        self.persisted_file_list_revisions.extend(revisions);
    }

    fn merged_persistent_file_list_cache_snapshots(
        &self,
        existing: Option<&BTreeMap<RootId, Arc<FileListSnapshot>>>,
    ) -> BTreeMap<RootId, Arc<FileListSnapshot>> {
        let mut snapshots = existing.cloned().unwrap_or_default();
        snapshots.retain(|root_id, snapshot| {
            self.root_by_id(root_id).is_some_and(|root| {
                root_allows_persistent_file_list_cache(root)
                    && normalize_path(&snapshot.root.path) == normalize_path(&root.path)
            })
        });
        snapshots.extend(self.persistent_file_list_cache_snapshots());
        snapshots
    }

    fn load_catalog_cache_data(&mut self, data: CatalogCacheData) -> ScriptMetaKitResult<()> {
        if !data.catalog_snapshot.candidate_cache.is_current_schema() {
            return Err(ScriptMetaKitError::Cache(format!(
                "unsupported candidate cache schema version {}",
                data.catalog_snapshot.candidate_cache.schema_version
            )));
        }
        let previous_catalog_snapshot = self.catalog_snapshot.clone();
        let has_unpersisted_catalog_data = self.pending_catalog_persistence.is_some()
            || (self.catalog_snapshot.is_some() && !self.catalog_persistence_is_current);
        let incoming = serde_json::to_value(&data)
            .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
        let merged = self.merged_catalog_cache_data(Some(incoming))?;
        let CatalogCacheData {
            mut catalog_snapshot,
            update_check_result,
        } = decode_catalog_cache_data(merged)?;
        let file_items = file_items_from_cache(&catalog_snapshot.candidate_cache);
        catalog_snapshot.all_items = deduplicated_items(&file_items);
        catalog_snapshot.file_items = file_items;
        let mut catalog_snapshot =
            self.catalog_snapshot_for_policy(&catalog_snapshot, root_allows_memory_cache);
        let unchanged_root_ids = catalog_snapshot
            .roots
            .iter()
            .filter(|root| {
                previous_catalog_snapshot.as_ref().is_some_and(|previous| {
                    catalog_root_content_equal(previous, &catalog_snapshot, &root.root_id)
                })
            })
            .map(|root| root.root_id.clone())
            .collect::<BTreeSet<_>>();
        for root in &mut catalog_snapshot.roots {
            let root_id = root.root_id.clone();
            let mut current_root = self
                .root_snapshots
                .get(&root_id)
                .cloned()
                .unwrap_or_else(|| RootSnapshot::new(root_id.clone(), root.path.clone()));
            if !unchanged_root_ids.contains(&root_id) {
                current_root.state_revision = self.issue_revision();
            }
            self.root_snapshots.insert(root_id, current_root.clone());
            *root = current_root;
        }
        self.update_check_result = update_check_result
            .as_ref()
            .and_then(|result| filter_update_result_to_items(result, &catalog_snapshot.file_items));
        self.catalog_snapshot = Some(Arc::new(catalog_snapshot));
        self.catalog_persistence_is_current = !has_unpersisted_catalog_data;
        let loaded_root_ids = self
            .catalog_snapshot
            .iter()
            .flat_map(|snapshot| snapshot.roots.iter().map(|root| root.root_id.clone()))
            .collect::<Vec<_>>();
        if let Some(snapshot) = self.catalog_snapshot.as_ref() {
            for root in &snapshot.roots {
                self.evicted_catalog_roots.remove(&root.root_id);
            }
        }
        for root_id in loaded_root_ids {
            self.refresh_cache_unavailable_revision(&root_id);
        }
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
                if self.pending_file_list_persistence.contains_key(&root_id)
                    || self.file_list_snapshots.contains_key(&root_id)
                {
                    continue;
                }
                let mut snapshot = snapshot.as_ref().clone();
                let mut current_root = self
                    .root_snapshots
                    .get(&root_id)
                    .cloned()
                    .unwrap_or_else(|| RootSnapshot::new(root_id.clone(), root.path.clone()));
                current_root.state_revision = self.issue_revision();
                snapshot.root = current_root.clone();
                snapshot.content_revision = if snapshot.children.is_some() {
                    self.issue_revision()
                } else {
                    SnapshotRevision::default()
                };
                let snapshot = Arc::new(snapshot);
                self.root_snapshots.insert(root_id.clone(), current_root);
                self.evicted_file_list_roots.remove(&root_id);
                if snapshot.content_revision.is_available() {
                    self.persisted_file_list_revisions
                        .insert(root_id.clone(), snapshot.content_revision.clone());
                }
                self.file_list_snapshots.insert(root_id.clone(), snapshot);
                self.refresh_cache_unavailable_revision(&root_id);
            }
        }
        let loaded_snapshots = self
            .file_list_snapshots
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for snapshot in &loaded_snapshots {
            self.replace_resolved_watch_paths(snapshot);
        }
        self.rebuild_known_path_index();
        self.enforce_memory_node_limit();
        self.touch_memory_cache_if_needed();
        Ok(())
    }

    fn merged_catalog_cache_data(
        &self,
        existing: Option<serde_json::Value>,
    ) -> ScriptMetaKitResult<serde_json::Value> {
        let existing = existing.map(decode_catalog_cache_data).transpose()?;
        let allowed_roots = self
            .roots
            .iter()
            .filter(|root| root_allows_persistent_catalog_cache(root))
            .collect::<Vec<_>>();
        let allowed_by_id = allowed_roots
            .iter()
            .map(|root| (root.root_id.clone(), *root))
            .collect::<BTreeMap<_, _>>();
        let mut roots_by_id = BTreeMap::<RootId, RootSnapshot>::new();
        let mut records_by_root = BTreeMap::<RootId, Vec<CandidateRecord>>::new();
        let mut source_revision = Uuid::new_v4();
        let mut built_at = 0;
        let mut merged_update_result: Option<UpdateCheckResult> = None;

        let mut merge_source =
            |snapshot: &ScriptMetaCatalogSnapshot, update_result: Option<&UpdateCheckResult>| {
                source_revision = snapshot.source_revision;
                built_at = built_at.max(snapshot.candidate_cache.built_at);
                let source_root_ids = snapshot
                    .roots
                    .iter()
                    .filter_map(|root| {
                        allowed_by_id.get(&root.root_id).and_then(|registration| {
                            (normalize_path(&root.path) == normalize_path(&registration.path))
                                .then_some(root.root_id.clone())
                        })
                    })
                    .collect::<BTreeSet<_>>();
                for root in &snapshot.roots {
                    if source_root_ids.contains(&root.root_id) {
                        roots_by_id.insert(root.root_id.clone(), root.clone());
                        records_by_root.remove(&root.root_id);
                    }
                }
                for record in &snapshot.candidate_cache.records {
                    if source_root_ids.contains(&record.root_id) {
                        records_by_root
                            .entry(record.root_id.clone())
                            .or_default()
                            .push(record.clone());
                    }
                }
                if let Some(update_result) = update_result {
                    merge_update_check_result(&mut merged_update_result, update_result);
                }
            };

        if let Some(existing) = existing.as_ref() {
            merge_source(
                &existing.catalog_snapshot,
                existing.update_check_result.as_ref(),
            );
        }
        if let Some(current) = self.catalog_snapshot.as_ref() {
            merge_source(current, self.update_check_result.as_deref());
        }
        if let Some(pending) = self.pending_catalog_persistence.as_ref() {
            merge_source(pending, self.pending_update_check_persistence.as_deref());
        }

        let roots = allowed_roots
            .iter()
            .filter_map(|root| roots_by_id.remove(&root.root_id))
            .collect::<Vec<_>>();
        let mut records = records_by_root.into_values().flatten().collect::<Vec<_>>();
        records.sort_by(|lhs, rhs| lhs.identity_path.cmp(&rhs.identity_path));
        let candidate_cache = CandidateCache {
            schema_version: CandidateCache::CURRENT_SCHEMA_VERSION,
            built_at: if built_at == 0 {
                now_timestamp_millis()
            } else {
                built_at
            },
            registered_roots: registered_root_signatures(&allowed_roots),
            records,
        };
        let file_items = file_items_from_cache(&candidate_cache);
        let all_items = deduplicated_items(&file_items);
        let catalog_snapshot = ScriptMetaCatalogSnapshot {
            source_revision,
            roots,
            all_items,
            file_items,
            candidate_cache,
        };
        let update_check_result = merged_update_result
            .as_ref()
            .and_then(|result| filter_update_result_to_items(result, &catalog_snapshot.file_items));
        serde_json::to_value(CatalogCacheDataRef {
            catalog_snapshot: &catalog_snapshot,
            update_check_result: update_check_result.as_deref(),
        })
        .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))
    }

    fn persistent_file_list_cache_snapshots(&self) -> BTreeMap<RootId, Arc<FileListSnapshot>> {
        let mut snapshots = self
            .file_list_snapshots
            .iter()
            .filter(|(root_id, snapshot)| {
                snapshot.children.is_some()
                    && self.root_by_id(root_id).is_some_and(|root| {
                        root_allows_persistent_file_list_cache(root)
                            && normalize_path(&snapshot.root.path) == normalize_path(&root.path)
                    })
            })
            .map(|(root_id, snapshot)| (root_id.clone(), Arc::clone(snapshot)))
            .collect::<BTreeMap<_, _>>();
        snapshots.extend(
            self.pending_file_list_persistence
                .iter()
                .filter(|(root_id, snapshot)| {
                    snapshot.children.is_some()
                        && self
                            .root_by_id(root_id)
                            .is_some_and(root_allows_persistent_file_list_cache)
                })
                .map(|(root_id, snapshot)| (root_id.clone(), Arc::clone(snapshot))),
        );
        snapshots
    }

    pub fn invalidate_cache(
        &mut self,
        scope: CacheScope,
        reason: CacheInvalidationReason,
    ) -> Vec<ScriptMetaKitEvent> {
        let affected_root_ids = match scope {
            CacheScope::All => {
                self.file_list_snapshots
                    .keys()
                    .cloned()
                    .chain(self.catalog_snapshot.iter().flat_map(|snapshot| {
                        snapshot.roots.iter().map(|root| root.root_id.clone())
                    }))
                    .collect::<BTreeSet<_>>()
            }
            CacheScope::Catalog => self
                .catalog_snapshot
                .iter()
                .flat_map(|snapshot| snapshot.roots.iter().map(|root| root.root_id.clone()))
                .collect(),
            CacheScope::FileList | CacheScope::Root => {
                self.file_list_snapshots.keys().cloned().collect()
            }
        };
        for root_id in affected_root_ids {
            let revision = self.issue_revision();
            if let Some(root) = self.root_snapshots.get_mut(&root_id) {
                root.state_revision = revision;
            }
        }
        match scope {
            CacheScope::All => {
                self.catalog_snapshot = None;
                self.update_check_result = None;
                self.file_list_snapshots.clear();
                self.pending_file_list_persistence.clear();
                self.pending_catalog_persistence = None;
                self.pending_update_check_persistence = None;
                self.evicted_file_list_roots.clear();
                self.evicted_catalog_roots.clear();
                self.persisted_file_list_revisions.clear();
                self.catalog_persistence_is_current = false;
                self.invalidate_persistent_file_list_on_next_export = true;
                self.invalidate_persistent_catalog_on_next_export = true;
                self.resolved_watch_targets.clear();
                self.resolved_watch_sources.clear();
            }
            CacheScope::Catalog => {
                self.catalog_snapshot = None;
                self.update_check_result = None;
                self.pending_catalog_persistence = None;
                self.pending_update_check_persistence = None;
                self.evicted_catalog_roots.clear();
                self.catalog_persistence_is_current = false;
                self.invalidate_persistent_catalog_on_next_export = true;
            }
            CacheScope::FileList | CacheScope::Root => {
                self.file_list_snapshots.clear();
                self.pending_file_list_persistence.clear();
                self.evicted_file_list_roots.clear();
                self.persisted_file_list_revisions.clear();
                self.invalidate_persistent_file_list_on_next_export = true;
                self.resolved_watch_targets.clear();
                self.resolved_watch_sources.clear();
            }
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

        jobs.sort_by_key(|job| root_scan_priority(&job.root, self.visible_root_id.as_ref()));

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

    fn ensure_cache_unavailable_revision(&mut self, root_id: &RootId) -> SnapshotRevision {
        if let Some(revision) = self.cache_unavailable_revisions.get(root_id) {
            return revision.clone();
        }
        let revision = self.issue_revision();
        self.cache_unavailable_revisions
            .insert(root_id.clone(), revision.clone());
        revision
    }

    fn refresh_cache_unavailable_revision(&mut self, root_id: &RootId) {
        let Some(root) = self.root_by_id(root_id) else {
            self.cache_unavailable_revisions.remove(root_id);
            return;
        };
        let file_list_is_resident =
            !root.purpose.includes_file_list() || self.file_list_snapshots.contains_key(root_id);
        let catalog_is_resident = !root.purpose.includes_metadata()
            || self
                .catalog_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.roots.iter().any(|root| root.root_id == *root_id));
        if file_list_is_resident && catalog_is_resident {
            self.cache_unavailable_revisions.remove(root_id);
        } else {
            self.ensure_cache_unavailable_revision(root_id);
        }
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
            directories.extend(snapshot.directory_states.keys().map(PathBuf::from));
        }
        for targets in self.resolved_watch_targets.values() {
            directories.extend(targets.iter().cloned());
        }
        for sources in self.resolved_watch_sources.values() {
            directories.extend(sources.iter().cloned());
        }
        self.known_directory_paths = directories;
        self.known_file_paths = files;
    }

    fn refresh_resolved_watch_paths(&mut self, snapshots: &[Arc<FileListSnapshot>]) {
        for snapshot in snapshots {
            match snapshot.root.status {
                RootStatus::Ready => self.replace_resolved_watch_paths(snapshot),
                RootStatus::Missing => {
                    self.resolved_watch_targets.remove(&snapshot.root.root_id);
                    self.resolved_watch_sources.remove(&snapshot.root.root_id);
                }
                RootStatus::NotLoaded
                | RootStatus::Dirty
                | RootStatus::Loading
                | RootStatus::Unreadable
                | RootStatus::TimedOut
                | RootStatus::Overflowed
                | RootStatus::Cancelled => {}
            }
        }
    }

    fn replace_resolved_watch_paths(&mut self, snapshot: &FileListSnapshot) {
        let Some(root) = self.root_by_id(&snapshot.root.root_id) else {
            return;
        };
        let root_resolution = resolve_registered_path(
            &root.path,
            &self.config.scanner,
            Some(&self.config.supported_extensions),
        );
        let physical_root = normalize_path(&root_resolution.resolved_path);
        let mut targets = snapshot
            .directory_states
            .keys()
            .map(PathBuf::from)
            .map(|path| normalize_path(&path))
            .filter(|path| !path.starts_with(&physical_root))
            .collect::<Vec<_>>();
        targets.sort_by(|lhs, rhs| {
            lhs.components()
                .count()
                .cmp(&rhs.components().count())
                .then_with(|| lhs.cmp(rhs))
        });
        let mut minimal_targets = BTreeSet::new();
        for target in targets {
            if !minimal_targets
                .iter()
                .any(|parent: &PathBuf| target.starts_with(parent))
            {
                minimal_targets.insert(target);
            }
        }

        let mut sources = BTreeSet::new();
        collect_resolved_directory_link_sources(
            snapshot.children.as_deref().unwrap_or_default(),
            &mut sources,
        );
        if minimal_targets.is_empty() {
            self.resolved_watch_targets.remove(&snapshot.root.root_id);
        } else {
            self.resolved_watch_targets
                .insert(snapshot.root.root_id.clone(), minimal_targets);
        }
        if sources.is_empty() {
            self.resolved_watch_sources.remove(&snapshot.root.root_id);
        } else {
            self.resolved_watch_sources
                .insert(snapshot.root.root_id.clone(), sources);
        }
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
        let affected_root_ids = snapshot
            .roots
            .iter()
            .map(|root| root.root_id.clone())
            .collect::<Vec<_>>();
        let persistent_snapshot = self
            .catalog_snapshot_for_policy(snapshot.as_ref(), root_allows_persistent_catalog_cache);
        if let Some((pending_snapshot, pending_update_result)) = self
            .merged_pending_catalog_persistence(persistent_snapshot, update_check_result.as_deref())
        {
            self.pending_update_check_persistence = pending_update_result;
            self.pending_catalog_persistence = Some(pending_snapshot);
            self.catalog_persistence_is_current = false;
        }

        if !self.memory_cache_enabled() {
            self.catalog_snapshot = None;
            self.update_check_result = None;
            self.last_memory_cache_accessed_at = None;
            for root_id in affected_root_ids {
                self.refresh_cache_unavailable_revision(&root_id);
            }
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
        for root_id in affected_root_ids {
            self.refresh_cache_unavailable_revision(&root_id);
        }
        self.enforce_memory_node_limit();
        self.touch_memory_cache_if_needed();
    }

    fn merged_pending_catalog_persistence(
        &self,
        refreshed: ScriptMetaCatalogSnapshot,
        refreshed_update_result: Option<&UpdateCheckResult>,
    ) -> Option<(
        Arc<ScriptMetaCatalogSnapshot>,
        Option<Arc<UpdateCheckResult>>,
    )> {
        let refreshed_root_ids = refreshed
            .roots
            .iter()
            .filter(|root| matches!(root.status, RootStatus::Ready | RootStatus::Missing))
            .map(|root| root.root_id.clone())
            .collect::<BTreeSet<_>>();
        if refreshed_root_ids.is_empty() {
            return None;
        }

        let refreshed_roots = refreshed
            .roots
            .iter()
            .filter(|root| refreshed_root_ids.contains(&root.root_id))
            .cloned()
            .collect::<Vec<_>>();
        let mut refreshed_candidate_cache = refreshed.candidate_cache.clone();
        refreshed_candidate_cache
            .records
            .retain(|record| refreshed_root_ids.contains(&record.root_id));
        let (roots, candidate_cache) =
            if let Some(previous) = self.pending_catalog_persistence.as_ref() {
                (
                    merged_catalog_roots(previous, refreshed_roots, &refreshed_root_ids),
                    merged_candidate_cache(
                        previous,
                        refreshed_candidate_cache,
                        &refreshed_root_ids,
                        &self.roots,
                        root_allows_persistent_catalog_cache,
                    ),
                )
            } else {
                (refreshed_roots, refreshed_candidate_cache)
            };
        let file_items = file_items_from_cache(&candidate_cache);
        let snapshot = Arc::new(ScriptMetaCatalogSnapshot {
            source_revision: refreshed.source_revision,
            roots,
            all_items: deduplicated_items(&file_items),
            file_items,
            candidate_cache,
        });

        let unaffected_previous_items = self
            .pending_catalog_persistence
            .iter()
            .flat_map(|previous| previous.file_items.iter())
            .filter(|item| !refreshed_root_ids.contains(&item.root_id))
            .cloned()
            .collect::<Vec<_>>();
        let mut merged_update_result = self
            .pending_update_check_persistence
            .as_ref()
            .and_then(|result| filter_update_result_to_items(result, &unaffected_previous_items))
            .map(|result| result.as_ref().clone());
        let refreshed_items = snapshot
            .file_items
            .iter()
            .filter(|item| refreshed_root_ids.contains(&item.root_id))
            .cloned()
            .collect::<Vec<_>>();
        if let Some(result) = refreshed_update_result
            .and_then(|result| filter_update_result_to_items(result, &refreshed_items))
        {
            merge_update_check_result(&mut merged_update_result, &result);
        }
        let merged_update_result = merged_update_result
            .as_ref()
            .and_then(|result| filter_update_result_to_items(result, &snapshot.file_items));
        Some((snapshot, merged_update_result))
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
        self.catalog_snapshot_for_policy_excluding(snapshot, allows_root, &BTreeSet::new())
    }

    fn catalog_snapshot_for_policy_excluding(
        &self,
        snapshot: &ScriptMetaCatalogSnapshot,
        allows_root: impl Fn(&RootRegistration) -> bool,
        excluded_root_ids: &BTreeSet<RootId>,
    ) -> ScriptMetaCatalogSnapshot {
        let allowed_root_ids: BTreeSet<_> = self
            .roots
            .iter()
            .filter(|root| allows_root(root))
            .filter(|root| !excluded_root_ids.contains(&root.root_id))
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
                .filter(|root| {
                    allowed_root_ids.contains(root.root_id.as_ref())
                        && self.root_by_id(&root.root_id).is_some_and(|registration| {
                            normalize_path(&registration.path) == normalize_path(&root.path)
                        })
                })
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
        if let Some(snapshot) = self.file_list_snapshots.remove(root_id) {
            self.ensure_cache_unavailable_revision(root_id);
            if self
                .root_by_id(root_id)
                .is_some_and(root_allows_persistent_file_list_cache)
                && self.persisted_file_list_revisions.get(root_id)
                    != Some(&snapshot.content_revision)
            {
                self.pending_file_list_persistence
                    .insert(root_id.clone(), snapshot);
            }
            self.evicted_file_list_roots.insert(root_id.clone());
        }
        if let Some(previous) = self.catalog_snapshot.clone() {
            if previous.roots.iter().any(|root| root.root_id == *root_id)
                || previous
                    .candidate_cache
                    .records
                    .iter()
                    .any(|record| record.root_id == *root_id)
            {
                self.ensure_cache_unavailable_revision(root_id);
                self.evicted_catalog_roots.insert(root_id.clone());
                if !self.catalog_persistence_is_current
                    && self.pending_catalog_persistence.is_none()
                {
                    self.pending_catalog_persistence = Some(Arc::clone(&previous));
                    self.pending_update_check_persistence = self.update_check_result.clone();
                }
            }
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

    fn mark_resident_memory_cache_evicted(&mut self) {
        let resident_root_ids = self
            .file_list_snapshots
            .keys()
            .cloned()
            .chain(
                self.catalog_snapshot
                    .iter()
                    .flat_map(|snapshot| snapshot.roots.iter().map(|root| root.root_id.clone())),
            )
            .collect::<BTreeSet<_>>();
        for root_id in resident_root_ids {
            let revision = self.issue_revision();
            if let Some(root) = self.root_snapshots.get_mut(&root_id) {
                root.state_revision = revision;
            }
        }
        for (root_id, snapshot) in &self.file_list_snapshots {
            if self
                .root_by_id(root_id)
                .is_some_and(root_allows_persistent_file_list_cache)
                && self.persisted_file_list_revisions.get(root_id)
                    != Some(&snapshot.content_revision)
            {
                self.pending_file_list_persistence
                    .insert(root_id.clone(), Arc::clone(snapshot));
            }
        }
        if !self.catalog_persistence_is_current
            && self.pending_catalog_persistence.is_none()
            && let Some(snapshot) = self.catalog_snapshot.as_ref()
        {
            self.pending_catalog_persistence = Some(Arc::clone(snapshot));
            self.pending_update_check_persistence = self.update_check_result.clone();
        }
        self.evicted_file_list_roots
            .extend(self.file_list_snapshots.keys().cloned());
        if let Some(snapshot) = self.catalog_snapshot.as_ref() {
            self.evicted_catalog_roots
                .extend(snapshot.roots.iter().map(|root| root.root_id.clone()));
            self.evicted_catalog_roots.extend(
                snapshot
                    .candidate_cache
                    .records
                    .iter()
                    .map(|record| record.root_id.clone()),
            );
        }
    }

    fn expire_idle_memory_cache(&mut self) {
        if !self.memory_cache_enabled() {
            self.clear_memory_cache();
            return;
        }
        let idle_lifetime = self.config.cache.idle_lifetime_millis;
        if idle_lifetime == 0 {
            self.mark_resident_memory_cache_evicted();
            self.clear_memory_cache();
            return;
        }
        let Some(last_accessed_at) = self.last_memory_cache_accessed_at else {
            return;
        };
        let now = now_timestamp_millis();
        if now.saturating_sub(last_accessed_at) >= idle_lifetime {
            self.mark_resident_memory_cache_evicted();
            self.clear_memory_cache();
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

fn root_state_equal(lhs: &RootSnapshot, rhs: &RootSnapshot) -> bool {
    lhs.root_id == rhs.root_id
        && lhs.path == rhs.path
        && lhs.status == rhs.status
        && lhs.is_dirty == rhs.is_dirty
        && lhs.last_loaded_at == rhs.last_loaded_at
        && lhs.last_event_at == rhs.last_event_at
        && lhs.item_count == rhs.item_count
        && lhs.error == rhs.error
}

fn catalog_root_content_equal(
    lhs: &ScriptMetaCatalogSnapshot,
    rhs: &ScriptMetaCatalogSnapshot,
    root_id: &RootId,
) -> bool {
    let lhs_root = lhs.roots.iter().find(|root| root.root_id == *root_id);
    let rhs_root = rhs.roots.iter().find(|root| root.root_id == *root_id);
    if !lhs_root
        .zip(rhs_root)
        .is_some_and(|(lhs, rhs)| root_state_equal(lhs, rhs))
    {
        return false;
    }
    lhs.candidate_cache
        .records
        .iter()
        .filter(|record| record.root_id == *root_id)
        .eq(rhs
            .candidate_cache
            .records
            .iter()
            .filter(|record| record.root_id == *root_id))
}

fn file_list_content_equal(lhs: &FileListSnapshot, rhs: &FileListSnapshot) -> bool {
    lhs.children == rhs.children && lhs.truncated == rhs.truncated
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

fn merged_roots_from_groups(
    root_groups: &BTreeMap<String, BTreeMap<RootId, RootRegistration>>,
) -> ScriptMetaKitResult<Vec<RootRegistration>> {
    let mut merged_roots_by_id: BTreeMap<RootId, RootRegistration> = BTreeMap::new();
    for group_roots in root_groups.values() {
        for root in group_roots.values() {
            if let Some(existing) = merged_roots_by_id.get_mut(&root.root_id) {
                *existing = merged_root_registration(existing, root)?;
            } else {
                merged_roots_by_id.insert(root.root_id.clone(), root.clone());
            }
        }
    }
    Ok(merged_roots_by_id.into_values().collect())
}

fn merged_root_registration(
    existing: &RootRegistration,
    next: &RootRegistration,
) -> ScriptMetaKitResult<RootRegistration> {
    if normalize_path(&existing.path) != normalize_path(&next.path) {
        return Err(ScriptMetaKitError::InvalidConfig(format!(
            "root_id `{}` is registered with conflicting paths `{}` and `{}`",
            existing.root_id,
            existing.path.display(),
            next.path.display()
        )));
    }
    Ok(RootRegistration {
        root_id: next.root_id.clone(),
        path: merged_root_path(existing, next),
        display_name: merged_display_name(existing, next),
        purpose: merged_root_purpose(existing.purpose, next.purpose),
        watch_policy: merged_watch_policy(existing.watch_policy, next.watch_policy),
        cache_policy: merged_cache_policy(existing.cache_policy, next.cache_policy),
        refresh_policy: merged_refresh_policy(existing.refresh_policy, next.refresh_policy),
        priority: merged_root_priority(existing.priority, next.priority),
    })
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
    if matches!(
        lhs,
        RefreshPolicy::OnFileEvent | RefreshPolicy::OnFileEventDeferred
    ) || matches!(
        rhs,
        RefreshPolicy::OnFileEvent | RefreshPolicy::OnFileEventDeferred
    ) {
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

fn root_scan_priority(root: &RootRegistration, visible_root_id: Option<&RootId>) -> u8 {
    match root.priority {
        RootPriority::UserInitiated => 0,
        RootPriority::VisibleWhenSelected if visible_root_id == Some(&root.root_id) => 1,
        RootPriority::VisibleWhenSelected | RootPriority::Background => 2,
    }
}

const fn canonical_cache_scope(scope: CacheScope) -> CacheScope {
    match scope {
        CacheScope::Root => CacheScope::FileList,
        scope => scope,
    }
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

fn merge_update_check_result(target: &mut Option<UpdateCheckResult>, source: &UpdateCheckResult) {
    let Some(target) = target.as_mut() else {
        *target = Some(source.clone());
        return;
    };
    target.checked_at = target.checked_at.max(source.checked_at);
    target.operation = source.operation.clone();
    target
        .resolutions_by_item_id
        .extend(source.resolutions_by_item_id.clone());
    target
        .failures_by_item_id
        .extend(source.failures_by_item_id.clone());
    target
        .errors_by_item_id
        .extend(source.errors_by_item_id.clone());
    target
        .statuses_by_item_id
        .extend(source.statuses_by_item_id.clone());
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

#[derive(Clone, Debug, Serialize, Deserialize)]
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

fn collect_resolved_directory_link_sources(
    entries: &[FileSystemEntry],
    output: &mut BTreeSet<PathBuf>,
) {
    for entry in entries {
        if entry.is_directory
            && entry.path_kind != PathKind::Normal
            && entry.resolution_status == PathResolutionStatus::Resolved
        {
            output.insert(entry.display_path.clone());
        }
        collect_resolved_directory_link_sources(&entry.children, output);
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
    use std::{collections::BTreeMap, path::Path, path::PathBuf, sync::Arc};

    use url::Url;

    use super::{
        UpdateCheckCacheMode, apply_metadata_capabilities_to_file_list_snapshots,
        decode_all_cache_data, file_list_content_equal, merged_root_purpose, root_scan_priority,
        update_retry_delay, update_work_groups,
    };
    use crate::{
        RootId,
        catalog::{
            CachePolicy, DirectoryState, FileListSnapshot, RefreshPolicy, RootPriority,
            RootPurpose, RootRegistration, RootSnapshot, UpdateCheckResult,
        },
        core::{OperationSummary, ScriptMetaEditState, ScriptMetaItem},
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
        assert_eq!(merged.refresh_policy, RefreshPolicy::OnFileEvent);
        assert_eq!(merged.priority, RootPriority::UserInitiated);

        engine
            .replace_root_group("file-list", Vec::new())
            .expect("remove file list roots");

        assert_eq!(engine.roots(), &[metadata_root]);
    }

    #[test]
    fn failed_set_roots_preserves_existing_root_groups() {
        let mut engine = super::ScriptMetaKitEngine::new(Default::default()).expect("engine");
        let first = RootRegistration::file_list_and_metadata("first", "/tmp/first");
        let second = RootRegistration::file_list_and_metadata("second", "/tmp/second");
        engine
            .replace_root_group("existing", vec![first.clone()])
            .expect("existing group");

        assert!(engine.set_roots(vec![second.clone(), second]).is_err());
        engine
            .replace_root_group(
                "additional",
                vec![RootRegistration::file_list_and_metadata(
                    "third",
                    "/tmp/third",
                )],
            )
            .expect("additional group");

        assert!(engine.roots().contains(&first));
        assert_eq!(engine.roots().len(), 2);
    }

    #[test]
    fn conflicting_paths_for_the_same_grouped_root_id_are_rejected_atomically() {
        let mut engine = super::ScriptMetaKitEngine::new(Default::default()).expect("engine");
        let first = RootRegistration::file_list_and_metadata("shared", "/tmp/first");
        let second = RootRegistration::file_list_and_metadata("shared", "/tmp/second");
        engine
            .replace_root_group("first", vec![first.clone()])
            .expect("first group");

        let error = engine
            .replace_root_group("second", vec![second])
            .expect_err("conflicting path");

        assert!(error.to_string().contains("conflicting paths"));
        assert_eq!(engine.roots(), &[first]);
        engine
            .replace_root_group(
                "third",
                vec![RootRegistration::file_list_and_metadata(
                    "additional",
                    "/tmp/additional",
                )],
            )
            .expect("valid group after rejection");
        assert_eq!(engine.roots().len(), 2);
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
    fn root_scan_priority_prefers_user_work_then_the_selected_visible_root() {
        let root = |root_id: &str, priority| RootRegistration {
            root_id: RootId::from(root_id),
            path: PathBuf::from(format!("/tmp/{root_id}")),
            display_name: None,
            purpose: RootPurpose::FileList,
            watch_policy: WatchPolicy::Disabled,
            cache_policy: CachePolicy::MemoryOnly,
            refresh_policy: RefreshPolicy::ManualOnly,
            priority,
        };
        let user = root("user", RootPriority::UserInitiated);
        let visible = root("visible", RootPriority::VisibleWhenSelected);
        let background = root("background", RootPriority::Background);
        let visible_id = RootId::from("visible");

        assert!(
            root_scan_priority(&user, Some(&visible_id))
                < root_scan_priority(&visible, Some(&visible_id))
        );
        assert!(
            root_scan_priority(&visible, Some(&visible_id))
                < root_scan_priority(&background, Some(&visible_id))
        );
        assert_eq!(
            root_scan_priority(&visible, None),
            root_scan_priority(&background, None)
        );
    }

    #[test]
    fn storing_update_results_marks_catalog_persistence_dirty() {
        let mut engine = super::ScriptMetaKitEngine::new(Default::default()).expect("engine");
        engine.catalog_persistence_is_current = true;
        engine.store_update_check_result(
            &[],
            Arc::new(UpdateCheckResult {
                checked_at: 1,
                operation: OperationSummary::default(),
                resolutions_by_item_id: BTreeMap::new(),
                failures_by_item_id: BTreeMap::new(),
                errors_by_item_id: BTreeMap::new(),
                statuses_by_item_id: BTreeMap::new(),
            }),
            UpdateCheckCacheMode::Replace,
        );

        assert!(!engine.catalog_persistence_is_current);
    }

    #[test]
    fn eviction_does_not_retain_an_update_result_without_a_catalog() {
        let mut engine = super::ScriptMetaKitEngine::new(Default::default()).expect("engine");
        engine.update_check_result = Some(Arc::new(UpdateCheckResult {
            checked_at: 1,
            operation: OperationSummary::default(),
            resolutions_by_item_id: BTreeMap::new(),
            failures_by_item_id: BTreeMap::new(),
            errors_by_item_id: BTreeMap::new(),
            statuses_by_item_id: BTreeMap::new(),
        }));
        engine.mark_resident_memory_cache_evicted();

        assert!(engine.pending_catalog_persistence.is_none());
        assert!(engine.pending_update_check_persistence.is_none());
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
            content_revision: Default::default(),
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

    #[test]
    fn content_projection_ignores_directory_states_but_includes_tree_and_truncation() {
        let root = RootSnapshot::new(RootId::from("root"), PathBuf::from("/tmp/root"));
        let path = PathBuf::from("/tmp/root/Tool.jsx");
        let baseline = FileListSnapshot {
            root,
            children: Some(vec![file_entry(&path)]),
            directory_states: Default::default(),
            truncated: false,
            content_revision: Default::default(),
        };
        let mut directory_state_only = baseline.clone();
        directory_state_only.directory_states.insert(
            "/tmp/root".to_string(),
            DirectoryState {
                modification_time_millis: Some(1),
                child_count: 1,
                child_fingerprint: 42,
                identity: None,
            },
        );
        assert!(file_list_content_equal(&baseline, &directory_state_only));

        let mut changed_tree = baseline.clone();
        changed_tree.children.as_mut().expect("children")[0].display_path =
            PathBuf::from("/tmp/root/Renamed.jsx");
        assert!(!file_list_content_equal(&baseline, &changed_tree));

        let mut changed_truncation = baseline.clone();
        changed_truncation.truncated = true;
        assert!(!file_list_content_equal(&baseline, &changed_truncation));
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
