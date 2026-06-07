#![deny(unsafe_op_in_unsafe_fn)]

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    ptr, slice, str,
    sync::Arc,
};

use scriptmetakit::scanner::CandidateRecord;
use scriptmetakit::{
    CachePolicy, CacheScope, CommentSyntax,
    DistributionMetadataDraft as KitDistributionMetadataDraft, DistributionResolution,
    FileEntryChange, FileEntryChangeKind, FileIdentity, FileIssue, FileListSnapshot,
    FileSystemEntry, IgnoredWatchPath, OperationSummary, RefreshPolicy, RootChangeBatch, RootId,
    RootPriority, RootPurpose, RootRegistration, RootSnapshot, RootStatus, ScanChangeSummary,
    ScanMode, ScanRequest, ScanResult, ScriptFileInspection, ScriptIdUniquenessItem,
    ScriptMetaBackupGeneration as KitScriptMetaBackupGeneration, ScriptMetaBackupOptions,
    ScriptMetaBackupReason, ScriptMetaBackupRecord as KitScriptMetaBackupRecord,
    ScriptMetaCatalogSnapshot, ScriptMetaEditState, ScriptMetaItem, ScriptMetaKitConfig,
    ScriptMetaKitEngine, ScriptMetaWriteMode, ScriptMetaWriteOperation,
    ScriptMetadataDraft as KitScriptMetadataDraft,
    ScriptMetadataEditPreviewResult as KitScriptMetadataEditPreviewResult,
    ScriptMetadataEditReadResult as KitScriptMetadataEditReadResult,
    ScriptMetadataFileWriteResult as KitScriptMetadataFileWriteResult, ScriptRuntimeKind,
    UpdateCheckProgress, UpdateCheckProgressPhase, UpdateCheckResult, UpdateFailure, UpdateStatus,
    WatchIgnoreReason, WatchPathEvent, WatchPathEventKind, WatchPolicy, WatchRenameCandidate,
    WatchRenameConfidence, WatchRescanReason, WatchRescanTarget, can_read_directory_contents,
    clear_scriptmeta_backups, create_scriptmeta_backup, generate_edit_password_sha256,
    inspect_script_file, is_valid_edit_password_sha256, load_cache_payload, normalize_metadata_url,
    normalize_version_string, read_script_metadata_draft_from_file,
    read_script_metadata_edit_preview_from_file, render_distribution_metadata_block,
    reset_scriptmeta_backups_with_current_as_initial, restore_scriptmeta_backup,
    save_cache_payload, script_path_may_affect_metadata, scriptmeta_backup_generations,
    supported_script_extensions_text, validate_script_id_uniqueness, verify_edit_password_sha256,
    write_script_metadata_to_file,
};
use url::Url;

