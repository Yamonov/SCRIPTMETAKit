use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    ItemId, RootId, TimestampMillis,
    core::{
        DistributionResolution, FileIssue, OperationSummary, ScriptMetaItem, ScriptMetaItemRef,
        ScriptMetaKitError, ScriptRuntimeKind,
    },
    scanner::{CandidateCache, FileSystemEntry, PathKind, PathResolutionStatus},
};

pub type DirectoryStateMap = BTreeMap<String, DirectoryState>;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SnapshotRevision {
    #[serde(default)]
    pub workspace_epoch: String,
    #[serde(default)]
    pub sequence: u64,
}

impl SnapshotRevision {
    #[must_use]
    pub fn is_available(&self) -> bool {
        !self.workspace_epoch.is_empty() && self.sequence > 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DirectoryState {
    pub modification_time_millis: Option<TimestampMillis>,
    pub child_count: usize,
    pub child_fingerprint: u64,
    #[serde(default)]
    pub identity: Option<FileIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileIdentity {
    pub stable_id: String,
    #[serde(default)]
    pub volume_id: Option<String>,
    #[serde(default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub content_modified_at: Option<TimestampMillis>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RootSnapshot {
    pub root_id: RootId,
    pub path: PathBuf,
    pub status: RootStatus,
    pub is_dirty: bool,
    pub last_loaded_at: Option<TimestampMillis>,
    pub last_event_at: Option<TimestampMillis>,
    pub item_count: usize,
    pub error: Option<RootError>,
    #[serde(default)]
    pub state_revision: SnapshotRevision,
}

impl RootSnapshot {
    #[must_use]
    pub fn new(root_id: RootId, path: PathBuf) -> Self {
        Self {
            root_id,
            path,
            status: RootStatus::NotLoaded,
            is_dirty: true,
            last_loaded_at: None,
            last_event_at: None,
            item_count: 0,
            error: None,
            state_revision: SnapshotRevision::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootStatus {
    NotLoaded,
    Ready,
    Dirty,
    Loading,
    Missing,
    Unreadable,
    TimedOut,
    Overflowed,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RootError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileListSnapshot {
    pub root: RootSnapshot,
    pub children: Option<Vec<FileSystemEntry>>,
    pub directory_states: DirectoryStateMap,
    pub truncated: bool,
    #[serde(default)]
    pub content_revision: SnapshotRevision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetaCatalogSnapshot {
    pub source_revision: Uuid,
    pub roots: Vec<RootSnapshot>,
    pub all_items: Vec<ScriptMetaItemRef>,
    pub file_items: Vec<ScriptMetaItemRef>,
    pub candidate_cache: CandidateCache,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScanRequest {
    pub root_ids: Vec<RootId>,
    pub mode: ScanMode,
}

impl ScanRequest {
    #[must_use]
    pub fn all(mode: ScanMode) -> Self {
        Self {
            root_ids: Vec::new(),
            mode,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RefreshRequest {
    pub mode: ScanMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanMode {
    FileListOnly,
    MetadataOnly,
    FileListAndMetadata,
}

impl ScanMode {
    #[must_use]
    pub fn includes_file_list(self) -> bool {
        matches!(self, Self::FileListOnly | Self::FileListAndMetadata)
    }

    #[must_use]
    pub fn includes_metadata(self) -> bool {
        matches!(self, Self::MetadataOnly | Self::FileListAndMetadata)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScanResult {
    pub roots: Vec<RootSnapshot>,
    pub file_list_snapshots: Vec<Arc<FileListSnapshot>>,
    pub catalog_snapshot: Option<Arc<ScriptMetaCatalogSnapshot>>,
    #[serde(default)]
    pub operation: OperationSummary,
    #[serde(default)]
    pub file_issues: Vec<FileIssue>,
    #[serde(default)]
    pub update_check_result: Option<Arc<UpdateCheckResult>>,
    #[serde(default)]
    pub change_summary: Option<ScanChangeSummary>,
    #[serde(default)]
    pub watch_change_batch: Option<crate::watcher::RootChangeBatch>,
    #[serde(default)]
    pub watch_reconciliation: bool,
    #[serde(default)]
    pub watch_covers_all_roots: bool,
}

impl ScanResult {
    pub fn flattened_file_entries(&self) -> impl Iterator<Item = &FileSystemEntry> {
        self.file_list_snapshots
            .iter()
            .filter_map(|snapshot| snapshot.children.as_deref())
            .flat_map(flatten_file_entries)
    }
}

fn flatten_file_entries(
    entries: &[FileSystemEntry],
) -> Box<dyn Iterator<Item = &FileSystemEntry> + '_> {
    Box::new(
        entries
            .iter()
            .flat_map(|entry| std::iter::once(entry).chain(flatten_file_entries(&entry.children))),
    )
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScanChangeSummary {
    pub added_count: usize,
    pub removed_count: usize,
    pub modified_count: usize,
    pub changes: Vec<FileEntryChange>,
}

impl ScanChangeSummary {
    pub fn extend(&mut self, other: Self) {
        self.added_count += other.added_count;
        self.removed_count += other.removed_count;
        self.modified_count += other.modified_count;
        self.changes.extend(other.changes);
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileEntryChange {
    pub root_id: RootId,
    pub kind: FileEntryChangeKind,
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
    pub content_modified_at: Option<TimestampMillis>,
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
    pub identity: Option<FileIdentity>,
}

impl FileEntryChange {
    #[must_use]
    pub fn from_entry(root_id: RootId, kind: FileEntryChangeKind, entry: &FileSystemEntry) -> Self {
        Self {
            root_id,
            kind,
            display_path: entry.display_path.clone(),
            resolved_path: entry.resolved_path.clone(),
            path_kind: entry.path_kind,
            resolution_status: entry.resolution_status,
            resolution_message: entry.resolution_message.clone(),
            is_directory: entry.is_directory,
            file_size: entry.file_size,
            content_modified_at: entry.content_modified_at,
            runtime_kind: entry.runtime_kind,
            shebang: entry.shebang.clone(),
            has_scriptmeta: entry.has_scriptmeta,
            has_scriptmeta_edit_password: entry.has_scriptmeta_edit_password,
            is_file_locked: entry.is_file_locked,
            is_read_only: entry.is_read_only,
            can_edit_scriptmeta: entry.can_edit_scriptmeta,
            can_append_scriptmeta: entry.can_append_scriptmeta,
            scriptmeta_edit_state: entry.scriptmeta_edit_state,
            identity: entry.identity.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileEntryChangeKind {
    Added,
    Removed,
    Modified,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateCheckRequest {
    pub items: Vec<ScriptMetaItemRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub checked_at: TimestampMillis,
    #[serde(default)]
    pub operation: OperationSummary,
    pub resolutions_by_item_id: BTreeMap<ItemId, DistributionResolution>,
    #[serde(default)]
    pub failures_by_item_id: BTreeMap<ItemId, UpdateFailure>,
    pub errors_by_item_id: BTreeMap<ItemId, String>,
    pub statuses_by_item_id: BTreeMap<ItemId, UpdateStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateFailure {
    pub code: String,
    pub message: String,
    pub item_id: ItemId,
    pub file_path: PathBuf,
    pub script_id: String,
    pub current_version: Option<String>,
    pub meta_url: Option<Url>,
    pub source_url: Option<Url>,
    pub checked_at: TimestampMillis,
}

impl UpdateFailure {
    #[must_use]
    pub fn from_error(
        item_id: ItemId,
        item: &ScriptMetaItem,
        checked_at: TimestampMillis,
        error: &ScriptMetaKitError,
    ) -> Self {
        Self::from_message(
            item_id,
            item,
            checked_at,
            error.code(),
            error.to_string(),
            None,
        )
    }

    #[must_use]
    pub fn unresolved_distribution(
        item_id: ItemId,
        item: &ScriptMetaItem,
        resolution: &DistributionResolution,
    ) -> Self {
        let message = resolution
            .note
            .clone()
            .unwrap_or_else(|| "distribution metadata could not be resolved".to_string());
        Self::from_message(
            item_id,
            item,
            resolution.checked_at,
            "unresolved_distribution",
            message,
            Some(resolution.final_page_url.clone()),
        )
    }

    #[must_use]
    pub fn from_message(
        item_id: ItemId,
        item: &ScriptMetaItem,
        checked_at: TimestampMillis,
        code: impl Into<String>,
        message: impl Into<String>,
        source_url: Option<Url>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            item_id,
            file_path: item.file_path.clone(),
            script_id: item.script_id.clone(),
            current_version: item.version.clone(),
            meta_url: item.meta_url.clone(),
            source_url,
            checked_at,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    UpdateAvailable,
    Failed,
    NotCheckable,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheScope {
    All,
    Catalog,
    FileList,
    Root,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CacheInvalidationReason {
    pub code: String,
    pub message: String,
}

impl CacheInvalidationReason {
    #[must_use]
    pub fn manual(message: impl Into<String>) -> Self {
        Self {
            code: "manual".to_string(),
            message: message.into(),
        }
    }
}

#[must_use]
pub fn unresolved_distribution(
    url: Url,
    checked_at: TimestampMillis,
    note: String,
) -> DistributionResolution {
    DistributionResolution {
        latest_version: None,
        latest_page_url: None,
        final_page_url: url,
        latest_url_history: Vec::new(),
        checked_at,
        is_unresolved: true,
        note: Some(note),
        redirect_count: None,
    }
}
