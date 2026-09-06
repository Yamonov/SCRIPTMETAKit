#![doc = "Core scanning, metadata parsing, update checking, and watch planning APIs for SCRIPTMETA."]
#![deny(unsafe_code)]

pub mod catalog;
pub mod core;
pub mod editkit;
pub mod engine;
pub mod formats;
pub mod resolver;
pub mod scanner;
pub mod storage;
pub mod watcher;

pub use catalog::{
    CacheInvalidationReason, CacheOptions, CachePolicy, CacheScope, DirectoryState,
    DirectoryStateMap, FileEntryChange, FileEntryChangeKind, FileIdentity, FileListSnapshot,
    ProgressUpdate, RefreshPolicy, RefreshRequest, RootError, RootPriority, RootPurpose,
    RootRegistration, RootSnapshot, RootStatus, ScanChangeSummary, ScanMode, ScanRequest,
    ScanResult, ScriptMetaCatalogSnapshot, ScriptMetaKitConfig, ScriptMetaKitEvent,
    SnapshotRevision, UpdateCheckOptions, UpdateCheckProgress, UpdateCheckProgressPhase,
    UpdateCheckRequest, UpdateCheckResult, UpdateFailure, UpdateStatus, WatcherOptions,
    path_based_root_id,
};
pub use core::{
    DistributionMetadata, DistributionResolution, FileIssue, OperationCancellation,
    OperationCancellationReservation, OperationStatus, OperationSummary, ParserOptions,
    ScriptMetaEditCapability, ScriptMetaEditState, ScriptMetaItem, ScriptMetaItemRef,
    ScriptMetaKitError, ScriptMetaKitResult, ScriptMetadata, ScriptRuntimeKind, VersionOrdering,
    compare_versions, decode_script_text, decode_script_text_strict, normalize_metadata_url,
    normalize_version_string, parse_distribution_metadata, parse_distribution_metadata_for_script,
    parse_distribution_metadata_records, parse_script_metadata, parse_script_metadata_with_options,
};
pub use editkit::{
    DEFAULT_SCRIPT_METADATA_PREVIEW_BYTES, DistributionMetadataDraft,
    MAX_SCRIPT_METADATA_PREVIEW_BYTES, ScriptIdDuplicate, ScriptIdUniquenessItem,
    ScriptIdUniquenessReport, ScriptMetaBackupGeneration, ScriptMetaBackupOptions,
    ScriptMetaBackupReason, ScriptMetaBackupRecord, ScriptMetaCommentStyle, ScriptMetaWriteMode,
    ScriptMetaWriteOperation, ScriptMetadataDraft, ScriptMetadataEditPreviewResult,
    ScriptMetadataEditReadResult, ScriptMetadataFileWriteResult, ScriptMetadataTextWriteResult,
    append_script_metadata_to_file, append_script_metadata_to_text, clear_scriptmeta_backups,
    create_scriptmeta_backup, generate_edit_password_sha256, is_valid_edit_password_sha256,
    read_script_metadata_draft_from_file, read_script_metadata_draft_from_text,
    read_script_metadata_edit_preview_from_file, render_distribution_metadata_block,
    render_script_metadata_block, render_script_metadata_for_style,
    reset_scriptmeta_backups_with_current_as_initial, restore_scriptmeta_backup,
    scriptmeta_backup_generations, validate_script_id_uniqueness, verify_edit_password_sha256,
    write_script_metadata_to_file, write_script_metadata_to_file_if_unchanged,
    write_script_metadata_to_text,
};
pub use engine::ScriptMetaKitEngine;
pub use formats::{
    CommentSyntax, ScriptFileInfo, ScriptFileInspection, ScriptFormatKind, ScriptFormatSpec,
    all_format_specs, comment_syntax_for_path, detect_script_file, format_for_path,
    format_kind_for_path, inspect_script_file, script_path_may_affect_metadata,
    supported_script_extensions, supported_script_extensions_text,
};
pub use resolver::{DistributionResolver, DistributionResolverOptions, UpdateResolver};
pub use scanner::{
    ExtensionPolicy, FileSystemEntry, PathKind, PathResolutionStatus, RootPreflightOptions,
    ScannablePathResolution, ScannerOptions, can_read_directory_contents, resolve_registered_path,
};
pub use storage::{
    CachePayload, CacheSchema, MAX_CACHE_FILE_BYTES, cache_payload_content_fingerprint,
    load_cache_payload, load_cache_payload_with_limit, save_cache_payload,
    save_cache_payload_with_limit,
};
#[cfg(feature = "native-watch")]
pub use watcher::NativeWatcher;
pub use watcher::{
    IgnoredWatchPath, LogicalWatchRoot, MonitorRootStrategy, OverflowPolicy, PhysicalWatchRoot,
    RawChangeBatch, RootChange, RootChangeBatch, WatchIgnoreReason, WatchPathEvent,
    WatchPathEventKind, WatchPlan, WatchPolicy, WatchRenameCandidate, WatchRenameConfidence,
    WatchRescanReason, WatchRescanTarget,
};

pub type RootId = std::sync::Arc<str>;
pub type ItemId = String;
pub type TimestampMillis = u64;

/// Returns the Rust package name.
#[must_use]
pub fn package_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// Returns the Rust package version.
#[must_use]
pub fn package_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[must_use]
pub fn now_timestamp_millis() -> TimestampMillis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{package_name, package_version};

    #[test]
    fn exposes_package_metadata() {
        assert_eq!(package_name(), "scriptmetakit");
        assert_eq!(package_version(), "1.3.1");
    }
}