#[cfg(feature = "native-watch")]
use scriptmetakit::{NativeWatcher, RefreshRequest};

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmkStatus {
    Ok = 0,
    NullArgument = 1,
    InvalidUtf8 = 2,
    InvalidArgument = 3,
    EngineError = 4,
    Panic = 5,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SmkUtf8Slice {
    pub ptr: *const u8,
    pub len: usize,
}

impl SmkUtf8Slice {
    const fn empty() -> Self {
        Self {
            ptr: ptr::null(),
            len: 0,
        }
    }
}

impl Default for SmkUtf8Slice {
    fn default() -> Self {
        Self::empty()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkRootSnapshot {
    pub root_id: SmkUtf8Slice,
    pub path: SmkUtf8Slice,
    pub status: SmkUtf8Slice,
    pub is_dirty: u8,
    pub has_last_loaded_at: u8,
    pub last_loaded_at: u64,
    pub has_last_event_at: u8,
    pub last_event_at: u64,
    pub item_count: usize,
    pub error_code: SmkUtf8Slice,
    pub error_message: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkRootRegistration {
    pub root_id: SmkUtf8Slice,
    pub path: SmkUtf8Slice,
    pub display_name: SmkUtf8Slice,
    pub purpose: u32,
    pub watch_policy: u32,
    pub cache_policy: u32,
    pub refresh_policy: u32,
    pub priority: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkRegisteredRootSignature {
    pub root_id: SmkUtf8Slice,
    pub path: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkCatalogInfo {
    pub has_catalog: u8,
    pub source_revision: SmkUtf8Slice,
    pub candidate_cache_schema_version: u32,
    pub candidate_cache_built_at: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkFileIdentity {
    pub stable_id: SmkUtf8Slice,
    pub volume_id: SmkUtf8Slice,
    pub file_id: SmkUtf8Slice,
    pub has_file_size: u8,
    pub file_size: u64,
    pub has_content_modified_at: u8,
    pub content_modified_at: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptFileInspection {
    pub is_supported_script_path: u8,
    pub runtime_kind: SmkUtf8Slice,
    pub shebang: SmkUtf8Slice,
    pub comment_syntax: SmkUtf8Slice,
    pub supports_inline_scriptmeta_editing: u8,
    pub is_file_locked: u8,
    pub is_read_only: u8,
    pub can_edit_scriptmeta: u8,
    pub can_append_scriptmeta: u8,
    pub scriptmeta_edit_state: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkFileEntry {
    pub display_path: SmkUtf8Slice,
    pub resolved_path: SmkUtf8Slice,
    pub path_kind: SmkUtf8Slice,
    pub resolution_status: SmkUtf8Slice,
    pub resolution_message: SmkUtf8Slice,
    pub is_directory: u8,
    pub has_file_size: u8,
    pub file_size: u64,
    pub has_content_modified_at: u8,
    pub content_modified_at: u64,
    pub has_identity: u8,
    pub identity: SmkFileIdentity,
    pub runtime_kind: SmkUtf8Slice,
    pub shebang: SmkUtf8Slice,
    pub has_scriptmeta: u8,
    pub has_scriptmeta_edit_password: u8,
    pub is_file_locked: u8,
    pub is_read_only: u8,
    pub can_edit_scriptmeta: u8,
    pub can_append_scriptmeta: u8,
    pub scriptmeta_edit_state: SmkUtf8Slice,
    pub has_scriptmeta_item: u8,
    pub scriptmeta_item: SmkScriptItem,
    pub first_child_index: usize,
    pub child_count: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkFileListSnapshot {
    pub root_index: usize,
    pub first_child_index: usize,
    pub child_count: usize,
    pub truncated: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptItem {
    pub root_id: SmkUtf8Slice,
    pub file_path: SmkUtf8Slice,
    pub identity_path: SmkUtf8Slice,
    pub runtime_kind: SmkUtf8Slice,
    pub shebang: SmkUtf8Slice,
    pub script_id: SmkUtf8Slice,
    pub version: SmkUtf8Slice,
    pub name: SmkUtf8Slice,
    pub description: SmkUtf8Slice,
    pub target_app: SmkUtf8Slice,
    pub min_target_version: SmkUtf8Slice,
    pub meta_url: SmkUtf8Slice,
    pub author: SmkUtf8Slice,
    pub release_date: SmkUtf8Slice,
    pub edit_password_sha256: SmkUtf8Slice,
    pub has_scriptmeta: u8,
    pub has_scriptmeta_edit_password: u8,
    pub is_file_locked: u8,
    pub is_read_only: u8,
    pub can_edit_scriptmeta: u8,
    pub can_append_scriptmeta: u8,
    pub scriptmeta_edit_state: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptIdUniquenessItem {
    pub item_id: SmkUtf8Slice,
    pub file_path: SmkUtf8Slice,
    pub script_id: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkCandidateRecord {
    pub root_id: SmkUtf8Slice,
    pub root_path: SmkUtf8Slice,
    pub file_path: SmkUtf8Slice,
    pub identity_path: SmkUtf8Slice,
    pub path_kind: SmkUtf8Slice,
    pub resolution_status: SmkUtf8Slice,
    pub resolution_message: SmkUtf8Slice,
    pub runtime_kind: SmkUtf8Slice,
    pub shebang: SmkUtf8Slice,
    pub has_scriptmeta: u8,
    pub has_scriptmeta_edit_password: u8,
    pub is_file_locked: u8,
    pub is_read_only: u8,
    pub can_edit_scriptmeta: u8,
    pub can_append_scriptmeta: u8,
    pub scriptmeta_edit_state: SmkUtf8Slice,
    pub has_file_size: u8,
    pub file_size: u64,
    pub has_content_modified_at: u8,
    pub content_modified_at: u64,
    pub has_item: u8,
    pub item: SmkScriptItem,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkUpdateCheckInfo {
    pub has_update_check: u8,
    pub checked_at: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkUpdateStatusEntry {
    pub item_id: SmkUtf8Slice,
    pub status: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkDistributionResolutionEntry {
    pub item_id: SmkUtf8Slice,
    pub latest_version: SmkUtf8Slice,
    pub latest_page_url: SmkUtf8Slice,
    pub final_page_url: SmkUtf8Slice,
    pub first_latest_url_history_index: usize,
    pub latest_url_history_count: usize,
    pub checked_at: u64,
    pub is_unresolved: u8,
    pub note: SmkUtf8Slice,
    pub has_redirect_count: u8,
    pub redirect_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkUpdateFailureEntry {
    pub item_id: SmkUtf8Slice,
    pub code: SmkUtf8Slice,
    pub message: SmkUtf8Slice,
    pub file_path: SmkUtf8Slice,
    pub script_id: SmkUtf8Slice,
    pub current_version: SmkUtf8Slice,
    pub meta_url: SmkUtf8Slice,
    pub source_url: SmkUtf8Slice,
    pub checked_at: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkUpdateErrorEntry {
    pub item_id: SmkUtf8Slice,
    pub message: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkUpdateProgress {
    pub completed_items: usize,
    pub total_items: usize,
    pub item_id: SmkUtf8Slice,
    pub script_id: SmkUtf8Slice,
    pub phase: SmkUtf8Slice,
    pub message: SmkUtf8Slice,
}

pub type SmkUpdateProgressCallback =
    Option<extern "C" fn(progress: *const SmkUpdateProgress, context: *mut c_void)>;

pub type SmkWatchNotificationCallback = Option<extern "C" fn(context: *mut c_void)>;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScanChangeInfo {
    pub has_change_summary: u8,
    pub added_count: usize,
    pub removed_count: usize,
    pub modified_count: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkOperationInfo {
    pub status: SmkUtf8Slice,
    pub total_units: usize,
    pub completed_units: usize,
    pub failed_units: usize,
    pub cancelled: u8,
    pub timed_out: u8,
    pub reason_code: SmkUtf8Slice,
    pub message: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkFileIssue {
    pub has_root_id: u8,
    pub root_id: SmkUtf8Slice,
    pub path: SmkUtf8Slice,
    pub code: SmkUtf8Slice,
    pub message: SmkUtf8Slice,
    pub path_kind: SmkUtf8Slice,
    pub resolution_status: SmkUtf8Slice,
    pub is_directory: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkFileEntryChange {
    pub root_id: SmkUtf8Slice,
    pub kind: SmkUtf8Slice,
    pub display_path: SmkUtf8Slice,
    pub resolved_path: SmkUtf8Slice,
    pub path_kind: SmkUtf8Slice,
    pub resolution_status: SmkUtf8Slice,
    pub resolution_message: SmkUtf8Slice,
    pub is_directory: u8,
    pub has_file_size: u8,
    pub file_size: u64,
    pub has_content_modified_at: u8,
    pub content_modified_at: u64,
    pub has_identity: u8,
    pub identity: SmkFileIdentity,
    pub runtime_kind: SmkUtf8Slice,
    pub shebang: SmkUtf8Slice,
    pub has_scriptmeta: u8,
    pub has_scriptmeta_edit_password: u8,
    pub is_file_locked: u8,
    pub is_read_only: u8,
    pub can_edit_scriptmeta: u8,
    pub can_append_scriptmeta: u8,
    pub scriptmeta_edit_state: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkRootSnapshotSlice {
    pub ptr: *const SmkRootSnapshot,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkRegisteredRootSignatureSlice {
    pub ptr: *const SmkRegisteredRootSignature,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkFileListSnapshotSlice {
    pub ptr: *const SmkFileListSnapshot,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkFileEntrySlice {
    pub ptr: *const SmkFileEntry,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptItemSlice {
    pub ptr: *const SmkScriptItem,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptIdUniquenessReport {
    pub total_items: usize,
    pub unique_script_ids: usize,
    pub duplicate_count: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptIdDuplicate {
    pub script_id: SmkUtf8Slice,
    pub first_item_id_index: usize,
    pub item_id_count: usize,
    pub first_file_path_index: usize,
    pub file_path_count: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptIdDuplicateSlice {
    pub ptr: *const SmkScriptIdDuplicate,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkCandidateRecordSlice {
    pub ptr: *const SmkCandidateRecord,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkUpdateStatusEntrySlice {
    pub ptr: *const SmkUpdateStatusEntry,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkDistributionResolutionEntrySlice {
    pub ptr: *const SmkDistributionResolutionEntry,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkUpdateFailureEntrySlice {
    pub ptr: *const SmkUpdateFailureEntry,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkUpdateErrorEntrySlice {
    pub ptr: *const SmkUpdateErrorEntry,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkUtf8SliceSlice {
    pub ptr: *const SmkUtf8Slice,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkFileEntryChangeSlice {
    pub ptr: *const SmkFileEntryChange,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkFileIssueSlice {
    pub ptr: *const SmkFileIssue,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkWatchChangeInfo {
    pub has_watch_change: u8,
    pub overflowed: u8,
    pub path_count: usize,
    pub affected_root_count: usize,
    pub event_count: usize,
    pub ignored_path_count: usize,
    pub rename_candidate_count: usize,
    pub rescan_target_count: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkWatchPathEvent {
    pub root_id: SmkUtf8Slice,
    pub path: SmkUtf8Slice,
    pub kind: SmkUtf8Slice,
    pub is_directory: u8,
    pub rescan_directory: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkIgnoredWatchPath {
    pub has_root_id: u8,
    pub root_id: SmkUtf8Slice,
    pub path: SmkUtf8Slice,
    pub reason: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkWatchRenameCandidate {
    pub root_id: SmkUtf8Slice,
    pub old_path: SmkUtf8Slice,
    pub new_path: SmkUtf8Slice,
    pub confidence: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkWatchRescanTarget {
    pub root_id: SmkUtf8Slice,
    pub path: SmkUtf8Slice,
    pub reason: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkWatchPathEventSlice {
    pub ptr: *const SmkWatchPathEvent,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkIgnoredWatchPathSlice {
    pub ptr: *const SmkIgnoredWatchPath,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkWatchRenameCandidateSlice {
    pub ptr: *const SmkWatchRenameCandidate,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkWatchRescanTargetSlice {
    pub ptr: *const SmkWatchRescanTarget,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptMetadataDraft {
    pub script_id: SmkUtf8Slice,
    pub version: SmkUtf8Slice,
    pub description: SmkUtf8Slice,
    pub target_app: SmkUtf8Slice,
    pub min_target_version: SmkUtf8Slice,
    pub meta_url: SmkUtf8Slice,
    pub name: SmkUtf8Slice,
    pub author: SmkUtf8Slice,
    pub release_date: SmkUtf8Slice,
    pub edit_password_sha256: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptMetadataWriteRequest {
    pub file_path: SmkUtf8Slice,
    pub backup_root_path: SmkUtf8Slice,
    pub write_mode: u32,
    pub draft: SmkScriptMetadataDraft,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkDistributionMetadataDraft {
    pub script_id: SmkUtf8Slice,
    pub version: SmkUtf8Slice,
    pub latest_url: SmkUtf8Slice,
    pub latest_page_url: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptMetaBackupRecord {
    pub id: SmkUtf8Slice,
    pub created_at_millis: u64,
    pub backup_file_name: SmkUtf8Slice,
    pub backup_file_path: SmkUtf8Slice,
    pub file_size: u64,
    pub reason: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptMetadataFileWriteResult {
    pub file_path: SmkUtf8Slice,
    pub operation: SmkUtf8Slice,
    pub has_backup: u8,
    pub backup: SmkScriptMetaBackupRecord,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptMetadataEditReadResult {
    pub file_path: SmkUtf8Slice,
    pub draft: SmkScriptMetadataDraft,
    pub comment_style: SmkUtf8Slice,
    pub line_ending: SmkUtf8Slice,
    pub has_existing_block: u8,
    pub existing_block_text: SmkUtf8Slice,
    pub source_fingerprint: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptMetadataEditPreviewResult {
    pub file_path: SmkUtf8Slice,
    pub preview_text: SmkUtf8Slice,
    pub preview_byte_count: usize,
    pub file_size: u64,
    pub has_file_size: u8,
    pub comment_style: SmkUtf8Slice,
    pub line_ending: SmkUtf8Slice,
    pub has_scriptmeta_marker_in_preview: u8,
    pub is_truncated: u8,
    pub requires_full_read: u8,
    pub file_state_fingerprint: SmkUtf8Slice,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptMetaBackupGeneration {
    pub id: SmkUtf8Slice,
    pub sequence_number: usize,
    pub created_at_millis: u64,
    pub file_path: SmkUtf8Slice,
    pub file_size: u64,
    pub reason: SmkUtf8Slice,
    pub is_current_file: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SmkScriptMetaBackupGenerationSlice {
    pub ptr: *const SmkScriptMetaBackupGeneration,
    pub len: usize,
}

pub struct SmkEngine {
    engine: ScriptMetaKitEngine,
    cancellation: scriptmetakit::OperationCancellation,
    last_error: Vec<u8>,
    #[cfg(feature = "native-watch")]
    watcher: Option<NativeWatcher>,
}

pub struct SmkScanResult {
    string_storage: Vec<u8>,
    string_overflow: Vec<Box<[u8]>>,
    string_index: BTreeMap<String, SmkUtf8Slice>,
    catalog_info: SmkCatalogInfo,
    registered_root_signatures: Vec<SmkRegisteredRootSignature>,
    roots: Vec<SmkRootSnapshot>,
    file_lists: Vec<SmkFileListSnapshot>,
    file_entries: Vec<SmkFileEntry>,
    items: Vec<SmkScriptItem>,
    file_items: Vec<SmkScriptItem>,
    candidate_records: Vec<SmkCandidateRecord>,
    update_info: SmkUpdateCheckInfo,
    update_statuses: Vec<SmkUpdateStatusEntry>,
    update_resolutions: Vec<SmkDistributionResolutionEntry>,
    update_failures: Vec<SmkUpdateFailureEntry>,
    update_errors: Vec<SmkUpdateErrorEntry>,
    latest_url_history_urls: Vec<SmkUtf8Slice>,
    change_info: SmkScanChangeInfo,
    file_entry_changes: Vec<SmkFileEntryChange>,
    operation_info: SmkOperationInfo,
    file_issues: Vec<SmkFileIssue>,
    watch_info: SmkWatchChangeInfo,
    watch_events: Vec<SmkWatchPathEvent>,
    ignored_watch_paths: Vec<SmkIgnoredWatchPath>,
    watch_rename_candidates: Vec<SmkWatchRenameCandidate>,
    watch_rescan_targets: Vec<SmkWatchRescanTarget>,
}

pub struct SmkScriptIdUniquenessResult {
    string_storage: Vec<u8>,
    string_overflow: Vec<Box<[u8]>>,
    report: SmkScriptIdUniquenessReport,
    duplicates: Vec<SmkScriptIdDuplicate>,
    item_ids: Vec<SmkUtf8Slice>,
    file_paths: Vec<SmkUtf8Slice>,
}

pub struct SmkEditResult {
    string_storage: Vec<u8>,
    string_overflow: Vec<Box<[u8]>>,
    text: SmkUtf8Slice,
    file_write_result: SmkScriptMetadataFileWriteResult,
    edit_read_result: SmkScriptMetadataEditReadResult,
    edit_preview_result: SmkScriptMetadataEditPreviewResult,
    existing_lines: Vec<SmkUtf8Slice>,
    unknown_lines: Vec<SmkUtf8Slice>,
    has_backup_record: u8,
    backup_record: SmkScriptMetaBackupRecord,
    backup_generations: Vec<SmkScriptMetaBackupGeneration>,
}

pub struct SmkScriptFileInspectionResult {
    string_storage: Vec<u8>,
    string_overflow: Vec<Box<[u8]>>,
    inspection: SmkScriptFileInspection,
}

thread_local! {
    static SCRIPT_FILE_INSPECTION_STORAGE: RefCell<Option<SmkScriptFileInspectionResult>> =
        const { RefCell::new(None) };
}

impl SmkScriptFileInspectionResult {
    fn new(value: ScriptFileInspection) -> Self {
        let mut result = Self {
            string_storage: Vec::with_capacity(value.shebang.as_ref().map_or(0, String::len)),
            string_overflow: Vec::new(),
            inspection: SmkScriptFileInspection::default(),
        };
        result.inspection = smk_script_file_inspection(
            value,
            &mut result.string_storage,
            &mut result.string_overflow,
        );
        result
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `out_engine` must be a valid, writable pointer to receive the new opaque
/// engine handle. The returned handle must be released with `smk_engine_free`.
pub unsafe extern "C" fn smk_engine_create_default(out_engine: *mut *mut SmkEngine) -> SmkStatus {
    ffi_guard(|| {
        let out_engine = out_mut(out_engine)?;
        *out_engine = ptr::null_mut();
        let mut config = ScriptMetaKitConfig::default();
        config.watcher.watch_policy = WatchPolicy::AllRegistered;
        let engine = ScriptMetaKitEngine::new(config)
            .map_err(|error| (SmkStatus::EngineError, error.to_string()))?;
        let cancellation = engine.cancellation_token();
        let handle = Box::new(SmkEngine {
            engine,
            cancellation,
            last_error: Vec::new(),
            #[cfg(feature = "native-watch")]
            watcher: None,
        });
        *out_engine = Box::into_raw(handle);
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be null or a live handle returned by `smk_engine_create_default`.
/// Each non-null handle must be freed at most once.
pub unsafe extern "C" fn smk_engine_free(engine: *mut SmkEngine) {
    if !engine.is_null() {
        // SAFETY: `engine` must be a pointer returned by `smk_engine_create_default`.
        unsafe {
            drop(Box::from_raw(engine));
        }
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `out_extensions` must be a valid, writable pointer. The returned slice is a
/// static newline-separated UTF-8 list and does not need to be freed.
pub unsafe extern "C" fn smk_supported_script_extensions(
    out_extensions: *mut SmkUtf8Slice,
) -> SmkStatus {
    ffi_guard(|| {
        let out_extensions = out_mut(out_extensions)?;
        *out_extensions = static_slice(supported_script_extensions_text());
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `path` must be a valid UTF-8 path slice for the duration of the call.
/// `out_inspection` must be a valid, writable pointer.
pub unsafe extern "C" fn smk_inspect_script_file_path(
    path: SmkUtf8Slice,
    out_inspection: *mut SmkScriptFileInspection,
) -> SmkStatus {
    ffi_guard(|| {
        let path = path_from_slice(path)?;
        let out_inspection = out_mut(out_inspection)?;
        SCRIPT_FILE_INSPECTION_STORAGE.with(|storage| {
            let mut storage = storage.borrow_mut();
            *storage = Some(SmkScriptFileInspectionResult::new(inspect_script_file(
                &path,
            )));
            *out_inspection = storage
                .as_ref()
                .map(|result| result.inspection)
                .unwrap_or_default();
            Ok(())
        })
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `path` must be a valid UTF-8 path slice for the duration of the call.
/// `out_may_affect` must be a valid, writable pointer.
pub unsafe extern "C" fn smk_script_path_may_affect_metadata(
    path: SmkUtf8Slice,
    out_may_affect: *mut u8,
) -> SmkStatus {
    ffi_guard(|| {
        let path = path_from_slice(path)?;
        let out_may_affect = out_mut(out_may_affect)?;
        *out_may_affect = u8::from(script_path_may_affect_metadata(&path));
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `path` must be a valid UTF-8 path slice for the duration of the call.
/// `out_can_read` must be a valid, writable pointer.
pub unsafe extern "C" fn smk_can_read_directory_contents(
    path: SmkUtf8Slice,
    out_can_read: *mut u8,
) -> SmkStatus {
    ffi_guard(|| {
        let path = path_from_slice(path)?;
        let out_can_read = out_mut(out_can_read)?;
        *out_can_read = u8::from(can_read_directory_contents(&path));
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `out_message` must be a valid,
/// writable pointer. The returned slice is borrowed from `engine` and remains
/// valid only until the next mutable call using that handle or until the handle
/// is freed.
pub unsafe extern "C" fn smk_engine_last_error(
    engine: *const SmkEngine,
    out_message: *mut SmkUtf8Slice,
) -> SmkStatus {
    ffi_guard(|| {
        let engine = engine_ref(engine)?;
        let out_message = out_mut(out_message)?;
        *out_message = borrowed_slice(&engine.last_error);
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle returned by
/// `smk_engine_create_default`.
pub unsafe extern "C" fn smk_engine_set_resolve_macos_alias(
    engine: *mut SmkEngine,
    enabled: u8,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();
        engine.engine.config_mut().scanner.resolve_macos_alias = enabled != 0;
        Ok(())
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle returned by
/// `smk_engine_create_default`.
pub unsafe extern "C" fn smk_engine_set_decompile_compiled_osa_during_scan(
    engine: *mut SmkEngine,
    enabled: u8,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();
        engine
            .engine
            .config_mut()
            .scanner
            .decompile_compiled_osa_during_scan = enabled != 0;
        Ok(())
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle returned by
/// `smk_engine_create_default`.
pub unsafe extern "C" fn smk_engine_set_native_event_latency_millis(
    engine: *mut SmkEngine,
    latency_millis: u64,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();
        engine
            .engine
            .config_mut()
            .watcher
            .native_event_latency_millis = latency_millis;
        Ok(())
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle returned by
/// `smk_engine_create_default`.
pub unsafe extern "C" fn smk_engine_set_root_preflight_options(
    engine: *mut SmkEngine,
    reject_trash_roots: u8,
    reject_restricted_roots: u8,
    reject_low_script_density_large_roots: u8,
    max_scanned_items: usize,
    max_duration_millis: u64,
    min_scanned_file_count_for_large_root: usize,
    min_script_ratio_denominator: usize,
    min_scanned_items_for_time_limit: usize,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();
        let preflight = &mut engine.engine.config_mut().scanner.root_preflight;
        preflight.reject_trash_roots = reject_trash_roots != 0;
        preflight.reject_restricted_roots = reject_restricted_roots != 0;
        preflight.reject_low_script_density_large_roots =
            reject_low_script_density_large_roots != 0;
        preflight.max_scanned_items = max_scanned_items;
        preflight.max_duration_millis = max_duration_millis;
        preflight.min_scanned_file_count_for_large_root = min_scanned_file_count_for_large_root;
        preflight.min_script_ratio_denominator = min_script_ratio_denominator;
        preflight.min_scanned_items_for_time_limit = min_scanned_items_for_time_limit;
        Ok(())
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle returned by
/// `smk_engine_create_default`. The function only sets an atomic cancellation
/// flag and is intended to be callable while another operation is running.
pub unsafe extern "C" fn smk_engine_cancel_current_operation(engine: *mut SmkEngine) -> SmkStatus {
    let status = ffi_guard(|| {
        if engine.is_null() {
            return Err((
                SmkStatus::NullArgument,
                "engine pointer was null".to_string(),
            ));
        }
        // SAFETY: `engine` is a live handle. This only touches the cancellation
        // token, which is internally atomic and intentionally separate from the
        // mutable engine state used by long-running operations.
        unsafe {
            (*engine).cancellation.cancel();
        }
        Ok(())
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// Compatibility wrapper for a single-folder scan. `engine` must be a live
/// engine handle. If `path_len` is greater than zero, `path_ptr` must point to
/// `path_len` readable UTF-8 bytes for the duration of the call. `out_result`
/// must be a valid, writable pointer. A non-null result must be released with
/// `smk_scan_result_free`.
pub unsafe extern "C" fn smk_engine_scan_folder(
    engine: *mut SmkEngine,
    path_ptr: *const u8,
    path_len: usize,
    out_result: *mut *mut SmkScanResult,
) -> SmkStatus {
    let path = SmkUtf8Slice {
        ptr: path_ptr,
        len: path_len,
    };
    // SAFETY: forwards the caller-provided path slice as a one-element slice.
    unsafe { smk_engine_scan_folders(engine, &path, 1, 0, out_result) }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `paths_ptr` must point to
/// `path_count` readable `SmkUtf8Slice` values for the duration of the call.
/// Each non-empty path slice must contain valid UTF-8. `check_updates` treats
/// any non-zero value as true. `out_result` must be a valid, writable pointer.
/// A non-null result must be released with `smk_scan_result_free`.
pub unsafe extern "C" fn smk_engine_scan_folders(
    engine: *mut SmkEngine,
    paths_ptr: *const SmkUtf8Slice,
    path_count: usize,
    check_updates: u8,
    out_result: *mut *mut SmkScanResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();

        let paths = utf8_path_slices(paths_ptr, path_count)?;
        if paths.is_empty() {
            let message = "folder path is empty".to_string();
            engine.set_error(&message);
            return Err((SmkStatus::InvalidArgument, message));
        }

        match scan_folders(
            &mut engine.engine,
            paths,
            check_updates != 0,
            None,
            ptr::null_mut(),
        ) {
            Ok(result) => {
                *out_result = Box::into_raw(Box::new(result));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `paths_ptr` must point to
/// `path_count` readable `SmkUtf8Slice` values for the duration of the call.
/// Each non-empty path slice must contain valid UTF-8. `progress_callback`, if
/// non-null, is called synchronously during this function; strings in
/// `SmkUpdateProgress` are valid only for the duration of that callback.
/// `out_result` must be a valid, writable pointer. A non-null result must be
/// released with `smk_scan_result_free`.
pub unsafe extern "C" fn smk_engine_scan_folders_with_progress(
    engine: *mut SmkEngine,
    paths_ptr: *const SmkUtf8Slice,
    path_count: usize,
    check_updates: u8,
    progress_callback: SmkUpdateProgressCallback,
    progress_context: *mut c_void,
    out_result: *mut *mut SmkScanResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();

        let paths = utf8_path_slices(paths_ptr, path_count)?;
        if paths.is_empty() {
            let message = "folder path is empty".to_string();
            engine.set_error(&message);
            return Err((SmkStatus::InvalidArgument, message));
        }

        match scan_folders(
            &mut engine.engine,
            paths,
            check_updates != 0,
            progress_callback,
            progress_context,
        ) {
            Ok(result) => {
                *out_result = Box::into_raw(Box::new(result));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `roots_ptr` must point to
/// `root_count` readable `SmkRootRegistration` values for the duration of the
/// call. String slices must contain valid UTF-8. This configures roots without
/// scanning them.
pub unsafe extern "C" fn smk_engine_set_roots(
    engine: *mut SmkEngine,
    roots_ptr: *const SmkRootRegistration,
    root_count: usize,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();
        let roots = root_registrations_from_raw(roots_ptr, root_count)?;

        match engine.engine.set_roots(roots) {
            Ok(_) => Ok(()),
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `group_id` must contain valid UTF-8.
/// `roots_ptr` must point to `root_count` readable `SmkRootRegistration`
/// values for the duration of the call unless `root_count` is zero. Existing
/// roots in this group are replaced before the engine recomputes the merged
/// registered root set.
pub unsafe extern "C" fn smk_engine_replace_root_group(
    engine: *mut SmkEngine,
    group_id: SmkUtf8Slice,
    roots_ptr: *const SmkRootRegistration,
    root_count: usize,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();
        let group_id = required_str_from_slice(group_id, "group_id")?.to_string();
        let roots = root_registrations_from_raw(roots_ptr, root_count)?;

        match engine.engine.replace_root_group(group_id, roots) {
            Ok(_) => Ok(()),
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `group_id` must contain valid UTF-8.
/// `roots_ptr` must point to `root_count` readable `SmkRootRegistration`
/// values for the duration of the call unless `root_count` is zero. Roots are
/// inserted into this group, replacing entries with the same root_id in that
/// group, and then the engine recomputes the merged registered root set.
pub unsafe extern "C" fn smk_engine_insert_roots_into_group(
    engine: *mut SmkEngine,
    group_id: SmkUtf8Slice,
    roots_ptr: *const SmkRootRegistration,
    root_count: usize,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();
        let group_id = required_str_from_slice(group_id, "group_id")?.to_string();
        let roots = root_registrations_from_raw(roots_ptr, root_count)?;

        match engine.engine.insert_roots_into_group(group_id, roots) {
            Ok(_) => Ok(()),
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. When `has_root_id` is non-zero,
/// `root_id` must contain valid UTF-8 for the duration of this call. Passing
/// `has_root_id=0` clears the visible root.
pub unsafe extern "C" fn smk_engine_set_visible_root(
    engine: *mut SmkEngine,
    root_id: SmkUtf8Slice,
    has_root_id: u8,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();
        let root_id = if has_root_id == 0 {
            None
        } else {
            Some(required_str_from_slice(root_id, "root_id")?.into())
        };
        engine.engine.set_visible_root(root_id);
        Ok(())
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle with roots configured by
/// `smk_engine_set_roots`. `scan_mode` uses 0=file_list_only,
/// 1=metadata_only, 2=file_list_and_metadata. `out_result` must be writable.
/// A non-null result must be released with `smk_scan_result_free`.
pub unsafe extern "C" fn smk_engine_scan_registered_roots(
    engine: *mut SmkEngine,
    scan_mode: u32,
    check_updates: u8,
    out_result: *mut *mut SmkScanResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();
        let scan_mode = scan_mode_from_u32(scan_mode)?;

        match scan_registered_roots(
            &mut engine.engine,
            scan_mode,
            check_updates != 0,
            None,
            ptr::null_mut(),
        ) {
            Ok(result) => {
                *out_result = Box::into_raw(Box::new(result));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle with roots configured by
/// `smk_engine_set_roots`. `root_ids_ptr` must point to `root_id_count`
/// readable `SmkUtf8Slice` values for the duration of the call, unless
/// `root_id_count` is zero. `scan_mode` uses 0=file_list_only,
/// 1=metadata_only, 2=file_list_and_metadata. `out_result` must be writable.
/// A non-null result must be released with `smk_scan_result_free`.
pub unsafe extern "C" fn smk_engine_scan_roots(
    engine: *mut SmkEngine,
    root_ids_ptr: *const SmkUtf8Slice,
    root_id_count: usize,
    scan_mode: u32,
    check_updates: u8,
    out_result: *mut *mut SmkScanResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();
        let root_ids = root_ids_from_raw(root_ids_ptr, root_id_count)?;
        let scan_mode = scan_mode_from_u32(scan_mode)?;

        match scan_selected_roots(
            &mut engine.engine,
            root_ids,
            scan_mode,
            check_updates != 0,
            None,
            ptr::null_mut(),
        ) {
            Ok(result) => {
                *out_result = Box::into_raw(Box::new(result));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle with roots configured by
/// `smk_engine_set_roots`. `root_ids_ptr` must point to `root_id_count`
/// readable `SmkUtf8Slice` values for the duration of the call, unless
/// `root_id_count` is zero. `scan_mode` uses 0=file_list_only,
/// 1=metadata_only, 2=file_list_and_metadata. This returns only currently
/// cached snapshots and never scans the file system. `out_result` must be
/// writable. A non-null result must be released with `smk_scan_result_free`.
pub unsafe extern "C" fn smk_engine_cached_roots(
    engine: *mut SmkEngine,
    root_ids_ptr: *const SmkUtf8Slice,
    root_id_count: usize,
    scan_mode: u32,
    out_result: *mut *mut SmkScanResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();
        let root_ids = root_ids_from_raw(root_ids_ptr, root_id_count)?;
        let scan_mode = scan_mode_from_u32(scan_mode)?;
        let result = engine.engine.cached_scan_result(ScanRequest {
            root_ids,
            mode: scan_mode,
        });
        *out_result = Box::into_raw(Box::new(SmkScanResult::from_scan_result(result, None)));
        Ok(())
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// Same as `smk_engine_scan_registered_roots`, with a synchronous progress
/// callback during update checks.
pub unsafe extern "C" fn smk_engine_scan_registered_roots_with_progress(
    engine: *mut SmkEngine,
    scan_mode: u32,
    check_updates: u8,
    progress_callback: SmkUpdateProgressCallback,
    progress_context: *mut c_void,
    out_result: *mut *mut SmkScanResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();
        let scan_mode = scan_mode_from_u32(scan_mode)?;

        match scan_registered_roots(
            &mut engine.engine,
            scan_mode,
            check_updates != 0,
            progress_callback,
            progress_context,
        ) {
            Ok(result) => {
                *out_result = Box::into_raw(Box::new(result));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// Same as `smk_engine_scan_roots`, with a synchronous progress callback during
/// update checks.
pub unsafe extern "C" fn smk_engine_scan_roots_with_progress(
    engine: *mut SmkEngine,
    root_ids_ptr: *const SmkUtf8Slice,
    root_id_count: usize,
    scan_mode: u32,
    check_updates: u8,
    progress_callback: SmkUpdateProgressCallback,
    progress_context: *mut c_void,
    out_result: *mut *mut SmkScanResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();
        let root_ids = root_ids_from_raw(root_ids_ptr, root_id_count)?;
        let scan_mode = scan_mode_from_u32(scan_mode)?;

        match scan_selected_roots(
            &mut engine.engine,
            root_ids,
            scan_mode,
            check_updates != 0,
            progress_callback,
            progress_context,
        ) {
            Ok(result) => {
                *out_result = Box::into_raw(Box::new(result));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `item` must point to a readable
/// `SmkScriptItem` whose string slices contain valid UTF-8 for the duration of
/// the call. `out_result` must be a valid, writable pointer. A non-null result
/// must be released with `smk_scan_result_free`.
pub unsafe extern "C" fn smk_engine_check_update_item(
    engine: *mut SmkEngine,
    item: *const SmkScriptItem,
    out_result: *mut *mut SmkScanResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();
        let item = Arc::new(script_item_from_ffi(input_ref(item, "item")?)?);

        match pollster::block_on(engine.engine.check_update_for_item(item)) {
            Ok(result) => {
                *out_result =
                    Box::into_raw(Box::new(SmkScanResult::from_update_result(result.as_ref())));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// Same as `smk_engine_check_update_item`, with a synchronous progress callback
/// during the update check. Strings in `SmkUpdateProgress` are valid only for
/// the duration of that callback.
pub unsafe extern "C" fn smk_engine_check_update_item_with_progress(
    engine: *mut SmkEngine,
    item: *const SmkScriptItem,
    progress_callback: SmkUpdateProgressCallback,
    progress_context: *mut c_void,
    out_result: *mut *mut SmkScanResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();
        let item = Arc::new(script_item_from_ffi(input_ref(item, "item")?)?);

        match pollster::block_on(
            engine
                .engine
                .check_update_for_item_with_progress(item, |progress| {
                    emit_update_progress(progress_callback, progress_context, &progress)
                }),
        ) {
            Ok(result) => {
                *out_result =
                    Box::into_raw(Box::new(SmkScanResult::from_update_result(result.as_ref())));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `items_ptr` must point to `item_count` readable
/// `SmkScriptIdUniquenessItem` values for the duration of the call, unless
/// `item_count` is zero. `out_result` must be writable. A non-null result must
/// be released with
/// `smk_script_id_uniqueness_result_free`.
pub unsafe extern "C" fn smk_validate_script_id_uniqueness(
    items_ptr: *const SmkScriptIdUniquenessItem,
    item_count: usize,
    out_result: *mut *mut SmkScriptIdUniquenessResult,
) -> SmkStatus {
    ffi_guard(|| {
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        let items = script_items_for_uniqueness_from_raw(items_ptr, item_count)?;
        let report = validate_script_id_uniqueness(&items);
        *out_result = Box::into_raw(Box::new(SmkScriptIdUniquenessResult::from_report(&report)));
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live result handle returned by
/// `smk_validate_script_id_uniqueness`. `out_report` must be writable.
pub unsafe extern "C" fn smk_script_id_uniqueness_result_report(
    result: *const SmkScriptIdUniquenessResult,
    out_report: *mut SmkScriptIdUniquenessReport,
) -> SmkStatus {
    ffi_guard(|| {
        let result = input_ref(result, "script id uniqueness result")?;
        let out_report = out_mut(out_report)?;
        *out_report = result.report;
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live result handle returned by
/// `smk_validate_script_id_uniqueness`. `out_duplicates` must be writable.
/// Returned pointers remain valid until the result is freed.
pub unsafe extern "C" fn smk_script_id_uniqueness_result_duplicates(
    result: *const SmkScriptIdUniquenessResult,
    out_duplicates: *mut SmkScriptIdDuplicateSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = input_ref(result, "script id uniqueness result")?;
        let out_duplicates = out_mut(out_duplicates)?;
        *out_duplicates = SmkScriptIdDuplicateSlice {
            ptr: result.duplicates.as_ptr(),
            len: result.duplicates.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live result handle returned by
/// `smk_validate_script_id_uniqueness`. `out_item_ids` must be writable.
/// Returned pointers remain valid until the result is freed.
pub unsafe extern "C" fn smk_script_id_uniqueness_result_item_ids(
    result: *const SmkScriptIdUniquenessResult,
    out_item_ids: *mut SmkUtf8SliceSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = input_ref(result, "script id uniqueness result")?;
        let out_item_ids = out_mut(out_item_ids)?;
        *out_item_ids = SmkUtf8SliceSlice {
            ptr: result.item_ids.as_ptr(),
            len: result.item_ids.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live result handle returned by
/// `smk_validate_script_id_uniqueness`. `out_file_paths` must be writable.
/// Returned pointers remain valid until the result is freed.
pub unsafe extern "C" fn smk_script_id_uniqueness_result_file_paths(
    result: *const SmkScriptIdUniquenessResult,
    out_file_paths: *mut SmkUtf8SliceSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = input_ref(result, "script id uniqueness result")?;
        let out_file_paths = out_mut(out_file_paths)?;
        *out_file_paths = SmkUtf8SliceSlice {
            ptr: result.file_paths.as_ptr(),
            len: result.file_paths.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be null or a handle returned by
/// `smk_validate_script_id_uniqueness` that has not already been freed.
pub unsafe extern "C" fn smk_script_id_uniqueness_result_free(
    result: *mut SmkScriptIdUniquenessResult,
) {
    if !result.is_null() {
        // SAFETY: the caller transfers ownership of a handle allocated by this library.
        unsafe {
            drop(Box::from_raw(result));
        }
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `cache_path` must contain a valid
/// UTF-8 file path. Persistent cache must be enabled by the registered roots'
/// cache policies.
pub unsafe extern "C" fn smk_engine_load_cache_file(
    engine: *mut SmkEngine,
    cache_path: SmkUtf8Slice,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();
        let cache_path = path_from_slice(cache_path)?;
        let payload = load_cache_payload(&cache_path).map_err(|error| {
            let message = error.to_string();
            engine.set_error(&message);
            (SmkStatus::EngineError, message)
        })?;
        engine.engine.load_cache(payload).map_err(|error| {
            let message = error.to_string();
            engine.set_error(&message);
            (SmkStatus::EngineError, message)
        })?;
        Ok(())
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `cache_path` must contain a valid
/// UTF-8 file path. Parent directories are created by Kit when needed.
pub unsafe extern "C" fn smk_engine_save_cache_file(
    engine: *mut SmkEngine,
    scope: u32,
    cache_path: SmkUtf8Slice,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();

        let scope = cache_scope_from_u32(scope)?;
        let cache_path = path_from_slice(cache_path)?;
        let payload = engine.engine.export_cache(scope).map_err(|error| {
            let message = error.to_string();
            engine.set_error(&message);
            (SmkStatus::EngineError, message)
        })?;
        save_cache_payload(&cache_path, &payload).map_err(|error| {
            let message = error.to_string();
            engine.set_error(&message);
            (SmkStatus::EngineError, message)
        })?;
        Ok(())
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. The engine must already have roots,
/// normally by calling one of the scan functions first.
pub unsafe extern "C" fn smk_engine_start_watching(engine: *mut SmkEngine) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();
        start_watching_engine(engine, None, ptr::null_mut()).map_err(|message| {
            engine.set_error(&message);
            (SmkStatus::EngineError, message)
        })
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. The engine must already have roots,
/// normally by calling one of the scan functions first. `callback`, if non-null,
/// may be called from a background watcher thread after a file event has been
/// queued. `context` is passed through unchanged and must remain valid until
/// `smk_engine_stop_watching` or `smk_engine_free` returns.
pub unsafe extern "C" fn smk_engine_start_watching_with_callback(
    engine: *mut SmkEngine,
    callback: SmkWatchNotificationCallback,
    context: *mut c_void,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();
        start_watching_engine(engine, callback, context).map_err(|message| {
            engine.set_error(&message);
            (SmkStatus::EngineError, message)
        })
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be null or a live engine handle.
pub unsafe extern "C" fn smk_engine_stop_watching(engine: *mut SmkEngine) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();
        stop_watching_engine(engine);
        Ok(())
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `out_changed` and `out_result` must be
/// valid, writable pointers. When `out_changed` is set to zero, `out_result` is
/// set to null. A non-null result must be released with `smk_scan_result_free`.
pub unsafe extern "C" fn smk_engine_poll_watcher_scan(
    engine: *mut SmkEngine,
    out_changed: *mut u8,
    out_result: *mut *mut SmkScanResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_changed = out_mut(out_changed)?;
        let out_result = out_mut(out_result)?;
        *out_changed = 0;
        *out_result = ptr::null_mut();
        engine.clear_error();

        match poll_watcher_scan(engine) {
            Ok(Some(result)) => {
                *out_changed = 1;
                *out_result = Box::into_raw(Box::new(result));
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(message) => {
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_roots` must be a valid,
/// writable pointer. The returned slice is borrowed from `result` and remains
/// valid only until `result` is freed.
pub unsafe extern "C" fn smk_scan_result_roots(
    result: *const SmkScanResult,
    out_roots: *mut SmkRootSnapshotSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_roots = out_mut(out_roots)?;
        *out_roots = SmkRootSnapshotSlice {
            ptr: result.roots.as_ptr(),
            len: result.roots.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_info` must be a valid,
/// writable pointer.
pub unsafe extern "C" fn smk_scan_result_catalog_info(
    result: *const SmkScanResult,
    out_info: *mut SmkCatalogInfo,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_info = out_mut(out_info)?;
        *out_info = result.catalog_info;
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_roots` must be a valid,
/// writable pointer. The returned slice is borrowed from `result` and remains
/// valid only until `result` is freed.
pub unsafe extern "C" fn smk_scan_result_registered_root_signatures(
    result: *const SmkScanResult,
    out_roots: *mut SmkRegisteredRootSignatureSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_roots = out_mut(out_roots)?;
        *out_roots = SmkRegisteredRootSignatureSlice {
            ptr: result.registered_root_signatures.as_ptr(),
            len: result.registered_root_signatures.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_file_lists` must be a
/// valid, writable pointer. The returned slice is borrowed from `result` and
/// remains valid only until `result` is freed.
pub unsafe extern "C" fn smk_scan_result_file_lists(
    result: *const SmkScanResult,
    out_file_lists: *mut SmkFileListSnapshotSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_file_lists = out_mut(out_file_lists)?;
        *out_file_lists = SmkFileListSnapshotSlice {
            ptr: result.file_lists.as_ptr(),
            len: result.file_lists.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_file_entries` must be a
/// valid, writable pointer. The returned slice is borrowed from `result` and
/// remains valid only until `result` is freed.
pub unsafe extern "C" fn smk_scan_result_file_entries(
    result: *const SmkScanResult,
    out_file_entries: *mut SmkFileEntrySlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_file_entries = out_mut(out_file_entries)?;
        *out_file_entries = SmkFileEntrySlice {
            ptr: result.file_entries.as_ptr(),
            len: result.file_entries.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_items` must be a valid,
/// writable pointer. The returned item slice is borrowed from `result` and
/// remains valid only until `result` is freed.
pub unsafe extern "C" fn smk_scan_result_items(
    result: *const SmkScanResult,
    out_items: *mut SmkScriptItemSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_items = out_mut(out_items)?;
        *out_items = SmkScriptItemSlice {
            ptr: result.items.as_ptr(),
            len: result.items.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_items` must be a valid,
/// writable pointer. The returned item slice is borrowed from `result` and
/// remains valid only until `result` is freed.
pub unsafe extern "C" fn smk_scan_result_file_items(
    result: *const SmkScanResult,
    out_items: *mut SmkScriptItemSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_items = out_mut(out_items)?;
        *out_items = SmkScriptItemSlice {
            ptr: result.file_items.as_ptr(),
            len: result.file_items.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_records` must be a valid,
/// writable pointer. The returned record slice is borrowed from `result` and
/// remains valid only until `result` is freed.
pub unsafe extern "C" fn smk_scan_result_candidate_records(
    result: *const SmkScanResult,
    out_records: *mut SmkCandidateRecordSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_records = out_mut(out_records)?;
        *out_records = SmkCandidateRecordSlice {
            ptr: result.candidate_records.as_ptr(),
            len: result.candidate_records.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_info` must be a valid,
/// writable pointer.
pub unsafe extern "C" fn smk_scan_result_update_info(
    result: *const SmkScanResult,
    out_info: *mut SmkUpdateCheckInfo,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_info = out_mut(out_info)?;
        *out_info = result.update_info;
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_statuses` must be a valid,
/// writable pointer. The returned slice is borrowed from `result`.
pub unsafe extern "C" fn smk_scan_result_update_statuses(
    result: *const SmkScanResult,
    out_statuses: *mut SmkUpdateStatusEntrySlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_statuses = out_mut(out_statuses)?;
        *out_statuses = SmkUpdateStatusEntrySlice {
            ptr: result.update_statuses.as_ptr(),
            len: result.update_statuses.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_resolutions` must be a
/// valid, writable pointer. The returned slice is borrowed from `result`.
pub unsafe extern "C" fn smk_scan_result_update_resolutions(
    result: *const SmkScanResult,
    out_resolutions: *mut SmkDistributionResolutionEntrySlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_resolutions = out_mut(out_resolutions)?;
        *out_resolutions = SmkDistributionResolutionEntrySlice {
            ptr: result.update_resolutions.as_ptr(),
            len: result.update_resolutions.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_failures` must be a valid,
/// writable pointer. The returned slice is borrowed from `result`.
pub unsafe extern "C" fn smk_scan_result_update_failures(
    result: *const SmkScanResult,
    out_failures: *mut SmkUpdateFailureEntrySlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_failures = out_mut(out_failures)?;
        *out_failures = SmkUpdateFailureEntrySlice {
            ptr: result.update_failures.as_ptr(),
            len: result.update_failures.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_errors` must be a valid,
/// writable pointer. The returned slice is borrowed from `result`.
pub unsafe extern "C" fn smk_scan_result_update_errors(
    result: *const SmkScanResult,
    out_errors: *mut SmkUpdateErrorEntrySlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_errors = out_mut(out_errors)?;
        *out_errors = SmkUpdateErrorEntrySlice {
            ptr: result.update_errors.as_ptr(),
            len: result.update_errors.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_urls` must be a valid,
/// writable pointer. The returned slice is borrowed from `result`.
pub unsafe extern "C" fn smk_scan_result_latest_url_history_urls(
    result: *const SmkScanResult,
    out_urls: *mut SmkUtf8SliceSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_urls = out_mut(out_urls)?;
        *out_urls = SmkUtf8SliceSlice {
            ptr: result.latest_url_history_urls.as_ptr(),
            len: result.latest_url_history_urls.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_info` must be a valid,
/// writable pointer.
pub unsafe extern "C" fn smk_scan_result_change_info(
    result: *const SmkScanResult,
    out_info: *mut SmkScanChangeInfo,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_info = out_mut(out_info)?;
        *out_info = result.change_info;
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_info` must be a valid,
/// writable pointer.
pub unsafe extern "C" fn smk_scan_result_operation_info(
    result: *const SmkScanResult,
    out_info: *mut SmkOperationInfo,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_info = out_mut(out_info)?;
        *out_info = result.operation_info;
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_issues` must be a valid,
/// writable pointer. The returned slice is borrowed from `result`.
pub unsafe extern "C" fn smk_scan_result_file_issues(
    result: *const SmkScanResult,
    out_issues: *mut SmkFileIssueSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_issues = out_mut(out_issues)?;
        *out_issues = SmkFileIssueSlice {
            ptr: result.file_issues.as_ptr(),
            len: result.file_issues.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_changes` must be a valid,
/// writable pointer. The returned slice is borrowed from `result`.
pub unsafe extern "C" fn smk_scan_result_file_entry_changes(
    result: *const SmkScanResult,
    out_changes: *mut SmkFileEntryChangeSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_changes = out_mut(out_changes)?;
        *out_changes = SmkFileEntryChangeSlice {
            ptr: result.file_entry_changes.as_ptr(),
            len: result.file_entry_changes.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_info` must be writable.
pub unsafe extern "C" fn smk_scan_result_watch_change_info(
    result: *const SmkScanResult,
    out_info: *mut SmkWatchChangeInfo,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_info = out_mut(out_info)?;
        *out_info = result.watch_info;
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_events` must be writable.
pub unsafe extern "C" fn smk_scan_result_watch_events(
    result: *const SmkScanResult,
    out_events: *mut SmkWatchPathEventSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_events = out_mut(out_events)?;
        *out_events = SmkWatchPathEventSlice {
            ptr: result.watch_events.as_ptr(),
            len: result.watch_events.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_paths` must be writable.
pub unsafe extern "C" fn smk_scan_result_ignored_watch_paths(
    result: *const SmkScanResult,
    out_paths: *mut SmkIgnoredWatchPathSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_paths = out_mut(out_paths)?;
        *out_paths = SmkIgnoredWatchPathSlice {
            ptr: result.ignored_watch_paths.as_ptr(),
            len: result.ignored_watch_paths.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_candidates` must be writable.
pub unsafe extern "C" fn smk_scan_result_watch_rename_candidates(
    result: *const SmkScanResult,
    out_candidates: *mut SmkWatchRenameCandidateSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_candidates = out_mut(out_candidates)?;
        *out_candidates = SmkWatchRenameCandidateSlice {
            ptr: result.watch_rename_candidates.as_ptr(),
            len: result.watch_rename_candidates.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live scan result handle. `out_targets` must be writable.
pub unsafe extern "C" fn smk_scan_result_watch_rescan_targets(
    result: *const SmkScanResult,
    out_targets: *mut SmkWatchRescanTargetSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = scan_result_ref(result)?;
        let out_targets = out_mut(out_targets)?;
        *out_targets = SmkWatchRescanTargetSlice {
            ptr: result.watch_rescan_targets.as_ptr(),
            len: result.watch_rescan_targets.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be null or a live scan result handle returned by this crate.
/// Each non-null handle must be freed at most once.
pub unsafe extern "C" fn smk_scan_result_free(result: *mut SmkScanResult) {
    if !result.is_null() {
        // SAFETY: `result` must be a pointer returned by this crate.
        unsafe {
            drop(Box::from_raw(result));
        }
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `request` must point to a readable
/// `SmkScriptMetadataWriteRequest` for the duration of the call. Each non-empty
/// string slice in the request must contain UTF-8. `out_result` must be a valid,
/// writable pointer. A non-null result must be released with
/// `smk_edit_result_free`.
pub unsafe extern "C" fn smk_engine_write_script_metadata_file(
    engine: *mut SmkEngine,
    request: *const SmkScriptMetadataWriteRequest,
    out_result: *mut *mut SmkEditResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();

        let request = input_ref(request, "script metadata write request")?;
        let file_path = path_from_slice(request.file_path)?;
        let backup_root_path = optional_path_from_slice(request.backup_root_path)?;
        let backup_options =
            backup_root_path.map(|root_directory| ScriptMetaBackupOptions { root_directory });
        let draft = script_metadata_draft_from_ffi(&request.draft)?;
        let mode = script_meta_write_mode(request.write_mode)?;

        match write_script_metadata_to_file(&file_path, &draft, mode, backup_options.as_ref()) {
            Ok(result) => {
                *out_result =
                    Box::into_raw(Box::new(SmkEditResult::from_file_write_result(&result)));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `file_path` must contain UTF-8.
/// `out_result` must be writable. A non-null result must be released with
/// `smk_edit_result_free`.
pub unsafe extern "C" fn smk_engine_read_script_metadata_draft_file(
    engine: *mut SmkEngine,
    file_path: SmkUtf8Slice,
    out_result: *mut *mut SmkEditResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();

        let file_path = path_from_slice(file_path)?;
        match read_script_metadata_draft_from_file(&file_path) {
            Ok(result) => {
                *out_result =
                    Box::into_raw(Box::new(SmkEditResult::from_edit_read_result(&result)));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `file_path` must contain UTF-8.
/// `out_result` must be writable. A non-null result must be released with
/// `smk_edit_result_free`.
pub unsafe extern "C" fn smk_engine_read_script_metadata_edit_preview_file(
    engine: *mut SmkEngine,
    file_path: SmkUtf8Slice,
    max_bytes: usize,
    out_result: *mut *mut SmkEditResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();

        let file_path = path_from_slice(file_path)?;
        match read_script_metadata_edit_preview_from_file(&file_path, max_bytes) {
            Ok(result) => {
                *out_result =
                    Box::into_raw(Box::new(SmkEditResult::from_edit_preview_result(&result)));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `records_ptr` must point to
/// `record_count` readable `SmkDistributionMetadataDraft` values for the
/// duration of the call. Each non-empty string slice must contain UTF-8.
/// `out_result` must be a valid, writable pointer. A non-null result must be
/// released with `smk_edit_result_free`.
pub unsafe extern "C" fn smk_engine_render_distribution_metadata(
    engine: *mut SmkEngine,
    records_ptr: *const SmkDistributionMetadataDraft,
    record_count: usize,
    out_result: *mut *mut SmkEditResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();

        let records = distribution_metadata_drafts_from_raw(records_ptr, record_count)?;
        match render_distribution_metadata_block(&records) {
            Ok(text) => {
                *out_result = Box::into_raw(Box::new(SmkEditResult::from_text(&text)));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `password` must contain UTF-8.
/// `out_result` must be a valid, writable pointer. A non-null result must be
/// released with `smk_edit_result_free`.
pub unsafe extern "C" fn smk_engine_generate_edit_password_sha256(
    engine: *mut SmkEngine,
    password: SmkUtf8Slice,
    out_result: *mut *mut SmkEditResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();

        let password = required_str_from_slice(password, "edit password")?;
        match generate_edit_password_sha256(password) {
            Ok(text) => {
                *out_result = Box::into_raw(Box::new(SmkEditResult::from_text(&text)));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `password` and `stored_value` must
/// contain UTF-8. `out_is_match` must be a valid, writable pointer.
pub unsafe extern "C" fn smk_engine_verify_edit_password_sha256(
    engine: *mut SmkEngine,
    password: SmkUtf8Slice,
    stored_value: SmkUtf8Slice,
    out_is_match: *mut u8,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_is_match = out_mut(out_is_match)?;
        engine.clear_error();

        let password = required_str_from_slice(password, "edit password")?;
        let stored_value = required_str_from_slice(stored_value, "Edit-Password-SHA256")?;
        if !is_valid_edit_password_sha256(stored_value) {
            let message = "Edit-Password-SHA256 is invalid".to_string();
            engine.set_error(&message);
            return Err((SmkStatus::InvalidArgument, message));
        }
        *out_is_match = bool_byte(verify_edit_password_sha256(password, stored_value));
        Ok(())
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `file_path` and `backup_root_path`
/// must contain valid UTF-8 if non-empty. `out_result` must be a valid,
/// writable pointer. A non-null result must be released with
/// `smk_edit_result_free`.
pub unsafe extern "C" fn smk_engine_scriptmeta_backup_generations(
    engine: *mut SmkEngine,
    file_path: SmkUtf8Slice,
    backup_root_path: SmkUtf8Slice,
    out_result: *mut *mut SmkEditResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();

        let file_path = path_from_slice(file_path)?;
        let backup_options = backup_options_from_slice(backup_root_path)?;
        match scriptmeta_backup_generations(&file_path, &backup_options) {
            Ok(generations) => {
                *out_result = Box::into_raw(Box::new(SmkEditResult::from_backup_generations(
                    &generations,
                )));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `file_path` and `backup_root_path`
/// must contain valid UTF-8 if non-empty. `out_result` must be a valid,
/// writable pointer. A non-null result must be released with
/// `smk_edit_result_free`.
pub unsafe extern "C" fn smk_engine_create_scriptmeta_backup(
    engine: *mut SmkEngine,
    file_path: SmkUtf8Slice,
    backup_root_path: SmkUtf8Slice,
    reason: u32,
    out_result: *mut *mut SmkEditResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();

        let file_path = path_from_slice(file_path)?;
        let backup_options = backup_options_from_slice(backup_root_path)?;
        let reason = script_meta_backup_reason(reason)?;
        match create_scriptmeta_backup(&file_path, &backup_options, reason) {
            Ok(record) => {
                *out_result = Box::into_raw(Box::new(SmkEditResult::from_backup_record(&record)));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `file_path`, `backup_root_path`, and
/// `generation_id` must contain valid UTF-8 if non-empty. `out_result` must be
/// a valid, writable pointer. A non-null result must be released with
/// `smk_edit_result_free`.
pub unsafe extern "C" fn smk_engine_restore_scriptmeta_backup(
    engine: *mut SmkEngine,
    file_path: SmkUtf8Slice,
    backup_root_path: SmkUtf8Slice,
    generation_id: SmkUtf8Slice,
    out_result: *mut *mut SmkEditResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();

        let file_path = path_from_slice(file_path)?;
        let backup_options = backup_options_from_slice(backup_root_path)?;
        let generation_id = required_str_from_slice(generation_id, "generation id")?;
        match restore_scriptmeta_backup(&file_path, &backup_options, generation_id) {
            Ok(record) => {
                *out_result = Box::into_raw(Box::new(SmkEditResult::from_backup_record(&record)));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `file_path` and `backup_root_path`
/// must contain valid UTF-8 if non-empty.
pub unsafe extern "C" fn smk_engine_clear_scriptmeta_backups(
    engine: *mut SmkEngine,
    file_path: SmkUtf8Slice,
    backup_root_path: SmkUtf8Slice,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        engine.clear_error();

        let file_path = path_from_slice(file_path)?;
        let backup_options = backup_options_from_slice(backup_root_path)?;
        match clear_scriptmeta_backups(&file_path, &backup_options) {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `engine` must be a live engine handle. `file_path` and `backup_root_path`
/// must contain valid UTF-8 if non-empty. `out_result` must be a valid,
/// writable pointer. A non-null result must be released with
/// `smk_edit_result_free`.
pub unsafe extern "C" fn smk_engine_reset_scriptmeta_backups_with_current_as_initial(
    engine: *mut SmkEngine,
    file_path: SmkUtf8Slice,
    backup_root_path: SmkUtf8Slice,
    out_result: *mut *mut SmkEditResult,
) -> SmkStatus {
    let status = ffi_guard(|| {
        let engine = engine_mut(engine)?;
        let out_result = out_mut(out_result)?;
        *out_result = ptr::null_mut();
        engine.clear_error();

        let file_path = path_from_slice(file_path)?;
        let backup_options = backup_options_from_slice(backup_root_path)?;
        match reset_scriptmeta_backups_with_current_as_initial(&file_path, &backup_options) {
            Ok(record) => {
                *out_result = Box::into_raw(Box::new(SmkEditResult::from_backup_record(&record)));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                engine.set_error(&message);
                Err((SmkStatus::EngineError, message))
            }
        }
    });

    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live edit result handle. `out_text` must be a valid,
/// writable pointer. The returned slice is borrowed from `result` and remains
/// valid only until `result` is freed.
pub unsafe extern "C" fn smk_edit_result_text(
    result: *const SmkEditResult,
    out_text: *mut SmkUtf8Slice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = edit_result_ref(result)?;
        let out_text = out_mut(out_text)?;
        *out_text = result.text;
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live edit result handle. `out_info` must be a valid,
/// writable pointer.
pub unsafe extern "C" fn smk_edit_result_file_write_result(
    result: *const SmkEditResult,
    out_info: *mut SmkScriptMetadataFileWriteResult,
) -> SmkStatus {
    ffi_guard(|| {
        let result = edit_result_ref(result)?;
        let out_info = out_mut(out_info)?;
        *out_info = result.file_write_result;
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live edit result handle. `out_info` must be writable.
pub unsafe extern "C" fn smk_edit_result_metadata_edit_read_result(
    result: *const SmkEditResult,
    out_info: *mut SmkScriptMetadataEditReadResult,
) -> SmkStatus {
    ffi_guard(|| {
        let result = edit_result_ref(result)?;
        let out_info = out_mut(out_info)?;
        *out_info = result.edit_read_result;
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live edit result handle. `out_info` must be writable.
pub unsafe extern "C" fn smk_edit_result_metadata_edit_preview_result(
    result: *const SmkEditResult,
    out_info: *mut SmkScriptMetadataEditPreviewResult,
) -> SmkStatus {
    ffi_guard(|| {
        let result = edit_result_ref(result)?;
        let out_info = out_mut(out_info)?;
        *out_info = result.edit_preview_result;
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live edit result handle. `out_lines` must be writable.
pub unsafe extern "C" fn smk_edit_result_existing_lines(
    result: *const SmkEditResult,
    out_lines: *mut SmkUtf8SliceSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = edit_result_ref(result)?;
        let out_lines = out_mut(out_lines)?;
        *out_lines = SmkUtf8SliceSlice {
            ptr: result.existing_lines.as_ptr(),
            len: result.existing_lines.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live edit result handle. `out_lines` must be writable.
pub unsafe extern "C" fn smk_edit_result_unknown_lines(
    result: *const SmkEditResult,
    out_lines: *mut SmkUtf8SliceSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = edit_result_ref(result)?;
        let out_lines = out_mut(out_lines)?;
        *out_lines = SmkUtf8SliceSlice {
            ptr: result.unknown_lines.as_ptr(),
            len: result.unknown_lines.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live edit result handle. `out_has_record` and
/// `out_record` must be valid, writable pointers.
pub unsafe extern "C" fn smk_edit_result_backup_record(
    result: *const SmkEditResult,
    out_has_record: *mut u8,
    out_record: *mut SmkScriptMetaBackupRecord,
) -> SmkStatus {
    ffi_guard(|| {
        let result = edit_result_ref(result)?;
        let out_has_record = out_mut(out_has_record)?;
        let out_record = out_mut(out_record)?;
        *out_has_record = result.has_backup_record;
        *out_record = result.backup_record;
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be a live edit result handle. `out_generations` must be a
/// valid, writable pointer. The returned slice is borrowed from `result`.
pub unsafe extern "C" fn smk_edit_result_backup_generations(
    result: *const SmkEditResult,
    out_generations: *mut SmkScriptMetaBackupGenerationSlice,
) -> SmkStatus {
    ffi_guard(|| {
        let result = edit_result_ref(result)?;
        let out_generations = out_mut(out_generations)?;
        *out_generations = SmkScriptMetaBackupGenerationSlice {
            ptr: result.backup_generations.as_ptr(),
            len: result.backup_generations.len(),
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `result` must be null or a live handle returned by this crate. Each non-null
/// handle must be freed at most once.
pub unsafe extern "C" fn smk_edit_result_free(result: *mut SmkEditResult) {
    if !result.is_null() {
        // SAFETY: `result` must be a pointer returned by this crate.
        unsafe {
            drop(Box::from_raw(result));
        }
    }
}

fn scan_folders(
    engine: &mut ScriptMetaKitEngine,
    folders: Vec<PathBuf>,
    check_updates: bool,
    progress_callback: SmkUpdateProgressCallback,
    progress_context: *mut c_void,
) -> scriptmetakit::ScriptMetaKitResult<SmkScanResult> {
    let roots = folders
        .into_iter()
        .map(|folder| {
            let root_id = folder
                .canonicalize()
                .unwrap_or_else(|_| folder.clone())
                .to_string_lossy()
                .into_owned();
            let display_name = folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| folder.display().to_string());
            RootRegistration {
                root_id: root_id.into(),
                path: folder,
                display_name: Some(display_name),
                purpose: RootPurpose::FileListAndMetadata,
                watch_policy: WatchPolicy::AllRegistered,
                cache_policy: CachePolicy::MemoryAndPersistent,
                refresh_policy: RefreshPolicy::OnFileEvent,
                priority: RootPriority::UserInitiated,
            }
        })
        .collect();

    engine.set_roots(roots)?;
    let scan_result = engine.scan_roots(ScanRequest {
        root_ids: Vec::new(),
        mode: ScanMode::FileListAndMetadata,
    })?;
    let update_result = if check_updates {
        let items = scan_result
            .catalog_snapshot
            .as_ref()
            .map_or(&[][..], |snapshot| snapshot.file_items.as_slice());
        let update_result = if progress_callback.is_some() {
            pollster::block_on(
                engine.check_updates_for_items_with_progress(items, |progress| {
                    emit_update_progress(progress_callback, progress_context, &progress)
                }),
            )?
        } else {
            pollster::block_on(engine.check_updates_for_items(items))?
        };
        Some(update_result)
    } else {
        None
    };

    Ok(SmkScanResult::from_scan_result(scan_result, update_result))
}

fn scan_registered_roots(
    engine: &mut ScriptMetaKitEngine,
    scan_mode: ScanMode,
    check_updates: bool,
    progress_callback: SmkUpdateProgressCallback,
    progress_context: *mut c_void,
) -> scriptmetakit::ScriptMetaKitResult<SmkScanResult> {
    scan_selected_roots(
        engine,
        Vec::new(),
        scan_mode,
        check_updates,
        progress_callback,
        progress_context,
    )
}

fn scan_selected_roots(
    engine: &mut ScriptMetaKitEngine,
    root_ids: Vec<RootId>,
    scan_mode: ScanMode,
    check_updates: bool,
    progress_callback: SmkUpdateProgressCallback,
    progress_context: *mut c_void,
) -> scriptmetakit::ScriptMetaKitResult<SmkScanResult> {
    let scan_result = engine.scan_roots(ScanRequest {
        root_ids,
        mode: scan_mode,
    })?;
    let update_result = if check_updates {
        let items = scan_result
            .catalog_snapshot
            .as_ref()
            .map_or(&[][..], |snapshot| snapshot.file_items.as_slice());
        let update_result = if progress_callback.is_some() {
            pollster::block_on(
                engine.check_updates_for_items_with_progress(items, |progress| {
                    emit_update_progress(progress_callback, progress_context, &progress)
                }),
            )?
        } else {
            pollster::block_on(engine.check_updates_for_items(items))?
        };
        Some(update_result)
    } else {
        None
    };

    Ok(SmkScanResult::from_scan_result(scan_result, update_result))
}

#[cfg(feature = "native-watch")]
fn start_watching_engine(
    engine: &mut SmkEngine,
    callback: SmkWatchNotificationCallback,
    context: *mut c_void,
) -> Result<(), String> {
    let plan = engine.engine.watch_plan();
    let watcher = if let Some(callback) = callback {
        let context = context as usize;
        NativeWatcher::start_with_notifier(
            &plan,
            Some(Arc::new(move || {
                callback(context as *mut c_void);
            })),
        )
    } else {
        NativeWatcher::start(&plan)
    }
    .map_err(|error| error.to_string())?;
    engine.watcher = Some(watcher);
    Ok(())
}

#[cfg(not(feature = "native-watch"))]
fn start_watching_engine(
    _engine: &mut SmkEngine,
    _callback: SmkWatchNotificationCallback,
    _context: *mut c_void,
) -> Result<(), String> {
    Err("native-watch feature is not enabled".to_string())
}

#[cfg(feature = "native-watch")]
fn stop_watching_engine(engine: &mut SmkEngine) {
    engine.watcher = None;
}

#[cfg(not(feature = "native-watch"))]
fn stop_watching_engine(_engine: &mut SmkEngine) {}

#[cfg(feature = "native-watch")]
fn poll_watcher_scan(engine: &mut SmkEngine) -> Result<Option<SmkScanResult>, String> {
    let Some(watcher) = engine.watcher.as_ref() else {
        return Err("watcher is not running".to_string());
    };
    let Some(mut batch) = watcher.try_recv() else {
        return Ok(None);
    };
    while let Some(next_batch) = watcher.try_recv() {
        batch.paths.extend(next_batch.paths);
        batch.overflowed |= next_batch.overflowed;
    }
    batch.paths.sort();
    batch.paths.dedup();

    let change_batch = engine
        .engine
        .mark_changed_paths(batch)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find_map(|event| match event {
            scriptmetakit::ScriptMetaKitEvent::ChangeDetected { batch } => Some(batch),
            _ => None,
        });
    let Some(change_batch) = change_batch else {
        return Ok(None);
    };
    let mut scan_result = engine
        .engine
        .refresh_dirty_roots(RefreshRequest {
            mode: ScanMode::FileListAndMetadata,
        })
        .map_err(|error| error.to_string())?;
    scan_result.watch_change_batch = Some(change_batch);
    Ok(Some(SmkScanResult::from_scan_result(scan_result, None)))
}

#[cfg(not(feature = "native-watch"))]
fn poll_watcher_scan(_engine: &mut SmkEngine) -> Result<Option<SmkScanResult>, String> {
    Err("native-watch feature is not enabled".to_string())
}

impl SmkScanResult {
    fn from_scan_result(
        scan_result: ScanResult,
        update_result: Option<Arc<UpdateCheckResult>>,
    ) -> Self {
        let update_result = update_result
            .as_deref()
            .or(scan_result.update_check_result.as_deref());
        let operation = update_result
            .as_ref()
            .map(|result| &result.operation)
            .unwrap_or(&scan_result.operation);
        let string_capacity = total_string_bytes(&scan_result, update_result);
        let file_entry_capacity = total_file_entry_count(&scan_result);
        let mut result = Self {
            string_storage: Vec::with_capacity(string_capacity),
            string_overflow: Vec::new(),
            string_index: BTreeMap::new(),
            catalog_info: SmkCatalogInfo::default(),
            registered_root_signatures: scan_result
                .catalog_snapshot
                .as_ref()
                .map_or_else(Vec::new, |snapshot| {
                    Vec::with_capacity(snapshot.candidate_cache.registered_roots.len())
                }),
            roots: Vec::with_capacity(scan_result.roots.len()),
            file_lists: Vec::with_capacity(scan_result.file_list_snapshots.len()),
            file_entries: Vec::with_capacity(file_entry_capacity),
            items: scan_result
                .catalog_snapshot
                .as_ref()
                .map_or_else(Vec::new, |snapshot| {
                    Vec::with_capacity(snapshot.all_items.len())
                }),
            file_items: scan_result
                .catalog_snapshot
                .as_ref()
                .map_or_else(Vec::new, |snapshot| {
                    Vec::with_capacity(snapshot.file_items.len())
                }),
            candidate_records: scan_result
                .catalog_snapshot
                .as_ref()
                .map_or_else(Vec::new, |snapshot| {
                    Vec::with_capacity(snapshot.candidate_cache.records.len())
                }),
            update_info: SmkUpdateCheckInfo::default(),
            update_statuses: Vec::new(),
            update_resolutions: Vec::new(),
            update_failures: Vec::new(),
            update_errors: Vec::new(),
            latest_url_history_urls: Vec::new(),
            change_info: SmkScanChangeInfo::default(),
            file_entry_changes: Vec::new(),
            operation_info: SmkOperationInfo::default(),
            file_issues: Vec::with_capacity(scan_result.file_issues.len()),
            watch_info: SmkWatchChangeInfo::default(),
            watch_events: Vec::new(),
            ignored_watch_paths: Vec::new(),
            watch_rename_candidates: Vec::new(),
            watch_rescan_targets: Vec::new(),
        };

        let root_indices: BTreeMap<_, _> = scan_result
            .roots
            .iter()
            .enumerate()
            .map(|(index, root)| (root.root_id.as_ref(), index))
            .collect();
        for root in &scan_result.roots {
            result.push_root(root);
        }
        result.push_operation_info(operation);
        for issue in &scan_result.file_issues {
            result.push_file_issue(issue);
        }

        for snapshot in &scan_result.file_list_snapshots {
            result.push_file_list(snapshot, &root_indices);
        }

        if let Some(snapshot) = scan_result.catalog_snapshot.as_ref() {
            result.push_catalog_info(snapshot);
            let mut all_item_indices = BTreeMap::new();
            for item in &snapshot.all_items {
                let item_key = std::sync::Arc::as_ptr(item) as usize;
                let item_index = result.items.len();
                result.push_script_item(item);
                all_item_indices.insert(item_key, item_index);
            }
            for item in &snapshot.file_items {
                let item_key = std::sync::Arc::as_ptr(item) as usize;
                if let Some(item_index) = all_item_indices.get(&item_key) {
                    result.file_items.push(result.items[*item_index]);
                } else {
                    result.push_file_script_item(item);
                }
            }
            for record in &snapshot.candidate_cache.records {
                result.push_candidate_record(record);
            }
        }

        if let Some(change_summary) = scan_result.change_summary.as_ref() {
            result.push_change_summary(change_summary);
        }
        if let Some(batch) = scan_result.watch_change_batch.as_ref() {
            result.push_watch_change_batch(batch);
        }

        if let Some(update_result) = update_result {
            result.push_update_result(update_result);
        }

        result.string_index.clear();
        result
    }

    fn from_update_result(update_result: &UpdateCheckResult) -> Self {
        let mut result = Self {
            string_storage: Vec::with_capacity(update_result_string_bytes(update_result)),
            string_overflow: Vec::new(),
            string_index: BTreeMap::new(),
            catalog_info: SmkCatalogInfo::default(),
            registered_root_signatures: Vec::new(),
            roots: Vec::new(),
            file_lists: Vec::new(),
            file_entries: Vec::new(),
            items: Vec::new(),
            file_items: Vec::new(),
            candidate_records: Vec::new(),
            update_info: SmkUpdateCheckInfo::default(),
            update_statuses: Vec::new(),
            update_resolutions: Vec::new(),
            update_failures: Vec::new(),
            update_errors: Vec::new(),
            latest_url_history_urls: Vec::new(),
            change_info: SmkScanChangeInfo::default(),
            file_entry_changes: Vec::new(),
            operation_info: SmkOperationInfo::default(),
            file_issues: Vec::new(),
            watch_info: SmkWatchChangeInfo::default(),
            watch_events: Vec::new(),
            ignored_watch_paths: Vec::new(),
            watch_rename_candidates: Vec::new(),
            watch_rescan_targets: Vec::new(),
        };
        result.push_operation_info(&update_result.operation);
        result.push_update_result(update_result);
        result.string_index.clear();
        result
    }

    fn push_root(&mut self, root: &RootSnapshot) {
        let (has_last_loaded_at, last_loaded_at) = optional_u64(root.last_loaded_at);
        let (has_last_event_at, last_event_at) = optional_u64(root.last_event_at);
        let ffi_root = SmkRootSnapshot {
            root_id: self.push_string(Some(root.root_id.as_ref())),
            path: self.push_path(&root.path),
            status: static_slice(root_status(root.status)),
            is_dirty: bool_byte(root.is_dirty),
            has_last_loaded_at,
            last_loaded_at,
            has_last_event_at,
            last_event_at,
            item_count: root.item_count,
            error_code: self.push_string(root.error.as_ref().map(|error| error.code.as_str())),
            error_message: self
                .push_string(root.error.as_ref().map(|error| error.message.as_str())),
        };
        self.roots.push(ffi_root);
    }

    fn push_catalog_info(&mut self, snapshot: &ScriptMetaCatalogSnapshot) {
        let source_revision = snapshot.source_revision.to_string();
        self.catalog_info = SmkCatalogInfo {
            has_catalog: 1,
            source_revision: self.push_string(Some(source_revision.as_str())),
            candidate_cache_schema_version: snapshot.candidate_cache.schema_version,
            candidate_cache_built_at: snapshot.candidate_cache.built_at,
        };
        for signature in &snapshot.candidate_cache.registered_roots {
            let ffi_signature = SmkRegisteredRootSignature {
                root_id: self.push_string(Some(signature.root_id.as_ref())),
                path: self.push_path(&signature.path),
            };
            self.registered_root_signatures.push(ffi_signature);
        }
    }

    fn push_operation_info(&mut self, operation: &OperationSummary) {
        self.operation_info = SmkOperationInfo {
            status: static_slice(operation_status(operation.status)),
            total_units: operation.total_units,
            completed_units: operation.completed_units,
            failed_units: operation.failed_units,
            cancelled: bool_byte(operation.cancelled),
            timed_out: bool_byte(operation.timed_out),
            reason_code: self.push_string(operation.reason_code.as_deref()),
            message: self.push_string(operation.message.as_deref()),
        };
    }

    fn push_file_issue(&mut self, issue: &FileIssue) {
        let ffi_issue = SmkFileIssue {
            has_root_id: bool_byte(issue.root_id.is_some()),
            root_id: self.push_string(issue.root_id.as_deref()),
            path: self.push_path(&issue.path),
            code: self.push_string(Some(issue.code.as_str())),
            message: self.push_string(Some(issue.message.as_str())),
            path_kind: self.push_string(issue.path_kind.as_deref()),
            resolution_status: self.push_string(issue.resolution_status.as_deref()),
            is_directory: bool_byte(issue.is_directory),
        };
        self.file_issues.push(ffi_issue);
    }

    fn push_file_list(
        &mut self,
        snapshot: &FileListSnapshot,
        root_indices: &BTreeMap<&str, usize>,
    ) {
        let first_child_index = self.file_entries.len();
        let child_count = snapshot
            .children
            .as_ref()
            .map_or(0, |children| self.push_file_entries(children));
        let root_index = root_indices
            .get(snapshot.root.root_id.as_ref())
            .copied()
            .unwrap_or(usize::MAX);
        self.file_lists.push(SmkFileListSnapshot {
            root_index,
            first_child_index,
            child_count,
            truncated: bool_byte(snapshot.truncated),
        });
    }

    fn push_file_entries(&mut self, entries: &[FileSystemEntry]) -> usize {
        let first_entry_index = self.file_entries.len();
        for entry in entries {
            let scriptmeta_item = entry
                .scriptmeta_item
                .as_ref()
                .map(|item| self.script_item(item))
                .unwrap_or_default();
            let ffi_entry = SmkFileEntry {
                display_path: self.push_path(&entry.display_path),
                resolved_path: self.push_path(&entry.resolved_path),
                path_kind: static_slice(entry.path_kind.as_str()),
                resolution_status: static_slice(entry.resolution_status.as_str()),
                resolution_message: self.push_string(entry.resolution_message.as_deref()),
                is_directory: bool_byte(entry.is_directory),
                has_file_size: bool_byte(entry.file_size.is_some()),
                file_size: entry.file_size.unwrap_or_default(),
                has_content_modified_at: bool_byte(entry.content_modified_at.is_some()),
                content_modified_at: entry.content_modified_at.unwrap_or_default(),
                has_identity: bool_byte(entry.identity.is_some()),
                identity: entry
                    .identity
                    .as_ref()
                    .map(|identity| self.file_identity(identity))
                    .unwrap_or_default(),
                runtime_kind: entry.runtime_kind.map_or_else(SmkUtf8Slice::empty, |kind| {
                    static_slice(script_runtime_kind(kind))
                }),
                shebang: self.push_string(entry.shebang.as_deref()),
                has_scriptmeta: bool_byte(entry.has_scriptmeta),
                has_scriptmeta_edit_password: bool_byte(entry.has_scriptmeta_edit_password),
                is_file_locked: bool_byte(entry.is_file_locked),
                is_read_only: bool_byte(entry.is_read_only),
                can_edit_scriptmeta: bool_byte(entry.can_edit_scriptmeta),
                can_append_scriptmeta: bool_byte(entry.can_append_scriptmeta),
                scriptmeta_edit_state: static_slice(entry.scriptmeta_edit_state.as_str()),
                has_scriptmeta_item: bool_byte(entry.scriptmeta_item.is_some()),
                scriptmeta_item,
                first_child_index: 0,
                child_count: 0,
            };
            self.file_entries.push(ffi_entry);
        }

        for (offset, entry) in entries.iter().enumerate() {
            let entry_index = first_entry_index + offset;
            let first_child_index = self.file_entries.len();
            let child_count = self.push_file_entries(&entry.children);
            self.file_entries[entry_index].first_child_index = first_child_index;
            self.file_entries[entry_index].child_count = child_count;
        }
        entries.len()
    }

    fn push_script_item(&mut self, item: &ScriptMetaItem) {
        let ffi_item = self.script_item(item);
        self.items.push(ffi_item);
    }

    fn push_file_script_item(&mut self, item: &ScriptMetaItem) {
        let ffi_item = self.script_item(item);
        self.file_items.push(ffi_item);
    }

    fn script_item(&mut self, item: &ScriptMetaItem) -> SmkScriptItem {
        SmkScriptItem {
            root_id: self.push_string(Some(item.root_id.as_ref())),
            file_path: self.push_path(&item.file_path),
            identity_path: self.push_path(&item.identity_path),
            runtime_kind: item.runtime_kind.map_or_else(SmkUtf8Slice::empty, |kind| {
                static_slice(script_runtime_kind(kind))
            }),
            shebang: self.push_string(item.shebang.as_deref()),
            script_id: self.push_string(Some(item.script_id.as_str())),
            version: self.push_string(item.version.as_deref()),
            name: self.push_string(item.name.as_deref()),
            description: self.push_string(item.description.as_deref()),
            target_app: self.push_string(item.target_app.as_deref()),
            min_target_version: self.push_string(item.min_target_version.as_deref()),
            meta_url: self.push_url(item.meta_url.as_ref()),
            author: self.push_string(item.author.as_deref()),
            release_date: self.push_string(item.release_date.as_deref()),
            edit_password_sha256: self.push_string(item.edit_password_sha256.as_deref()),
            has_scriptmeta: bool_byte(item.has_scriptmeta),
            has_scriptmeta_edit_password: bool_byte(item.has_scriptmeta_edit_password),
            is_file_locked: bool_byte(item.is_file_locked),
            is_read_only: bool_byte(item.is_read_only),
            can_edit_scriptmeta: bool_byte(item.can_edit_scriptmeta),
            can_append_scriptmeta: bool_byte(item.can_append_scriptmeta),
            scriptmeta_edit_state: static_slice(item.scriptmeta_edit_state.as_str()),
        }
    }

    fn push_candidate_record(&mut self, record: &CandidateRecord) {
        let ffi_record = self.candidate_record(record);
        self.candidate_records.push(ffi_record);
    }

    fn candidate_record(&mut self, record: &CandidateRecord) -> SmkCandidateRecord {
        let (has_file_size, file_size) = optional_u64(record.file_size);
        let (has_content_modified_at, content_modified_at) =
            optional_u64(record.content_modified_at);
        let item = record
            .item
            .as_ref()
            .map(|item| self.script_item(item))
            .unwrap_or_default();
        SmkCandidateRecord {
            root_id: self.push_string(Some(record.root_id.as_ref())),
            root_path: self.push_path(&record.root_path),
            file_path: self.push_path(&record.file_path),
            identity_path: self.push_path(&record.identity_path),
            path_kind: static_slice(record.path_kind.as_str()),
            resolution_status: static_slice(record.resolution_status.as_str()),
            resolution_message: self.push_string(record.resolution_message.as_deref()),
            runtime_kind: record
                .runtime_kind
                .map_or_else(SmkUtf8Slice::empty, |kind| {
                    static_slice(script_runtime_kind(kind))
                }),
            shebang: self.push_string(record.shebang.as_deref()),
            has_scriptmeta: bool_byte(record.has_scriptmeta),
            has_scriptmeta_edit_password: bool_byte(record.has_scriptmeta_edit_password),
            is_file_locked: bool_byte(record.is_file_locked),
            is_read_only: bool_byte(record.is_read_only),
            can_edit_scriptmeta: bool_byte(record.can_edit_scriptmeta),
            can_append_scriptmeta: bool_byte(record.can_append_scriptmeta),
            scriptmeta_edit_state: static_slice(record.scriptmeta_edit_state.as_str()),
            has_file_size,
            file_size,
            has_content_modified_at,
            content_modified_at,
            has_item: bool_byte(record.item.is_some()),
            item,
        }
    }

    fn push_change_summary(&mut self, summary: &ScanChangeSummary) {
        self.change_info = SmkScanChangeInfo {
            has_change_summary: 1,
            added_count: summary.added_count,
            removed_count: summary.removed_count,
            modified_count: summary.modified_count,
        };
        self.file_entry_changes.reserve(summary.changes.len());
        for change in &summary.changes {
            self.push_file_entry_change(change);
        }
    }

    fn push_file_entry_change(&mut self, change: &FileEntryChange) {
        let entry = SmkFileEntryChange {
            root_id: self.push_string(Some(change.root_id.as_ref())),
            kind: static_slice(file_entry_change_kind(change.kind)),
            display_path: self.push_path(&change.display_path),
            resolved_path: self.push_path(&change.resolved_path),
            path_kind: static_slice(change.path_kind.as_str()),
            resolution_status: static_slice(change.resolution_status.as_str()),
            resolution_message: self.push_string(change.resolution_message.as_deref()),
            is_directory: bool_byte(change.is_directory),
            has_file_size: bool_byte(change.file_size.is_some()),
            file_size: change.file_size.unwrap_or_default(),
            has_content_modified_at: bool_byte(change.content_modified_at.is_some()),
            content_modified_at: change.content_modified_at.unwrap_or_default(),
            has_identity: bool_byte(change.identity.is_some()),
            identity: change
                .identity
                .as_ref()
                .map(|identity| self.file_identity(identity))
                .unwrap_or_default(),
            runtime_kind: change
                .runtime_kind
                .map_or_else(SmkUtf8Slice::empty, |kind| {
                    static_slice(script_runtime_kind(kind))
                }),
            shebang: self.push_string(change.shebang.as_deref()),
            has_scriptmeta: bool_byte(change.has_scriptmeta),
            has_scriptmeta_edit_password: bool_byte(change.has_scriptmeta_edit_password),
            is_file_locked: bool_byte(change.is_file_locked),
            is_read_only: bool_byte(change.is_read_only),
            can_edit_scriptmeta: bool_byte(change.can_edit_scriptmeta),
            can_append_scriptmeta: bool_byte(change.can_append_scriptmeta),
            scriptmeta_edit_state: static_slice(change.scriptmeta_edit_state.as_str()),
        };
        self.file_entry_changes.push(entry);
    }

    fn push_watch_change_batch(&mut self, batch: &RootChangeBatch) {
        let rescan_target_count = batch
            .affected_roots
            .iter()
            .map(|root| root.rescan_targets.len())
            .sum();
        self.watch_info = SmkWatchChangeInfo {
            has_watch_change: 1,
            overflowed: bool_byte(batch.overflowed),
            path_count: batch.paths.len(),
            affected_root_count: batch.affected_roots.len(),
            event_count: batch.events.len(),
            ignored_path_count: batch.ignored_paths.len(),
            rename_candidate_count: batch.rename_candidates.len(),
            rescan_target_count,
        };
        for event in &batch.events {
            let event = self.watch_event(event);
            self.watch_events.push(event);
        }
        for ignored in &batch.ignored_paths {
            let ignored = self.ignored_watch_path(ignored);
            self.ignored_watch_paths.push(ignored);
        }
        for candidate in &batch.rename_candidates {
            let candidate = self.watch_rename_candidate(candidate);
            self.watch_rename_candidates.push(candidate);
        }
        for target in batch
            .affected_roots
            .iter()
            .flat_map(|root| root.rescan_targets.iter())
        {
            let target = self.watch_rescan_target(target);
            self.watch_rescan_targets.push(target);
        }
    }

    fn watch_event(&mut self, event: &WatchPathEvent) -> SmkWatchPathEvent {
        SmkWatchPathEvent {
            root_id: self.push_string(Some(event.root_id.as_ref())),
            path: self.push_path(&event.path),
            kind: static_slice(watch_path_event_kind(event.kind)),
            is_directory: bool_byte(event.is_directory),
            rescan_directory: self.push_path(&event.rescan_directory),
        }
    }

    fn ignored_watch_path(&mut self, ignored: &IgnoredWatchPath) -> SmkIgnoredWatchPath {
        SmkIgnoredWatchPath {
            has_root_id: bool_byte(ignored.root_id.is_some()),
            root_id: self.push_string(ignored.root_id.as_deref()),
            path: self.push_path(&ignored.path),
            reason: static_slice(watch_ignore_reason(ignored.reason)),
        }
    }

    fn watch_rename_candidate(
        &mut self,
        candidate: &WatchRenameCandidate,
    ) -> SmkWatchRenameCandidate {
        SmkWatchRenameCandidate {
            root_id: self.push_string(Some(candidate.root_id.as_ref())),
            old_path: self.push_path(&candidate.old_path),
            new_path: self.push_path(&candidate.new_path),
            confidence: static_slice(watch_rename_confidence(candidate.confidence)),
        }
    }

    fn watch_rescan_target(&mut self, target: &WatchRescanTarget) -> SmkWatchRescanTarget {
        SmkWatchRescanTarget {
            root_id: self.push_string(Some(target.root_id.as_ref())),
            path: self.push_path(&target.path),
            reason: static_slice(watch_rescan_reason(target.reason)),
        }
    }

    fn file_identity(&mut self, identity: &FileIdentity) -> SmkFileIdentity {
        let (has_file_size, file_size) = optional_u64(identity.file_size);
        let (has_content_modified_at, content_modified_at) =
            optional_u64(identity.content_modified_at);
        SmkFileIdentity {
            stable_id: self.push_string(Some(identity.stable_id.as_str())),
            volume_id: self.push_string(identity.volume_id.as_deref()),
            file_id: self.push_string(identity.file_id.as_deref()),
            has_file_size,
            file_size,
            has_content_modified_at,
            content_modified_at,
        }
    }

    fn push_update_result(&mut self, update_result: &UpdateCheckResult) {
        self.update_info = SmkUpdateCheckInfo {
            has_update_check: 1,
            checked_at: update_result.checked_at,
        };
        self.update_statuses
            .reserve(update_result.statuses_by_item_id.len());
        self.update_resolutions
            .reserve(update_result.resolutions_by_item_id.len());
        self.update_failures
            .reserve(update_result.failures_by_item_id.len());
        self.update_errors
            .reserve(update_result.errors_by_item_id.len());
        self.latest_url_history_urls.reserve(
            update_result
                .resolutions_by_item_id
                .values()
                .map(|resolution| resolution.latest_url_history.len())
                .sum(),
        );

        for (item_id, status) in &update_result.statuses_by_item_id {
            let entry = SmkUpdateStatusEntry {
                item_id: self.push_string(Some(item_id.as_str())),
                status: static_slice(update_status(*status)),
            };
            self.update_statuses.push(entry);
        }

        for (item_id, resolution) in &update_result.resolutions_by_item_id {
            self.push_distribution_resolution(item_id, resolution);
        }

        for (item_id, failure) in &update_result.failures_by_item_id {
            self.push_update_failure(item_id, failure);
        }

        for (item_id, message) in &update_result.errors_by_item_id {
            let entry = SmkUpdateErrorEntry {
                item_id: self.push_string(Some(item_id.as_str())),
                message: self.push_string(Some(message.as_str())),
            };
            self.update_errors.push(entry);
        }
    }

    fn push_distribution_resolution(&mut self, item_id: &str, resolution: &DistributionResolution) {
        let first_latest_url_history_index = self.latest_url_history_urls.len();
        for url in &resolution.latest_url_history {
            let slice = self.push_url(Some(url));
            self.latest_url_history_urls.push(slice);
        }
        let (has_redirect_count, redirect_count) = optional_u32(resolution.redirect_count);
        let entry = SmkDistributionResolutionEntry {
            item_id: self.push_string(Some(item_id)),
            latest_version: self.push_string(resolution.latest_version.as_deref()),
            latest_page_url: self.push_url(resolution.latest_page_url.as_ref()),
            final_page_url: self.push_url(Some(&resolution.final_page_url)),
            first_latest_url_history_index,
            latest_url_history_count: resolution.latest_url_history.len(),
            checked_at: resolution.checked_at,
            is_unresolved: bool_byte(resolution.is_unresolved),
            note: self.push_string(resolution.note.as_deref()),
            has_redirect_count,
            redirect_count,
        };
        self.update_resolutions.push(entry);
    }

    fn push_update_failure(&mut self, item_id: &str, failure: &UpdateFailure) {
        let entry = SmkUpdateFailureEntry {
            item_id: self.push_string(Some(item_id)),
            code: self.push_string(Some(failure.code.as_str())),
            message: self.push_string(Some(failure.message.as_str())),
            file_path: self.push_path(&failure.file_path),
            script_id: self.push_string(Some(failure.script_id.as_str())),
            current_version: self.push_string(failure.current_version.as_deref()),
            meta_url: self.push_url(failure.meta_url.as_ref()),
            source_url: self.push_url(failure.source_url.as_ref()),
            checked_at: failure.checked_at,
        };
        self.update_failures.push(entry);
    }

    fn push_string(&mut self, value: Option<&str>) -> SmkUtf8Slice {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return SmkUtf8Slice::empty();
        };
        if let Some(slice) = self.string_index.get(value) {
            return *slice;
        }
        let slice = push_stable_string(&mut self.string_storage, &mut self.string_overflow, value);
        self.string_index.insert(value.to_string(), slice);
        slice
    }

    fn push_path(&mut self, path: &Path) -> SmkUtf8Slice {
        let value = path.to_string_lossy();
        self.push_string(Some(value.as_ref()))
    }

    fn push_url(&mut self, url: Option<&Url>) -> SmkUtf8Slice {
        self.push_string(url.map(Url::as_str))
    }
}

impl SmkScriptIdUniquenessResult {
    fn from_report(report: &scriptmetakit::ScriptIdUniquenessReport) -> Self {
        let string_capacity = report
            .duplicates
            .iter()
            .map(|duplicate| {
                duplicate.script_id.len()
                    + duplicate.item_ids.iter().map(String::len).sum::<usize>()
                    + duplicate
                        .file_paths
                        .iter()
                        .map(|path| path.to_string_lossy().len())
                        .sum::<usize>()
            })
            .sum();
        let mut result = Self {
            string_storage: Vec::with_capacity(string_capacity),
            string_overflow: Vec::new(),
            report: SmkScriptIdUniquenessReport {
                total_items: report.total_items,
                unique_script_ids: report.unique_script_ids,
                duplicate_count: report.duplicates.len(),
            },
            duplicates: Vec::with_capacity(report.duplicates.len()),
            item_ids: Vec::new(),
            file_paths: Vec::new(),
        };

        for duplicate in &report.duplicates {
            let first_item_id_index = result.item_ids.len();
            for item_id in &duplicate.item_ids {
                let item_id = result.push_string(item_id);
                result.item_ids.push(item_id);
            }

            let first_file_path_index = result.file_paths.len();
            for file_path in &duplicate.file_paths {
                let file_path = file_path.to_string_lossy();
                let file_path = result.push_string(file_path.as_ref());
                result.file_paths.push(file_path);
            }

            let ffi_duplicate = SmkScriptIdDuplicate {
                script_id: result.push_string(&duplicate.script_id),
                first_item_id_index,
                item_id_count: duplicate.item_ids.len(),
                first_file_path_index,
                file_path_count: duplicate.file_paths.len(),
            };
            result.duplicates.push(ffi_duplicate);
        }

        result
    }

    fn push_string(&mut self, value: &str) -> SmkUtf8Slice {
        push_stable_string(&mut self.string_storage, &mut self.string_overflow, value)
    }
}

impl SmkEditResult {
    fn empty(string_capacity: usize, generation_capacity: usize) -> Self {
        Self {
            string_storage: Vec::with_capacity(string_capacity),
            string_overflow: Vec::new(),
            text: SmkUtf8Slice::empty(),
            file_write_result: SmkScriptMetadataFileWriteResult::default(),
            edit_read_result: SmkScriptMetadataEditReadResult::default(),
            edit_preview_result: SmkScriptMetadataEditPreviewResult::default(),
            existing_lines: Vec::new(),
            unknown_lines: Vec::new(),
            has_backup_record: 0,
            backup_record: SmkScriptMetaBackupRecord::default(),
            backup_generations: Vec::with_capacity(generation_capacity),
        }
    }

    fn from_text(text: &str) -> Self {
        let mut result = Self::empty(text.len(), 0);
        result.text = result.push_string(Some(text));
        result
    }

    fn from_file_write_result(value: &KitScriptMetadataFileWriteResult) -> Self {
        let capacity = file_write_result_string_bytes(value);
        let mut result = Self::empty(capacity, 0);
        result.file_write_result = result.file_write_result(value);
        result
    }

    fn from_edit_read_result(value: &KitScriptMetadataEditReadResult) -> Self {
        let capacity = edit_read_result_string_bytes(value);
        let mut result = Self::empty(capacity, 0);
        result.edit_read_result = result.edit_read_result(value);
        for line in &value.existing_lines {
            let line = result.push_string(Some(line.as_str()));
            result.existing_lines.push(line);
        }
        for line in &value.unknown_lines {
            let line = result.push_string(Some(line.as_str()));
            result.unknown_lines.push(line);
        }
        result
    }

    fn from_edit_preview_result(value: &KitScriptMetadataEditPreviewResult) -> Self {
        let capacity = edit_preview_result_string_bytes(value);
        let mut result = Self::empty(capacity, 0);
        result.edit_preview_result = result.edit_preview_result(value);
        result
    }

    fn from_backup_record(value: &KitScriptMetaBackupRecord) -> Self {
        let capacity = backup_record_string_bytes(value);
        let mut result = Self::empty(capacity, 0);
        result.has_backup_record = 1;
        result.backup_record = result.backup_record(value);
        result
    }

    fn from_backup_generations(values: &[KitScriptMetaBackupGeneration]) -> Self {
        let capacity = values
            .iter()
            .map(backup_generation_string_bytes)
            .fold(0usize, usize::saturating_add);
        let mut result = Self::empty(capacity, values.len());
        for generation in values {
            let generation = result.backup_generation(generation);
            result.backup_generations.push(generation);
        }
        result
    }

    fn file_write_result(
        &mut self,
        value: &KitScriptMetadataFileWriteResult,
    ) -> SmkScriptMetadataFileWriteResult {
        let backup = value
            .backup
            .as_ref()
            .map(|record| self.backup_record(record))
            .unwrap_or_default();
        SmkScriptMetadataFileWriteResult {
            file_path: self.push_path(&value.file_path),
            operation: static_slice(script_meta_write_operation(value.operation)),
            has_backup: bool_byte(value.backup.is_some()),
            backup,
        }
    }

    fn edit_read_result(
        &mut self,
        value: &KitScriptMetadataEditReadResult,
    ) -> SmkScriptMetadataEditReadResult {
        SmkScriptMetadataEditReadResult {
            file_path: self.push_path(&value.file_path),
            draft: self.script_metadata_draft(&value.draft),
            comment_style: static_slice(script_meta_comment_style(value.comment_style)),
            line_ending: self.push_string(Some(value.line_ending.as_str())),
            has_existing_block: bool_byte(value.has_existing_block),
            existing_block_text: self.push_string(value.existing_block_text.as_deref()),
            source_fingerprint: self.push_string(Some(value.source_fingerprint.as_str())),
        }
    }

    fn edit_preview_result(
        &mut self,
        value: &KitScriptMetadataEditPreviewResult,
    ) -> SmkScriptMetadataEditPreviewResult {
        SmkScriptMetadataEditPreviewResult {
            file_path: self.push_path(&value.file_path),
            preview_text: self.push_string(Some(value.preview_text.as_str())),
            preview_byte_count: value.preview_byte_count,
            file_size: value.file_size.unwrap_or_default(),
            has_file_size: bool_byte(value.file_size.is_some()),
            comment_style: value
                .comment_style
                .map(|style| static_slice(script_meta_comment_style(style)))
                .unwrap_or_else(SmkUtf8Slice::empty),
            line_ending: self.push_string(Some(value.line_ending.as_str())),
            has_scriptmeta_marker_in_preview: bool_byte(value.has_scriptmeta_marker_in_preview),
            is_truncated: bool_byte(value.is_truncated),
            requires_full_read: bool_byte(value.requires_full_read),
            file_state_fingerprint: self.push_string(Some(value.file_state_fingerprint.as_str())),
        }
    }

    fn script_metadata_draft(&mut self, draft: &KitScriptMetadataDraft) -> SmkScriptMetadataDraft {
        SmkScriptMetadataDraft {
            script_id: self.push_string(Some(draft.script_id.as_str())),
            version: self.push_string(draft.version.as_deref()),
            description: self.push_string(draft.description.as_deref()),
            target_app: self.push_string(draft.target_app.as_deref()),
            min_target_version: self.push_string(draft.min_target_version.as_deref()),
            meta_url: self.push_string(draft.meta_url.as_ref().map(Url::as_str)),
            name: self.push_string(draft.name.as_deref()),
            author: self.push_string(draft.author.as_deref()),
            release_date: self.push_string(draft.release_date.as_deref()),
            edit_password_sha256: self.push_string(draft.edit_password_sha256.as_deref()),
        }
    }

    fn backup_record(&mut self, value: &KitScriptMetaBackupRecord) -> SmkScriptMetaBackupRecord {
        SmkScriptMetaBackupRecord {
            id: self.push_string(Some(value.id.as_str())),
            created_at_millis: value.created_at_millis,
            backup_file_name: self.push_string(Some(value.backup_file_name.as_str())),
            backup_file_path: self.push_string(Some(&value.backup_file_path.to_string_lossy())),
            file_size: value.file_size,
            reason: static_slice(script_meta_backup_reason_string(value.reason)),
        }
    }

    fn backup_generation(
        &mut self,
        value: &KitScriptMetaBackupGeneration,
    ) -> SmkScriptMetaBackupGeneration {
        SmkScriptMetaBackupGeneration {
            id: self.push_string(Some(value.id.as_str())),
            sequence_number: value.sequence_number,
            created_at_millis: value.created_at_millis,
            file_path: self.push_path(&value.file_path),
            file_size: value.file_size,
            reason: static_slice(script_meta_backup_reason_string(value.reason)),
            is_current_file: bool_byte(value.is_current_file),
        }
    }

    fn push_string(&mut self, value: Option<&str>) -> SmkUtf8Slice {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return SmkUtf8Slice::empty();
        };
        push_stable_string(&mut self.string_storage, &mut self.string_overflow, value)
    }

    fn push_path(&mut self, path: &Path) -> SmkUtf8Slice {
        let value = path.to_string_lossy();
        self.push_string(Some(value.as_ref()))
    }
}

fn static_slice(value: &'static str) -> SmkUtf8Slice {
    borrowed_slice(value.as_bytes())
}

fn push_stable_string(
    storage: &mut Vec<u8>,
    overflow: &mut Vec<Box<[u8]>>,
    value: &str,
) -> SmkUtf8Slice {
    if storage.len().saturating_add(value.len()) <= storage.capacity() {
        let start = storage.len();
        storage.extend_from_slice(value.as_bytes());
        return borrowed_slice(&storage[start..]);
    }

    let bytes = value.as_bytes().to_vec().into_boxed_slice();
    let slice = borrowed_slice(&bytes);
    overflow.push(bytes);
    slice
}

fn total_file_entry_count(scan_result: &ScanResult) -> usize {
    scan_result
        .file_list_snapshots
        .iter()
        .map(|snapshot| snapshot.children.as_deref().map_or(0, file_entry_count))
        .sum()
}

fn file_entry_count(entries: &[FileSystemEntry]) -> usize {
    entries
        .iter()
        .map(|entry| 1usize.saturating_add(file_entry_count(&entry.children)))
        .sum()
}

fn total_string_bytes(
    scan_result: &ScanResult,
    update_result: Option<&UpdateCheckResult>,
) -> usize {
    let root_bytes = scan_result
        .roots
        .iter()
        .map(root_string_bytes)
        .sum::<usize>();
    let file_list_bytes = scan_result
        .file_list_snapshots
        .iter()
        .flat_map(|snapshot| snapshot.children.as_deref().unwrap_or_default())
        .map(file_entry_string_bytes)
        .sum::<usize>();
    let item_bytes = scan_result.catalog_snapshot.as_ref().map_or(0, |snapshot| {
        let all_item_keys: BTreeSet<_> = snapshot
            .all_items
            .iter()
            .map(|item| std::sync::Arc::as_ptr(item) as usize)
            .collect();
        let all_item_bytes = snapshot
            .all_items
            .iter()
            .map(|item| item_string_bytes(item))
            .sum::<usize>();
        let file_item_bytes = snapshot
            .file_items
            .iter()
            .filter(|item| !all_item_keys.contains(&(std::sync::Arc::as_ptr(item) as usize)))
            .map(|item| item_string_bytes(item))
            .sum::<usize>();
        let candidate_record_bytes = snapshot
            .candidate_cache
            .records
            .iter()
            .map(candidate_record_string_bytes)
            .sum::<usize>();
        all_item_bytes + file_item_bytes + candidate_record_bytes
    });
    let change_bytes = scan_result
        .change_summary
        .as_ref()
        .map_or(0, scan_change_summary_string_bytes);
    let watch_bytes = scan_result
        .watch_change_batch
        .as_ref()
        .map_or(0, watch_change_batch_string_bytes);
    let operation_bytes = update_result
        .map(|result| &result.operation)
        .unwrap_or(&scan_result.operation)
        .reason_code
        .as_deref()
        .map_or(0, str::len)
        .saturating_add(
            update_result
                .map(|result| &result.operation)
                .unwrap_or(&scan_result.operation)
                .message
                .as_deref()
                .map_or(0, str::len),
        );
    let file_issue_bytes = scan_result
        .file_issues
        .iter()
        .map(file_issue_string_bytes)
        .sum::<usize>();
    let update_bytes = update_result.map_or(0, update_result_string_bytes);
    root_bytes
        .saturating_add(file_list_bytes)
        .saturating_add(item_bytes)
        .saturating_add(change_bytes)
        .saturating_add(operation_bytes)
        .saturating_add(file_issue_bytes)
        .saturating_add(watch_bytes)
        .saturating_add(update_bytes)
}

fn root_string_bytes(root: &RootSnapshot) -> usize {
    root.root_id
        .len()
        .saturating_add(root.path.to_string_lossy().len())
        .saturating_add(root.error.as_ref().map_or(0, |error| {
            error.code.len().saturating_add(error.message.len())
        }))
}

fn file_entry_string_bytes(entry: &FileSystemEntry) -> usize {
    entry
        .display_path
        .to_string_lossy()
        .len()
        .saturating_add(entry.resolved_path.to_string_lossy().len())
        .saturating_add(entry.resolution_message.as_deref().map_or(0, str::len))
        .saturating_add(entry.shebang.as_deref().map_or(0, str::len))
        .saturating_add(
            entry
                .scriptmeta_item
                .as_ref()
                .map_or(0, |item| item_string_bytes(item)),
        )
        .saturating_add(
            entry
                .identity
                .as_ref()
                .map_or(0, file_identity_string_bytes),
        )
        .saturating_add(
            entry
                .children
                .iter()
                .map(file_entry_string_bytes)
                .sum::<usize>(),
        )
}

fn item_string_bytes(item: &ScriptMetaItem) -> usize {
    [
        item.root_id.len(),
        item.file_path.to_string_lossy().len(),
        item.identity_path.to_string_lossy().len(),
        item.shebang.as_deref().map_or(0, str::len),
        item.script_id.len(),
        item.version.as_deref().map_or(0, str::len),
        item.name.as_deref().map_or(0, str::len),
        item.description.as_deref().map_or(0, str::len),
        item.target_app.as_deref().map_or(0, str::len),
        item.min_target_version.as_deref().map_or(0, str::len),
        item.meta_url.as_ref().map_or(0, |url| url.as_str().len()),
        item.author.as_deref().map_or(0, str::len),
        item.release_date.as_deref().map_or(0, str::len),
        item.edit_password_sha256.as_deref().map_or(0, str::len),
    ]
    .into_iter()
    .fold(0usize, usize::saturating_add)
}

fn candidate_record_string_bytes(record: &CandidateRecord) -> usize {
    [
        record.root_id.len(),
        record.root_path.to_string_lossy().len(),
        record.file_path.to_string_lossy().len(),
        record.identity_path.to_string_lossy().len(),
        record.resolution_message.as_deref().map_or(0, str::len),
        record.shebang.as_deref().map_or(0, str::len),
        record
            .item
            .as_ref()
            .map_or(0, |item| item_string_bytes(item)),
    ]
    .into_iter()
    .fold(0usize, usize::saturating_add)
}

fn scan_change_summary_string_bytes(summary: &ScanChangeSummary) -> usize {
    summary
        .changes
        .iter()
        .map(file_entry_change_string_bytes)
        .sum::<usize>()
}

fn file_entry_change_string_bytes(change: &FileEntryChange) -> usize {
    change
        .root_id
        .len()
        .saturating_add(change.display_path.to_string_lossy().len())
        .saturating_add(change.resolved_path.to_string_lossy().len())
        .saturating_add(change.resolution_message.as_deref().map_or(0, str::len))
        .saturating_add(change.shebang.as_deref().map_or(0, str::len))
        .saturating_add(
            change
                .identity
                .as_ref()
                .map_or(0, file_identity_string_bytes),
        )
}

fn file_identity_string_bytes(identity: &FileIdentity) -> usize {
    identity
        .stable_id
        .len()
        .saturating_add(identity.volume_id.as_deref().map_or(0, str::len))
        .saturating_add(identity.file_id.as_deref().map_or(0, str::len))
}

fn file_issue_string_bytes(issue: &FileIssue) -> usize {
    issue
        .root_id
        .as_deref()
        .map_or(0, str::len)
        .saturating_add(issue.path.to_string_lossy().len())
        .saturating_add(issue.code.len())
        .saturating_add(issue.message.len())
        .saturating_add(issue.path_kind.as_deref().map_or(0, str::len))
        .saturating_add(issue.resolution_status.as_deref().map_or(0, str::len))
}

fn watch_change_batch_string_bytes(batch: &RootChangeBatch) -> usize {
    batch
        .events
        .iter()
        .map(watch_event_string_bytes)
        .sum::<usize>()
        .saturating_add(
            batch
                .ignored_paths
                .iter()
                .map(ignored_watch_path_string_bytes)
                .sum::<usize>(),
        )
        .saturating_add(
            batch
                .rename_candidates
                .iter()
                .map(watch_rename_candidate_string_bytes)
                .sum::<usize>(),
        )
        .saturating_add(
            batch
                .affected_roots
                .iter()
                .flat_map(|root| root.rescan_targets.iter())
                .map(watch_rescan_target_string_bytes)
                .sum::<usize>(),
        )
}

fn watch_event_string_bytes(event: &WatchPathEvent) -> usize {
    event
        .root_id
        .len()
        .saturating_add(event.path.to_string_lossy().len())
        .saturating_add(event.rescan_directory.to_string_lossy().len())
}

fn ignored_watch_path_string_bytes(path: &IgnoredWatchPath) -> usize {
    path.root_id
        .as_deref()
        .map_or(0, str::len)
        .saturating_add(path.path.to_string_lossy().len())
}

fn watch_rename_candidate_string_bytes(candidate: &WatchRenameCandidate) -> usize {
    candidate
        .root_id
        .len()
        .saturating_add(candidate.old_path.to_string_lossy().len())
        .saturating_add(candidate.new_path.to_string_lossy().len())
}

fn watch_rescan_target_string_bytes(target: &WatchRescanTarget) -> usize {
    target
        .root_id
        .len()
        .saturating_add(target.path.to_string_lossy().len())
}

fn update_result_string_bytes(update_result: &UpdateCheckResult) -> usize {
    let statuses = update_result
        .statuses_by_item_id
        .keys()
        .map(String::len)
        .sum::<usize>();
    let resolutions = update_result
        .resolutions_by_item_id
        .iter()
        .map(|(item_id, resolution)| {
            item_id
                .len()
                .saturating_add(resolution.latest_version.as_deref().map_or(0, str::len))
                .saturating_add(
                    resolution
                        .latest_page_url
                        .as_ref()
                        .map_or(0, |url| url.as_str().len()),
                )
                .saturating_add(resolution.final_page_url.as_str().len())
                .saturating_add(
                    resolution
                        .latest_url_history
                        .iter()
                        .map(|url| url.as_str().len())
                        .sum::<usize>(),
                )
                .saturating_add(resolution.note.as_deref().map_or(0, str::len))
        })
        .sum::<usize>();
    let failures = update_result
        .failures_by_item_id
        .iter()
        .map(|(item_id, failure)| {
            item_id
                .len()
                .saturating_add(failure.code.len())
                .saturating_add(failure.message.len())
                .saturating_add(failure.file_path.to_string_lossy().len())
                .saturating_add(failure.script_id.len())
                .saturating_add(failure.current_version.as_deref().map_or(0, str::len))
                .saturating_add(
                    failure
                        .meta_url
                        .as_ref()
                        .map_or(0, |url| url.as_str().len()),
                )
                .saturating_add(
                    failure
                        .source_url
                        .as_ref()
                        .map_or(0, |url| url.as_str().len()),
                )
        })
        .sum::<usize>();
    let errors = update_result
        .errors_by_item_id
        .iter()
        .map(|(item_id, message)| item_id.len().saturating_add(message.len()))
        .sum::<usize>();
    statuses
        .saturating_add(resolutions)
        .saturating_add(failures)
        .saturating_add(errors)
}

fn file_write_result_string_bytes(value: &KitScriptMetadataFileWriteResult) -> usize {
    value
        .file_path
        .to_string_lossy()
        .len()
        .saturating_add(value.backup.as_ref().map_or(0, backup_record_string_bytes))
}

fn edit_read_result_string_bytes(value: &KitScriptMetadataEditReadResult) -> usize {
    value
        .file_path
        .to_string_lossy()
        .len()
        .saturating_add(script_metadata_draft_string_bytes(&value.draft))
        .saturating_add(value.line_ending.len())
        .saturating_add(value.existing_block_text.as_deref().map_or(0, str::len))
        .saturating_add(value.source_fingerprint.len())
        .saturating_add(value.existing_lines.iter().map(String::len).sum::<usize>())
        .saturating_add(value.unknown_lines.iter().map(String::len).sum::<usize>())
}

fn edit_preview_result_string_bytes(value: &KitScriptMetadataEditPreviewResult) -> usize {
    value
        .file_path
        .to_string_lossy()
        .len()
        .saturating_add(value.preview_text.len())
        .saturating_add(value.line_ending.len())
        .saturating_add(value.file_state_fingerprint.len())
}

fn script_metadata_draft_string_bytes(draft: &KitScriptMetadataDraft) -> usize {
    draft
        .script_id
        .len()
        .saturating_add(draft.version.as_deref().map_or(0, str::len))
        .saturating_add(draft.description.as_deref().map_or(0, str::len))
        .saturating_add(draft.target_app.as_deref().map_or(0, str::len))
        .saturating_add(draft.min_target_version.as_deref().map_or(0, str::len))
        .saturating_add(draft.meta_url.as_ref().map_or(0, |url| url.as_str().len()))
        .saturating_add(draft.name.as_deref().map_or(0, str::len))
        .saturating_add(draft.author.as_deref().map_or(0, str::len))
        .saturating_add(draft.release_date.as_deref().map_or(0, str::len))
        .saturating_add(draft.edit_password_sha256.as_deref().map_or(0, str::len))
}

fn backup_record_string_bytes(value: &KitScriptMetaBackupRecord) -> usize {
    value
        .id
        .len()
        .saturating_add(value.backup_file_name.len())
        .saturating_add(value.backup_file_path.to_string_lossy().len())
}

fn backup_generation_string_bytes(value: &KitScriptMetaBackupGeneration) -> usize {
    value
        .id
        .len()
        .saturating_add(value.file_path.to_string_lossy().len())
}

impl SmkEngine {
    fn clear_error(&mut self) {
        self.last_error.clear();
    }

    fn set_error(&mut self, message: &str) {
        self.last_error.clear();
        self.last_error.extend_from_slice(message.as_bytes());
    }
}

fn ffi_guard<F>(operation: F) -> SmkStatus
where
    F: FnOnce() -> Result<(), (SmkStatus, String)>,
{
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => SmkStatus::Ok,
        Ok(Err((status, _message))) => status,
        Err(_) => SmkStatus::Panic,
    }
}

fn out_mut<'a, T>(ptr: *mut T) -> Result<&'a mut T, (SmkStatus, String)> {
    if ptr.is_null() {
        return Err((
            SmkStatus::NullArgument,
            "output pointer is null".to_string(),
        ));
    }
    // SAFETY: the caller provided a non-null output pointer and C ABI requires it to be writable.
    Ok(unsafe { &mut *ptr })
}

fn engine_mut<'a>(engine: *mut SmkEngine) -> Result<&'a mut SmkEngine, (SmkStatus, String)> {
    if engine.is_null() {
        return Err((SmkStatus::NullArgument, "engine handle is null".to_string()));
    }
    // SAFETY: the caller must pass a live `SmkEngine` returned by this crate.
    Ok(unsafe { &mut *engine })
}

fn engine_ref<'a>(engine: *const SmkEngine) -> Result<&'a SmkEngine, (SmkStatus, String)> {
    if engine.is_null() {
        return Err((SmkStatus::NullArgument, "engine handle is null".to_string()));
    }
    // SAFETY: the caller must pass a live `SmkEngine` returned by this crate.
    Ok(unsafe { &*engine })
}

fn scan_result_ref<'a>(
    result: *const SmkScanResult,
) -> Result<&'a SmkScanResult, (SmkStatus, String)> {
    if result.is_null() {
        return Err((
            SmkStatus::NullArgument,
            "scan result handle is null".to_string(),
        ));
    }
    // SAFETY: the caller must pass a live `SmkScanResult` returned by this crate.
    Ok(unsafe { &*result })
}

fn edit_result_ref<'a>(
    result: *const SmkEditResult,
) -> Result<&'a SmkEditResult, (SmkStatus, String)> {
    if result.is_null() {
        return Err((
            SmkStatus::NullArgument,
            "edit result handle is null".to_string(),
        ));
    }
    // SAFETY: the caller must pass a live `SmkEditResult` returned by this crate.
    Ok(unsafe { &*result })
}

fn input_ref<'a, T>(ptr: *const T, name: &str) -> Result<&'a T, (SmkStatus, String)> {
    if ptr.is_null() {
        return Err((SmkStatus::NullArgument, format!("{name} is null")));
    }
    // SAFETY: the caller promises `ptr` points to a readable value for this call.
    Ok(unsafe { &*ptr })
}

fn utf8_path_slices(
    ptr: *const SmkUtf8Slice,
    len: usize,
) -> Result<Vec<PathBuf>, (SmkStatus, String)> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if ptr.is_null() {
        return Err((SmkStatus::NullArgument, "path slice is null".to_string()));
    }
    // SAFETY: the caller promises `ptr` points to `len` readable path slices.
    let slices = unsafe { slice::from_raw_parts(ptr, len) };
    let mut paths = Vec::with_capacity(slices.len());
    for value in slices {
        let path = utf8_from_raw(value.ptr, value.len)?;
        if !path.is_empty() {
            paths.push(PathBuf::from(path));
        }
    }
    Ok(paths)
}

fn root_registrations_from_raw(
    ptr: *const SmkRootRegistration,
    len: usize,
) -> Result<Vec<RootRegistration>, (SmkStatus, String)> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if ptr.is_null() {
        return Err((
            SmkStatus::NullArgument,
            "root registration slice is null".to_string(),
        ));
    }
    // SAFETY: the caller promises `ptr` points to `len` readable root registrations.
    let roots = unsafe { slice::from_raw_parts(ptr, len) };
    roots
        .iter()
        .map(root_registration_from_ffi)
        .collect::<Result<Vec<_>, _>>()
}

fn root_ids_from_raw(
    ptr: *const SmkUtf8Slice,
    len: usize,
) -> Result<Vec<RootId>, (SmkStatus, String)> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if ptr.is_null() {
        return Err((SmkStatus::NullArgument, "root id slice is null".to_string()));
    }
    // SAFETY: the caller promises `ptr` points to `len` readable root id slices.
    let root_ids = unsafe { slice::from_raw_parts(ptr, len) };
    root_ids
        .iter()
        .map(|root_id| required_str_from_slice(*root_id, "root_id").map(RootId::from))
        .collect::<Result<Vec<_>, _>>()
}

fn script_items_for_uniqueness_from_raw(
    ptr: *const SmkScriptIdUniquenessItem,
    len: usize,
) -> Result<Vec<ScriptIdUniquenessItem>, (SmkStatus, String)> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if ptr.is_null() {
        return Err((SmkStatus::NullArgument, "item slice is null".to_string()));
    }
    // SAFETY: the caller promises `ptr` points to `len` readable uniqueness items.
    let items = unsafe { slice::from_raw_parts(ptr, len) };
    items
        .iter()
        .map(script_item_for_uniqueness_from_ffi)
        .collect::<Result<Vec<_>, _>>()
}

fn root_registration_from_ffi(
    root: &SmkRootRegistration,
) -> Result<RootRegistration, (SmkStatus, String)> {
    Ok(RootRegistration {
        root_id: required_str_from_slice(root.root_id, "root_id")?.into(),
        path: required_path_from_slice(root.path, "root path")?,
        display_name: optional_string_from_slice(root.display_name)?,
        purpose: root_purpose_from_u32(root.purpose)?,
        watch_policy: watch_policy_from_u32(root.watch_policy)?,
        cache_policy: cache_policy_from_u32(root.cache_policy)?,
        refresh_policy: refresh_policy_from_u32(root.refresh_policy)?,
        priority: root_priority_from_u32(root.priority)?,
    })
}

fn path_from_slice(value: SmkUtf8Slice) -> Result<PathBuf, (SmkStatus, String)> {
    required_path_from_slice(value, "file path")
}

fn required_path_from_slice(
    value: SmkUtf8Slice,
    name: &str,
) -> Result<PathBuf, (SmkStatus, String)> {
    let value = required_str_from_slice(value, name)?;
    Ok(PathBuf::from(value))
}

fn optional_path_from_slice(value: SmkUtf8Slice) -> Result<Option<PathBuf>, (SmkStatus, String)> {
    optional_str_from_slice(value).map(|value| value.map(PathBuf::from))
}

fn backup_options_from_slice(
    value: SmkUtf8Slice,
) -> Result<ScriptMetaBackupOptions, (SmkStatus, String)> {
    let root_directory = required_path_from_slice(value, "backup root path")?;
    Ok(ScriptMetaBackupOptions { root_directory })
}

fn required_str_from_slice<'a>(
    value: SmkUtf8Slice,
    name: &str,
) -> Result<&'a str, (SmkStatus, String)> {
    let value = utf8_from_raw(value.ptr, value.len)?;
    if value.is_empty() {
        return Err((SmkStatus::InvalidArgument, format!("{name} is empty")));
    }
    Ok(value)
}

fn optional_str_from_slice<'a>(
    value: SmkUtf8Slice,
) -> Result<Option<&'a str>, (SmkStatus, String)> {
    let value = utf8_from_raw(value.ptr, value.len)?;
    Ok((!value.is_empty()).then_some(value))
}

fn optional_string_from_slice(value: SmkUtf8Slice) -> Result<Option<String>, (SmkStatus, String)> {
    optional_str_from_slice(value).map(|value| value.map(ToOwned::to_owned))
}

fn optional_version_from_slice(
    value: SmkUtf8Slice,
    field_name: &str,
) -> Result<Option<String>, (SmkStatus, String)> {
    let Some(value) = optional_str_from_slice(value)? else {
        return Ok(None);
    };
    normalize_version_string(value).map(Some).ok_or_else(|| {
        (
            SmkStatus::InvalidArgument,
            format!("{field_name}: invalid version"),
        )
    })
}

fn optional_url_from_slice(
    value: SmkUtf8Slice,
    field_name: &str,
) -> Result<Option<Url>, (SmkStatus, String)> {
    let Some(value) = optional_str_from_slice(value)? else {
        return Ok(None);
    };
    normalize_metadata_url(value).map(Some).ok_or_else(|| {
        (
            SmkStatus::InvalidArgument,
            format!("{field_name}: invalid URL"),
        )
    })
}

fn script_item_from_ffi(item: &SmkScriptItem) -> Result<ScriptMetaItem, (SmkStatus, String)> {
    let file_path = required_path_from_slice(item.file_path, "file_path")?;
    let identity_path =
        optional_path_from_slice(item.identity_path)?.unwrap_or_else(|| file_path.clone());
    Ok(ScriptMetaItem {
        root_id: required_str_from_slice(item.root_id, "root_id")?.into(),
        file_path,
        identity_path,
        runtime_kind: optional_script_runtime_kind_from_slice(item.runtime_kind)?,
        shebang: optional_string_from_slice(item.shebang)?,
        script_id: required_str_from_slice(item.script_id, "Script-ID")?.to_string(),
        version: optional_version_from_slice(item.version, "Version")?,
        description: optional_string_from_slice(item.description)?,
        target_app: optional_string_from_slice(item.target_app)?,
        min_target_version: optional_version_from_slice(
            item.min_target_version,
            "Min-Target-Version",
        )?,
        meta_url: optional_url_from_slice(item.meta_url, "Meta-URL")?,
        name: optional_string_from_slice(item.name)?,
        author: optional_string_from_slice(item.author)?,
        release_date: optional_string_from_slice(item.release_date)?,
        edit_password_sha256: optional_string_from_slice(item.edit_password_sha256)?,
        has_scriptmeta: item.has_scriptmeta != 0,
        has_scriptmeta_edit_password: item.has_scriptmeta_edit_password != 0,
        is_file_locked: item.is_file_locked != 0,
        is_read_only: item.is_read_only != 0,
        can_edit_scriptmeta: item.can_edit_scriptmeta != 0,
        can_append_scriptmeta: item.can_append_scriptmeta != 0,
        scriptmeta_edit_state: script_meta_edit_state_from_slice(item.scriptmeta_edit_state)?,
    })
}

fn script_item_for_uniqueness_from_ffi(
    item: &SmkScriptIdUniquenessItem,
) -> Result<ScriptIdUniquenessItem, (SmkStatus, String)> {
    let file_path = required_path_from_slice(item.file_path, "file_path")?;
    let item_id = optional_string_from_slice(item.item_id)?
        .unwrap_or_else(|| file_path.to_string_lossy().into_owned());
    Ok(ScriptIdUniquenessItem {
        item_id,
        file_path,
        script_id: optional_string_from_slice(item.script_id)?.unwrap_or_default(),
    })
}

fn script_metadata_draft_from_ffi(
    draft: &SmkScriptMetadataDraft,
) -> Result<KitScriptMetadataDraft, (SmkStatus, String)> {
    Ok(KitScriptMetadataDraft {
        script_id: required_str_from_slice(draft.script_id, "Script-ID")?.to_string(),
        version: optional_version_from_slice(draft.version, "Version")?,
        description: optional_string_from_slice(draft.description)?,
        target_app: optional_string_from_slice(draft.target_app)?,
        min_target_version: optional_version_from_slice(
            draft.min_target_version,
            "Min-Target-Version",
        )?,
        meta_url: optional_url_from_slice(draft.meta_url, "Meta-URL")?,
        name: optional_string_from_slice(draft.name)?,
        author: optional_string_from_slice(draft.author)?,
        release_date: optional_string_from_slice(draft.release_date)?,
        edit_password_sha256: optional_string_from_slice(draft.edit_password_sha256)?,
    })
}

fn distribution_metadata_draft_from_ffi(
    draft: &SmkDistributionMetadataDraft,
) -> Result<KitDistributionMetadataDraft, (SmkStatus, String)> {
    Ok(KitDistributionMetadataDraft {
        script_id: required_str_from_slice(draft.script_id, "Script-ID")?.to_string(),
        version: optional_version_from_slice(draft.version, "Version")?,
        latest_url: optional_url_from_slice(draft.latest_url, "Latest-URL")?,
        latest_page_url: optional_url_from_slice(draft.latest_page_url, "Latest-Page-URL")?,
    })
}

fn distribution_metadata_drafts_from_raw(
    ptr: *const SmkDistributionMetadataDraft,
    len: usize,
) -> Result<Vec<KitDistributionMetadataDraft>, (SmkStatus, String)> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if ptr.is_null() {
        return Err((
            SmkStatus::NullArgument,
            "distribution metadata records are null".to_string(),
        ));
    }
    // SAFETY: the caller promises `ptr` points to `len` readable records.
    let drafts = unsafe { slice::from_raw_parts(ptr, len) };
    drafts
        .iter()
        .map(distribution_metadata_draft_from_ffi)
        .collect()
}

fn script_meta_write_mode(value: u32) -> Result<ScriptMetaWriteMode, (SmkStatus, String)> {
    match value {
        0 => Ok(ScriptMetaWriteMode::InsertOrReplace),
        1 => Ok(ScriptMetaWriteMode::InsertOnly),
        2 => Ok(ScriptMetaWriteMode::ReplaceOnly),
        _ => Err((
            SmkStatus::InvalidArgument,
            format!("unknown script metadata write mode `{value}`"),
        )),
    }
}

fn script_meta_backup_reason(value: u32) -> Result<ScriptMetaBackupReason, (SmkStatus, String)> {
    match value {
        0 => Ok(ScriptMetaBackupReason::BeforeSave),
        1 => Ok(ScriptMetaBackupReason::BeforeRestore),
        2 => Ok(ScriptMetaBackupReason::ResetInitial),
        _ => Err((
            SmkStatus::InvalidArgument,
            format!("unknown script metadata backup reason `{value}`"),
        )),
    }
}

fn scan_mode_from_u32(value: u32) -> Result<ScanMode, (SmkStatus, String)> {
    match value {
        0 => Ok(ScanMode::FileListOnly),
        1 => Ok(ScanMode::MetadataOnly),
        2 => Ok(ScanMode::FileListAndMetadata),
        _ => Err((
            SmkStatus::InvalidArgument,
            format!("unknown scan mode `{value}`"),
        )),
    }
}

fn root_purpose_from_u32(value: u32) -> Result<RootPurpose, (SmkStatus, String)> {
    match value {
        0 => Ok(RootPurpose::FileList),
        1 => Ok(RootPurpose::MetadataCatalog),
        2 => Ok(RootPurpose::UpdateCheck),
        3 => Ok(RootPurpose::FileListAndMetadata),
        _ => Err((
            SmkStatus::InvalidArgument,
            format!("unknown root purpose `{value}`"),
        )),
    }
}

fn watch_policy_from_u32(value: u32) -> Result<WatchPolicy, (SmkStatus, String)> {
    match value {
        0 => Ok(WatchPolicy::Disabled),
        1 => Ok(WatchPolicy::VisibleOnly),
        2 => Ok(WatchPolicy::AllRegistered),
        3 => Ok(WatchPolicy::Manual),
        _ => Err((
            SmkStatus::InvalidArgument,
            format!("unknown watch policy `{value}`"),
        )),
    }
}

fn cache_policy_from_u32(value: u32) -> Result<CachePolicy, (SmkStatus, String)> {
    match value {
        0 => Ok(CachePolicy::Disabled),
        1 => Ok(CachePolicy::MemoryOnly),
        2 => Ok(CachePolicy::PersistentCatalogOnly),
        3 => Ok(CachePolicy::MemoryAndPersistent),
        _ => Err((
            SmkStatus::InvalidArgument,
            format!("unknown cache policy `{value}`"),
        )),
    }
}

fn cache_scope_from_u32(value: u32) -> Result<CacheScope, (SmkStatus, String)> {
    match value {
        0 => Ok(CacheScope::All),
        1 => Ok(CacheScope::Catalog),
        2 => Ok(CacheScope::FileList),
        3 => Ok(CacheScope::Root),
        _ => Err((
            SmkStatus::InvalidArgument,
            format!("unknown cache scope `{value}`"),
        )),
    }
}

fn refresh_policy_from_u32(value: u32) -> Result<RefreshPolicy, (SmkStatus, String)> {
    match value {
        0 => Ok(RefreshPolicy::ManualOnly),
        1 => Ok(RefreshPolicy::OnVisible),
        2 => Ok(RefreshPolicy::OnFileEvent),
        3 => Ok(RefreshPolicy::OnFileEventDeferred),
        4 => Ok(RefreshPolicy::Scheduled),
        _ => Err((
            SmkStatus::InvalidArgument,
            format!("unknown refresh policy `{value}`"),
        )),
    }
}

fn root_priority_from_u32(value: u32) -> Result<RootPriority, (SmkStatus, String)> {
    match value {
        0 => Ok(RootPriority::VisibleWhenSelected),
        1 => Ok(RootPriority::UserInitiated),
        2 => Ok(RootPriority::Background),
        _ => Err((
            SmkStatus::InvalidArgument,
            format!("unknown root priority `{value}`"),
        )),
    }
}

fn utf8_from_raw<'a>(ptr: *const u8, len: usize) -> Result<&'a str, (SmkStatus, String)> {
    if len == 0 {
        return Ok("");
    }
    if ptr.is_null() {
        return Err((SmkStatus::NullArgument, "input slice is null".to_string()));
    }
    // SAFETY: the caller promises that `ptr` points to `len` readable bytes for this call.
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    str::from_utf8(bytes).map_err(|error| (SmkStatus::InvalidUtf8, error.to_string()))
}

fn borrowed_slice(bytes: &[u8]) -> SmkUtf8Slice {
    if bytes.is_empty() {
        SmkUtf8Slice::empty()
    } else {
        SmkUtf8Slice {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        }
    }
}

fn borrowed_str_slice(value: Option<&str>) -> SmkUtf8Slice {
    value.map_or_else(SmkUtf8Slice::empty, |value| {
        borrowed_slice(value.as_bytes())
    })
}

fn emit_update_progress(
    callback: SmkUpdateProgressCallback,
    context: *mut c_void,
    progress: &UpdateCheckProgress,
) {
    let Some(callback) = callback else {
        return;
    };
    let phase = update_progress_phase(progress.phase);
    let ffi_progress = SmkUpdateProgress {
        completed_items: progress.completed_items,
        total_items: progress.total_items,
        item_id: borrowed_str_slice(progress.item_id.as_deref()),
        script_id: borrowed_str_slice(progress.script_id.as_deref()),
        phase: borrowed_slice(phase.as_bytes()),
        message: borrowed_slice(progress.message.as_bytes()),
    };
    callback(&ffi_progress, context);
}

fn set_engine_error(engine: *mut SmkEngine, message: &str) {
    if !engine.is_null() {
        // SAFETY: best-effort error recording for a caller-provided live handle.
        unsafe {
            (*engine).set_error(message);
        }
    }
}

fn bool_byte(value: bool) -> u8 {
    u8::from(value)
}

fn optional_u64(value: Option<u64>) -> (u8, u64) {
    value.map_or((0, 0), |value| (1, value))
}

fn optional_u32(value: Option<u32>) -> (u8, u32) {
    value.map_or((0, 0), |value| (1, value))
}

fn root_status(status: RootStatus) -> &'static str {
    match status {
        RootStatus::NotLoaded => "not_loaded",
        RootStatus::Ready => "ready",
        RootStatus::Dirty => "dirty",
        RootStatus::Loading => "loading",
        RootStatus::Missing => "missing",
        RootStatus::Unreadable => "unreadable",
        RootStatus::TimedOut => "timed_out",
        RootStatus::Overflowed => "overflowed",
        RootStatus::Cancelled => "cancelled",
    }
}

fn operation_status(status: scriptmetakit::OperationStatus) -> &'static str {
    status.as_str()
}

fn script_runtime_kind(kind: ScriptRuntimeKind) -> &'static str {
    match kind {
        ScriptRuntimeKind::AppleScript => "apple_script",
        ScriptRuntimeKind::JavaScriptForAutomation => "java_script_for_automation",
        ScriptRuntimeKind::AdobeJavaScript => "adobe_java_script",
        ScriptRuntimeKind::AdobeUxp => "adobe_uxp",
    }
}

fn optional_script_runtime_kind_from_slice(
    value: SmkUtf8Slice,
) -> Result<Option<ScriptRuntimeKind>, (SmkStatus, String)> {
    let Some(value) = optional_str_from_slice(value)? else {
        return Ok(None);
    };
    match value {
        "apple_script" => Ok(Some(ScriptRuntimeKind::AppleScript)),
        "java_script_for_automation" => Ok(Some(ScriptRuntimeKind::JavaScriptForAutomation)),
        "adobe_java_script" => Ok(Some(ScriptRuntimeKind::AdobeJavaScript)),
        "adobe_uxp" => Ok(Some(ScriptRuntimeKind::AdobeUxp)),
        _ => Err((
            SmkStatus::InvalidArgument,
            format!("unknown runtime_kind `{value}`"),
        )),
    }
}

fn script_meta_edit_state_from_slice(
    value: SmkUtf8Slice,
) -> Result<ScriptMetaEditState, (SmkStatus, String)> {
    let Some(value) = optional_str_from_slice(value)? else {
        return Ok(ScriptMetaEditState::Unknown);
    };
    match value {
        "unknown" => Ok(ScriptMetaEditState::Unknown),
        "unsupported" => Ok(ScriptMetaEditState::Unsupported),
        "obfuscated" => Ok(ScriptMetaEditState::Obfuscated),
        "read_only" => Ok(ScriptMetaEditState::ReadOnly),
        "appendable" => Ok(ScriptMetaEditState::Appendable),
        "editable" => Ok(ScriptMetaEditState::Editable),
        _ => Err((
            SmkStatus::InvalidArgument,
            format!("unknown scriptmeta_edit_state `{value}`"),
        )),
    }
}

fn smk_script_file_inspection(
    value: ScriptFileInspection,
    string_storage: &mut Vec<u8>,
    string_overflow: &mut Vec<Box<[u8]>>,
) -> SmkScriptFileInspection {
    SmkScriptFileInspection {
        is_supported_script_path: bool_byte(value.is_supported_script_path),
        runtime_kind: value.runtime_kind.map_or_else(SmkUtf8Slice::empty, |kind| {
            static_slice(script_runtime_kind(kind))
        }),
        shebang: value
            .shebang
            .as_deref()
            .map_or_else(SmkUtf8Slice::empty, |shebang| {
                push_stable_string(string_storage, string_overflow, shebang)
            }),
        comment_syntax: value
            .comment_syntax
            .map_or_else(SmkUtf8Slice::empty, |syntax| {
                static_slice(comment_syntax(syntax))
            }),
        supports_inline_scriptmeta_editing: bool_byte(value.supports_inline_scriptmeta_editing),
        is_file_locked: bool_byte(value.is_file_locked),
        is_read_only: bool_byte(value.is_read_only),
        can_edit_scriptmeta: bool_byte(value.can_edit_scriptmeta),
        can_append_scriptmeta: bool_byte(value.can_append_scriptmeta),
        scriptmeta_edit_state: static_slice(value.scriptmeta_edit_state.as_str()),
    }
}

fn comment_syntax(syntax: CommentSyntax) -> &'static str {
    match syntax {
        CommentSyntax::JavaScriptBlock => "javascript_block",
        CommentSyntax::AppleScriptBlock => "apple_script_block",
        CommentSyntax::PlainText => "plain_text",
        CommentSyntax::None => "",
    }
}

fn file_entry_change_kind(kind: FileEntryChangeKind) -> &'static str {
    match kind {
        FileEntryChangeKind::Added => "added",
        FileEntryChangeKind::Removed => "removed",
        FileEntryChangeKind::Modified => "modified",
    }
}

fn watch_path_event_kind(kind: WatchPathEventKind) -> &'static str {
    match kind {
        WatchPathEventKind::Added => "added",
        WatchPathEventKind::Modified => "modified",
        WatchPathEventKind::Removed => "removed",
        WatchPathEventKind::DirectoryChanged => "directory_changed",
        WatchPathEventKind::RootChanged => "root_changed",
        WatchPathEventKind::Overflow => "overflow",
    }
}

fn watch_ignore_reason(reason: WatchIgnoreReason) -> &'static str {
    match reason {
        WatchIgnoreReason::OutsideRoot => "outside_root",
        WatchIgnoreReason::HiddenPath => "hidden_path",
        WatchIgnoreReason::PackagePath => "package_path",
        WatchIgnoreReason::UnsupportedExtension => "unsupported_extension",
        WatchIgnoreReason::NotRelevant => "not_relevant",
    }
}

fn watch_rename_confidence(confidence: WatchRenameConfidence) -> &'static str {
    match confidence {
        WatchRenameConfidence::Possible => "possible",
    }
}

fn watch_rescan_reason(reason: WatchRescanReason) -> &'static str {
    match reason {
        WatchRescanReason::ChangedPath => "changed_path",
        WatchRescanReason::DirectoryChanged => "directory_changed",
        WatchRescanReason::RootChanged => "root_changed",
        WatchRescanReason::Overflow => "overflow",
        WatchRescanReason::TooManyDirtyDirectories => "too_many_dirty_directories",
    }
}

fn update_status(status: UpdateStatus) -> &'static str {
    match status {
        UpdateStatus::Idle => "idle",
        UpdateStatus::Checking => "checking",
        UpdateStatus::UpToDate => "up_to_date",
        UpdateStatus::UpdateAvailable => "update_available",
        UpdateStatus::Failed => "failed",
        UpdateStatus::NotCheckable => "not_checkable",
        UpdateStatus::Cancelled => "cancelled",
    }
}

fn update_progress_phase(phase: UpdateCheckProgressPhase) -> &'static str {
    match phase {
        UpdateCheckProgressPhase::Started => "started",
        UpdateCheckProgressPhase::Checking => "checking",
        UpdateCheckProgressPhase::Retrying => "retrying",
        UpdateCheckProgressPhase::FinishedItem => "finished_item",
        UpdateCheckProgressPhase::FailedItem => "failed_item",
        UpdateCheckProgressPhase::Cancelled => "cancelled",
        UpdateCheckProgressPhase::Finished => "finished",
    }
}

fn script_meta_write_operation(operation: ScriptMetaWriteOperation) -> &'static str {
    match operation {
        ScriptMetaWriteOperation::Inserted => "inserted",
        ScriptMetaWriteOperation::Replaced => "replaced",
    }
}

fn script_meta_comment_style(style: scriptmetakit::ScriptMetaCommentStyle) -> &'static str {
    match style {
        scriptmetakit::ScriptMetaCommentStyle::JavaScriptBlock => "javascript_block",
        scriptmetakit::ScriptMetaCommentStyle::AppleScriptBlock => "apple_script_block",
        scriptmetakit::ScriptMetaCommentStyle::PlainText => "plain_text",
    }
}

fn script_meta_backup_reason_string(reason: ScriptMetaBackupReason) -> &'static str {
    match reason {
        ScriptMetaBackupReason::BeforeSave => "before_save",
        ScriptMetaBackupReason::BeforeRestore => "before_restore",
        ScriptMetaBackupReason::ResetInitial => "reset_initial",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice_text(slice: SmkUtf8Slice) -> String {
        if slice.len == 0 {
            return String::new();
        }
        assert!(!slice.ptr.is_null());
        // SAFETY: these tests only read slices returned by `push_stable_string`
        // while the backing storage is still alive in the same scope.
        let bytes = unsafe { slice::from_raw_parts(slice.ptr, slice.len) };
        str::from_utf8(bytes)
            .expect("slice should be valid UTF-8")
            .to_string()
    }

    #[test]
    fn push_stable_string_uses_arena_when_capacity_is_available() {
        let mut storage = Vec::with_capacity("alpha".len() + "beta".len());
        let mut overflow = Vec::new();

        let first = push_stable_string(&mut storage, &mut overflow, "alpha");
        let second = push_stable_string(&mut storage, &mut overflow, "beta");

        assert_eq!(slice_text(first), "alpha");
        assert_eq!(slice_text(second), "beta");
        assert!(overflow.is_empty());
    }

    #[test]
    fn push_stable_string_falls_back_without_reallocating_arena() {
        let mut storage = Vec::with_capacity(0);
        let mut overflow = Vec::new();

        let first = push_stable_string(&mut storage, &mut overflow, "alpha");
        let second = push_stable_string(&mut storage, &mut overflow, "beta");

        assert_eq!(storage.capacity(), 0);
        assert_eq!(overflow.len(), 2);
        assert_eq!(slice_text(first), "alpha");
        assert_eq!(slice_text(second), "beta");
    }

    #[test]
    fn script_file_inspection_keeps_dynamic_shebang_storage() {
        let result = SmkScriptFileInspectionResult::new(ScriptFileInspection {
            is_supported_script_path: true,
            runtime_kind: Some(ScriptRuntimeKind::JavaScriptForAutomation),
            shebang: Some("#!/usr/bin/env osascript -l JavaScript".to_string()),
            comment_syntax: Some(CommentSyntax::JavaScriptBlock),
            supports_inline_scriptmeta_editing: true,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: true,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: ScriptMetaEditState::Editable,
        });

        assert_eq!(
            slice_text(result.inspection.shebang),
            "#!/usr/bin/env osascript -l JavaScript"
        );
        assert_eq!(
            slice_text(result.inspection.runtime_kind),
            "java_script_for_automation"
        );
    }

    #[test]
    fn adobe_runtime_uses_core_serde_name() {
        assert_eq!(
            script_runtime_kind(ScriptRuntimeKind::AdobeJavaScript),
            "adobe_java_script"
        );
    }

    #[test]
    fn script_id_uniqueness_result_exposes_duplicates_and_ignores_empty_ids() {
        let report = scriptmetakit::validate_script_id_uniqueness(&[
            uniqueness_item("/tmp/a.jsx", "com.example.same"),
            uniqueness_item("/tmp/b.jsx", "com.example.same"),
            uniqueness_item("/tmp/c.jsx", ""),
            uniqueness_item("/tmp/d.jsx", ""),
        ]);
        let result = SmkScriptIdUniquenessResult::from_report(&report);

        assert_eq!(result.report.total_items, 4);
        assert_eq!(result.report.unique_script_ids, 1);
        assert_eq!(result.report.duplicate_count, 1);
        assert_eq!(result.duplicates.len(), 1);
        assert_eq!(
            slice_text(result.duplicates[0].script_id),
            "com.example.same"
        );
        assert_eq!(result.duplicates[0].item_id_count, 2);
        assert_eq!(result.duplicates[0].file_path_count, 2);

        let item_ids = result
            .item_ids
            .iter()
            .map(|slice| slice_text(*slice))
            .collect::<Vec<_>>();
        assert_eq!(item_ids, ["/tmp/a.jsx", "/tmp/b.jsx"]);
    }

    #[test]
    fn script_id_uniqueness_ffi_uses_minimal_items() {
        let items = [
            ffi_uniqueness_item("/tmp/a.jsx", "com.example.same"),
            ffi_uniqueness_item("/tmp/b.jsx", "com.example.same"),
            ffi_uniqueness_item("/tmp/c.jsx", ""),
        ];
        let mut result = ptr::null_mut();

        let status =
            unsafe { smk_validate_script_id_uniqueness(items.as_ptr(), items.len(), &mut result) };

        assert_eq!(status, SmkStatus::Ok);
        assert!(!result.is_null());
        let result_ref = unsafe { &*result };
        assert_eq!(result_ref.report.total_items, 3);
        assert_eq!(result_ref.report.unique_script_ids, 1);
        assert_eq!(result_ref.duplicates.len(), 1);
        assert_eq!(
            slice_text(result_ref.duplicates[0].script_id),
            "com.example.same"
        );

        unsafe {
            smk_script_id_uniqueness_result_free(result);
        }
    }

    fn uniqueness_item(file_path: &str, script_id: &str) -> scriptmetakit::ScriptIdUniquenessItem {
        scriptmetakit::ScriptIdUniquenessItem {
            item_id: file_path.to_string(),
            file_path: PathBuf::from(file_path),
            script_id: script_id.to_string(),
        }
    }

    fn ffi_uniqueness_item(
        file_path: &'static str,
        script_id: &'static str,
    ) -> SmkScriptIdUniquenessItem {
        SmkScriptIdUniquenessItem {
            item_id: input_slice(file_path),
            file_path: input_slice(file_path),
            script_id: input_slice(script_id),
        }
    }

    fn input_slice(value: &'static str) -> SmkUtf8Slice {
        SmkUtf8Slice {
            ptr: value.as_ptr(),
            len: value.len(),
        }
    }
}
