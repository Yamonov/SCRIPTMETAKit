#ifndef SCRIPTMETAKIT_FFI_H
#define SCRIPTMETAKIT_FFI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum SmkStatus {
    SMK_STATUS_OK = 0,
    SMK_STATUS_NULL_ARGUMENT = 1,
    SMK_STATUS_INVALID_UTF8 = 2,
    SMK_STATUS_INVALID_ARGUMENT = 3,
    SMK_STATUS_ENGINE_ERROR = 4,
    SMK_STATUS_PANIC = 5,
    SMK_STATUS_CONFLICT = 6,
} SmkStatus;

typedef struct SmkEngine SmkEngine;
typedef struct SmkScanResult SmkScanResult;
typedef struct SmkEditResult SmkEditResult;
typedef struct SmkScriptIdUniquenessResult SmkScriptIdUniquenessResult;

typedef struct SmkUtf8Slice {
    const uint8_t *ptr;
    size_t len;
} SmkUtf8Slice;

typedef struct SmkRootSnapshot {
    SmkUtf8Slice root_id;
    SmkUtf8Slice path;
    SmkUtf8Slice status;
    uint8_t is_dirty;
    uint8_t has_last_loaded_at;
    uint64_t last_loaded_at;
    uint8_t has_last_event_at;
    uint64_t last_event_at;
    size_t item_count;
    SmkUtf8Slice error_code;
    SmkUtf8Slice error_message;
} SmkRootSnapshot;

typedef struct SmkOperationalPolicy {
    size_t max_concurrent_meta_url_checks;
    size_t retry_attempts;
    uint64_t retry_initial_delay_millis;
    uint32_t retry_backoff_multiplier;
    uint64_t max_retry_delay_millis;
    uint64_t request_timeout_millis;
    uint64_t resource_timeout_millis;
    uint64_t watcher_debounce_delay_millis;
    uint64_t watcher_max_delivery_delay_millis;
    size_t watcher_max_pending_paths;
} SmkOperationalPolicy;

typedef struct SmkRootRegistration {
    SmkUtf8Slice root_id;
    SmkUtf8Slice path;
    SmkUtf8Slice display_name;
    uint32_t purpose;
    uint32_t watch_policy;
    uint32_t cache_policy;
    uint32_t refresh_policy;
    uint32_t priority;
} SmkRootRegistration;

typedef struct SmkRegisteredRootSignature {
    SmkUtf8Slice root_id;
    SmkUtf8Slice path;
} SmkRegisteredRootSignature;

typedef struct SmkCatalogInfo {
    uint8_t has_catalog;
    SmkUtf8Slice source_revision;
    uint32_t candidate_cache_schema_version;
    uint64_t candidate_cache_built_at;
} SmkCatalogInfo;

typedef struct SmkFileIdentity {
    SmkUtf8Slice stable_id;
    SmkUtf8Slice volume_id;
    SmkUtf8Slice file_id;
    uint8_t has_file_size;
    uint64_t file_size;
    uint8_t has_content_modified_at;
    uint64_t content_modified_at;
} SmkFileIdentity;

typedef struct SmkScriptFileInspection {
    uint8_t is_supported_script_path;
    SmkUtf8Slice runtime_kind;
    SmkUtf8Slice shebang;
    SmkUtf8Slice comment_syntax;
    uint8_t supports_inline_scriptmeta_editing;
    uint8_t is_file_locked;
    uint8_t is_read_only;
    uint8_t can_edit_scriptmeta;
    uint8_t can_append_scriptmeta;
    SmkUtf8Slice scriptmeta_edit_state;
} SmkScriptFileInspection;

typedef struct SmkPathResolution {
    SmkUtf8Slice display_path;
    SmkUtf8Slice source_path;
    SmkUtf8Slice resolved_path;
    SmkUtf8Slice path_kind;
    SmkUtf8Slice resolution_status;
    SmkUtf8Slice resolution_message;
} SmkPathResolution;

typedef struct SmkScriptItem {
    SmkUtf8Slice root_id;
    SmkUtf8Slice file_path;
    SmkUtf8Slice identity_path;
    SmkUtf8Slice runtime_kind;
    SmkUtf8Slice shebang;
    SmkUtf8Slice script_id;
    SmkUtf8Slice version;
    SmkUtf8Slice name;
    SmkUtf8Slice description;
    SmkUtf8Slice target_app;
    SmkUtf8Slice min_target_version;
    SmkUtf8Slice meta_url;
    SmkUtf8Slice author;
    SmkUtf8Slice release_date;
    SmkUtf8Slice edit_password_sha256;
    uint8_t has_scriptmeta;
    uint8_t has_scriptmeta_edit_password;
    uint8_t is_file_locked;
    uint8_t is_read_only;
    uint8_t can_edit_scriptmeta;
    uint8_t can_append_scriptmeta;
    SmkUtf8Slice scriptmeta_edit_state;
} SmkScriptItem;

typedef struct SmkScriptIdUniquenessItem {
    SmkUtf8Slice item_id;
    SmkUtf8Slice file_path;
    SmkUtf8Slice script_id;
} SmkScriptIdUniquenessItem;

typedef struct SmkFileEntry {
    SmkUtf8Slice display_path;
    SmkUtf8Slice resolved_path;
    SmkUtf8Slice path_kind;
    SmkUtf8Slice resolution_status;
    SmkUtf8Slice resolution_message;
    uint8_t is_directory;
    uint8_t has_file_size;
    uint64_t file_size;
    uint8_t has_content_modified_at;
    uint64_t content_modified_at;
    uint8_t has_identity;
    SmkFileIdentity identity;
    SmkUtf8Slice runtime_kind;
    SmkUtf8Slice shebang;
    uint8_t has_scriptmeta;
    uint8_t has_scriptmeta_edit_password;
    uint8_t is_file_locked;
    uint8_t is_read_only;
    uint8_t can_edit_scriptmeta;
    uint8_t can_append_scriptmeta;
    SmkUtf8Slice scriptmeta_edit_state;
    uint8_t has_scriptmeta_item;
    SmkScriptItem scriptmeta_item;
    size_t first_child_index;
    size_t child_count;
} SmkFileEntry;

typedef struct SmkFileListSnapshot {
    size_t root_index;
    size_t first_child_index;
    size_t child_count;
    uint8_t truncated;
} SmkFileListSnapshot;

typedef struct SmkFileListDirectoryStateRange {
    size_t first_directory_state_index;
    size_t directory_state_count;
} SmkFileListDirectoryStateRange;

typedef struct SmkDirectoryStateEntry {
    SmkUtf8Slice path;
    uint8_t has_modification_time_millis;
    uint64_t modification_time_millis;
    size_t child_count;
    uint64_t child_fingerprint;
    uint8_t has_identity;
    SmkFileIdentity identity;
} SmkDirectoryStateEntry;

typedef struct SmkCandidateRecord {
    SmkUtf8Slice root_id;
    SmkUtf8Slice root_path;
    SmkUtf8Slice file_path;
    SmkUtf8Slice identity_path;
    SmkUtf8Slice path_kind;
    SmkUtf8Slice resolution_status;
    SmkUtf8Slice resolution_message;
    SmkUtf8Slice runtime_kind;
    SmkUtf8Slice shebang;
    uint8_t has_scriptmeta;
    uint8_t has_scriptmeta_edit_password;
    uint8_t is_file_locked;
    uint8_t is_read_only;
    uint8_t can_edit_scriptmeta;
    uint8_t can_append_scriptmeta;
    SmkUtf8Slice scriptmeta_edit_state;
    uint8_t has_file_size;
    uint64_t file_size;
    uint8_t has_content_modified_at;
    uint64_t content_modified_at;
    uint8_t has_item;
    SmkScriptItem item;
} SmkCandidateRecord;

typedef struct SmkUpdateCheckInfo {
    uint8_t has_update_check;
    uint64_t checked_at;
} SmkUpdateCheckInfo;

typedef struct SmkUpdateStatusEntry {
    SmkUtf8Slice item_id;
    SmkUtf8Slice status;
} SmkUpdateStatusEntry;

typedef struct SmkDistributionResolutionEntry {
    SmkUtf8Slice item_id;
    SmkUtf8Slice latest_version;
    SmkUtf8Slice latest_page_url;
    SmkUtf8Slice final_page_url;
    size_t first_latest_url_history_index;
    size_t latest_url_history_count;
    uint64_t checked_at;
    uint8_t is_unresolved;
    SmkUtf8Slice note;
    uint8_t has_redirect_count;
    uint32_t redirect_count;
} SmkDistributionResolutionEntry;

typedef struct SmkUpdateFailureEntry {
    SmkUtf8Slice item_id;
    SmkUtf8Slice code;
    SmkUtf8Slice message;
    SmkUtf8Slice file_path;
    SmkUtf8Slice script_id;
    SmkUtf8Slice current_version;
    SmkUtf8Slice meta_url;
    SmkUtf8Slice source_url;
    uint64_t checked_at;
} SmkUpdateFailureEntry;

typedef struct SmkUpdateErrorEntry {
    SmkUtf8Slice item_id;
    SmkUtf8Slice message;
} SmkUpdateErrorEntry;

typedef struct SmkUpdateProgress {
    size_t completed_items;
    size_t total_items;
    SmkUtf8Slice item_id;
    SmkUtf8Slice script_id;
    SmkUtf8Slice phase;
    SmkUtf8Slice message;
} SmkUpdateProgress;

typedef void (*SmkUpdateProgressCallback)(
    const SmkUpdateProgress *progress,
    void *context
);
typedef void (*SmkWatchNotificationCallback)(void *context);

typedef struct SmkScanChangeInfo {
    uint8_t has_change_summary;
    size_t added_count;
    size_t removed_count;
    size_t modified_count;
} SmkScanChangeInfo;

typedef struct SmkOperationInfo {
    SmkUtf8Slice status;
    size_t total_units;
    size_t completed_units;
    size_t failed_units;
    uint8_t cancelled;
    uint8_t timed_out;
    SmkUtf8Slice reason_code;
    SmkUtf8Slice message;
} SmkOperationInfo;

typedef struct SmkFileIssue {
    uint8_t has_root_id;
    SmkUtf8Slice root_id;
    SmkUtf8Slice path;
    SmkUtf8Slice code;
    SmkUtf8Slice message;
    SmkUtf8Slice path_kind;
    SmkUtf8Slice resolution_status;
    uint8_t is_directory;
} SmkFileIssue;

typedef struct SmkFileEntryChange {
    SmkUtf8Slice root_id;
    SmkUtf8Slice kind;
    SmkUtf8Slice display_path;
    SmkUtf8Slice resolved_path;
    SmkUtf8Slice path_kind;
    SmkUtf8Slice resolution_status;
    SmkUtf8Slice resolution_message;
    uint8_t is_directory;
    uint8_t has_file_size;
    uint64_t file_size;
    uint8_t has_content_modified_at;
    uint64_t content_modified_at;
    uint8_t has_identity;
    SmkFileIdentity identity;
    SmkUtf8Slice runtime_kind;
    SmkUtf8Slice shebang;
    uint8_t has_scriptmeta;
    uint8_t has_scriptmeta_edit_password;
    uint8_t is_file_locked;
    uint8_t is_read_only;
    uint8_t can_edit_scriptmeta;
    uint8_t can_append_scriptmeta;
    SmkUtf8Slice scriptmeta_edit_state;
} SmkFileEntryChange;

typedef struct SmkRootSnapshotSlice {
    const SmkRootSnapshot *ptr;
    size_t len;
} SmkRootSnapshotSlice;

typedef struct SmkRegisteredRootSignatureSlice {
    const SmkRegisteredRootSignature *ptr;
    size_t len;
} SmkRegisteredRootSignatureSlice;

typedef struct SmkFileListSnapshotSlice {
    const SmkFileListSnapshot *ptr;
    size_t len;
} SmkFileListSnapshotSlice;

typedef struct SmkFileListDirectoryStateRangeSlice {
    const SmkFileListDirectoryStateRange *ptr;
    size_t len;
} SmkFileListDirectoryStateRangeSlice;

typedef struct SmkDirectoryStateEntrySlice {
    const SmkDirectoryStateEntry *ptr;
    size_t len;
} SmkDirectoryStateEntrySlice;

typedef struct SmkFileEntrySlice {
    const SmkFileEntry *ptr;
    size_t len;
} SmkFileEntrySlice;

typedef struct SmkScriptItemSlice {
    const SmkScriptItem *ptr;
    size_t len;
} SmkScriptItemSlice;

typedef struct SmkScriptIdUniquenessReport {
    size_t total_items;
    size_t unique_script_ids;
    size_t duplicate_count;
} SmkScriptIdUniquenessReport;

typedef struct SmkScriptIdDuplicate {
    SmkUtf8Slice script_id;
    size_t first_item_id_index;
    size_t item_id_count;
    size_t first_file_path_index;
    size_t file_path_count;
} SmkScriptIdDuplicate;

typedef struct SmkScriptIdDuplicateSlice {
    const SmkScriptIdDuplicate *ptr;
    size_t len;
} SmkScriptIdDuplicateSlice;

typedef struct SmkCandidateRecordSlice {
    const SmkCandidateRecord *ptr;
    size_t len;
} SmkCandidateRecordSlice;

typedef struct SmkUpdateStatusEntrySlice {
    const SmkUpdateStatusEntry *ptr;
    size_t len;
} SmkUpdateStatusEntrySlice;

typedef struct SmkDistributionResolutionEntrySlice {
    const SmkDistributionResolutionEntry *ptr;
    size_t len;
} SmkDistributionResolutionEntrySlice;

typedef struct SmkUpdateFailureEntrySlice {
    const SmkUpdateFailureEntry *ptr;
    size_t len;
} SmkUpdateFailureEntrySlice;

typedef struct SmkUpdateErrorEntrySlice {
    const SmkUpdateErrorEntry *ptr;
    size_t len;
} SmkUpdateErrorEntrySlice;

typedef struct SmkUtf8SliceSlice {
    const SmkUtf8Slice *ptr;
    size_t len;
} SmkUtf8SliceSlice;

typedef struct SmkFileEntryChangeSlice {
    const SmkFileEntryChange *ptr;
    size_t len;
} SmkFileEntryChangeSlice;

typedef struct SmkFileIssueSlice {
    const SmkFileIssue *ptr;
    size_t len;
} SmkFileIssueSlice;

typedef struct SmkWatchChangeInfo {
    uint8_t has_watch_change;
    uint8_t overflowed;
    size_t path_count;
    size_t affected_root_count;
    size_t event_count;
    size_t ignored_path_count;
    size_t rename_candidate_count;
    size_t rescan_target_count;
} SmkWatchChangeInfo;

typedef struct SmkWatchPathEvent {
    SmkUtf8Slice root_id;
    SmkUtf8Slice path;
    SmkUtf8Slice kind;
    uint8_t is_directory;
    SmkUtf8Slice rescan_directory;
} SmkWatchPathEvent;

typedef struct SmkIgnoredWatchPath {
    uint8_t has_root_id;
    SmkUtf8Slice root_id;
    SmkUtf8Slice path;
    SmkUtf8Slice reason;
} SmkIgnoredWatchPath;

typedef struct SmkWatchRenameCandidate {
    SmkUtf8Slice root_id;
    SmkUtf8Slice old_path;
    SmkUtf8Slice new_path;
    SmkUtf8Slice confidence;
} SmkWatchRenameCandidate;

typedef struct SmkWatchRescanTarget {
    SmkUtf8Slice root_id;
    SmkUtf8Slice path;
    SmkUtf8Slice reason;
} SmkWatchRescanTarget;

typedef struct SmkWatchPathEventSlice {
    const SmkWatchPathEvent *ptr;
    size_t len;
} SmkWatchPathEventSlice;

typedef struct SmkIgnoredWatchPathSlice {
    const SmkIgnoredWatchPath *ptr;
    size_t len;
} SmkIgnoredWatchPathSlice;

typedef struct SmkWatchRenameCandidateSlice {
    const SmkWatchRenameCandidate *ptr;
    size_t len;
} SmkWatchRenameCandidateSlice;

typedef struct SmkWatchRescanTargetSlice {
    const SmkWatchRescanTarget *ptr;
    size_t len;
} SmkWatchRescanTargetSlice;

typedef struct SmkScriptMetadataDraft {
    SmkUtf8Slice script_id;
    SmkUtf8Slice version;
    SmkUtf8Slice description;
    SmkUtf8Slice target_app;
    SmkUtf8Slice min_target_version;
    SmkUtf8Slice meta_url;
    SmkUtf8Slice name;
    SmkUtf8Slice author;
    SmkUtf8Slice release_date;
    SmkUtf8Slice edit_password_sha256;
} SmkScriptMetadataDraft;

typedef struct SmkScriptMetadataWriteRequest {
    SmkUtf8Slice file_path;
    SmkUtf8Slice backup_root_path;
    uint32_t write_mode;
    SmkScriptMetadataDraft draft;
} SmkScriptMetadataWriteRequest;

typedef struct SmkDistributionMetadataDraft {
    SmkUtf8Slice script_id;
    SmkUtf8Slice version;
    SmkUtf8Slice latest_url;
    SmkUtf8Slice latest_page_url;
} SmkDistributionMetadataDraft;

typedef struct SmkScriptMetaBackupRecord {
    SmkUtf8Slice id;
    uint64_t created_at_millis;
    SmkUtf8Slice backup_file_name;
    SmkUtf8Slice backup_file_path;
    uint64_t file_size;
    SmkUtf8Slice reason;
} SmkScriptMetaBackupRecord;

typedef struct SmkScriptMetadataFileWriteResult {
    SmkUtf8Slice file_path;
    SmkUtf8Slice operation;
    uint8_t has_backup;
    SmkScriptMetaBackupRecord backup;
} SmkScriptMetadataFileWriteResult;

typedef struct SmkScriptMetadataEditReadResult {
    SmkUtf8Slice file_path;
    SmkScriptMetadataDraft draft;
    SmkUtf8Slice comment_style;
    SmkUtf8Slice line_ending;
    uint8_t has_existing_block;
    SmkUtf8Slice existing_block_text;
    SmkUtf8Slice source_fingerprint;
} SmkScriptMetadataEditReadResult;

typedef struct SmkScriptMetadataEditPreviewResult {
    SmkUtf8Slice file_path;
    SmkUtf8Slice preview_text;
    size_t preview_byte_count;
    uint64_t file_size;
    uint8_t has_file_size;
    SmkUtf8Slice comment_style;
    SmkUtf8Slice line_ending;
    uint8_t has_scriptmeta_marker_in_preview;
    uint8_t is_truncated;
    uint8_t requires_full_read;
    SmkUtf8Slice file_state_fingerprint;
} SmkScriptMetadataEditPreviewResult;

typedef struct SmkScriptMetaBackupGeneration {
    SmkUtf8Slice id;
    size_t sequence_number;
    uint64_t created_at_millis;
    SmkUtf8Slice file_path;
    uint64_t file_size;
    SmkUtf8Slice reason;
    uint8_t is_current_file;
} SmkScriptMetaBackupGeneration;

typedef struct SmkScriptMetaBackupGenerationSlice {
    const SmkScriptMetaBackupGeneration *ptr;
    size_t len;
} SmkScriptMetaBackupGenerationSlice;

SmkStatus smk_engine_create_default(SmkEngine **out_engine);
void smk_engine_free(SmkEngine *engine);

SmkStatus smk_supported_script_extensions(SmkUtf8Slice *out_extensions);

// String slices inside out_inspection remain valid until the next
// smk_inspect_script_file_path call on the same thread. Copy them immediately
// if the caller needs to keep them longer or cross thread boundaries.
SmkStatus smk_inspect_script_file_path(
    SmkUtf8Slice path,
    SmkScriptFileInspection *out_inspection
);

// String slices inside out_resolution remain valid until the next
// smk_resolve_registered_path call on the same thread. Copy them immediately
// if the caller needs to keep them longer or cross thread boundaries.
SmkStatus smk_resolve_registered_path(
    SmkUtf8Slice path,
    uint8_t follow_symlinks,
    uint8_t resolve_macos_alias,
    SmkPathResolution *out_resolution
);

SmkStatus smk_script_path_may_affect_metadata(
    SmkUtf8Slice path,
    uint8_t *out_may_affect
);

SmkStatus smk_can_read_directory_contents(
    SmkUtf8Slice path,
    uint8_t *out_can_read
);

SmkStatus smk_normalize_version_string(
    SmkUtf8Slice value,
    uint8_t *out_has_version,
    SmkEditResult **out_result
);

SmkStatus smk_validate_version_string(
    SmkUtf8Slice value,
    uint8_t *out_is_valid
);

SmkStatus smk_compare_versions(
    SmkUtf8Slice lhs,
    SmkUtf8Slice rhs,
    int32_t *out_ordering
);

SmkStatus smk_validate_edit_password_sha256_format(
    SmkUtf8Slice value,
    uint8_t *out_is_valid
);

/*
 * Ownership and lifetime
 * ----------------------
 * - smk_engine_create_default transfers one SmkEngine to the caller. Release
 *   it exactly once with smk_engine_free after all operations have finished.
 * - Functions returning SmkScanResult, SmkEditResult, or
 *   SmkScriptIdUniquenessResult transfer one result object to the caller.
 *   Release it with the matching *_free function.
 * - Slices returned by a result accessor are borrowed from that result. They
 *   remain valid until the result is freed and must not be freed separately.
 * - Input slices and arrays are borrowed only for the duration of the call.
 * - smk_supported_script_extensions returns process-lifetime static storage.
 * - smk_inspect_script_file_path and smk_resolve_registered_path use
 *   thread-local storage as documented above their declarations.
 *
 * Thread safety and callbacks
 * ---------------------------
 * Engine state is synchronized internally. At most one ordinary operation on
 * an engine handle executes at a time; concurrent ordinary calls wait. A
 * caller may invoke smk_engine_cancel_current_operation concurrently, including
 * from an update progress callback. Do not call any other engine operation or
 * free the engine from a callback. Callback pointers and their nested slices
 * are borrowed only for the duration of that callback invocation; copy data
 * before retaining it or sending it to another thread.
 *
 * The C API is synchronous. Scan, update-check, cache, watcher start/stop, and
 * engine-free calls may perform file I/O, network I/O, or wait for internal
 * worker threads. Call them from a background worker thread rather than a UI or
 * high-QoS thread. Swift consumers should prefer ScriptMetaKitEngine or
 * ScriptMetaKitWorkspace, which dispatch blocking FFI work at utility priority.
 */

SmkStatus smk_engine_last_error(const SmkEngine *engine, SmkUtf8Slice *out_message);

SmkStatus smk_engine_set_resolve_macos_alias(SmkEngine *engine, uint8_t enabled);

SmkStatus smk_engine_set_decompile_compiled_osa_during_scan(SmkEngine *engine, uint8_t enabled);

SmkStatus smk_engine_set_native_event_latency_millis(SmkEngine *engine, uint64_t latency_millis);

SmkStatus smk_engine_set_operational_policy(
    SmkEngine *engine,
    const SmkOperationalPolicy *policy
);

SmkStatus smk_engine_set_root_preflight_options(
    SmkEngine *engine,
    uint8_t reject_trash_roots,
    uint8_t reject_restricted_roots,
    uint8_t reject_low_script_density_large_roots,
    size_t max_scanned_items,
    uint64_t max_duration_millis,
    size_t min_scanned_file_count_for_large_root,
    size_t min_script_ratio_denominator,
    size_t min_scanned_items_for_time_limit
);

SmkStatus smk_engine_cancel_current_operation(SmkEngine *engine);

/* Internal Swift Task hand-off support. Pair reserve with finish. */
SmkStatus smk_engine_reserve_next_operation(SmkEngine *engine);
SmkStatus smk_engine_finish_operation_reservation(SmkEngine *engine);
SmkStatus smk_engine_cancel_current_or_reserved_operation(SmkEngine *engine);

SmkStatus smk_engine_scan_folder(
    SmkEngine *engine,
    const uint8_t *path_ptr,
    size_t path_len,
    SmkScanResult **out_result
);

SmkStatus smk_engine_scan_folders(
    SmkEngine *engine,
    const SmkUtf8Slice *paths_ptr,
    size_t path_count,
    uint8_t check_updates,
    SmkScanResult **out_result
);

SmkStatus smk_engine_scan_folders_with_progress(
    SmkEngine *engine,
    const SmkUtf8Slice *paths_ptr,
    size_t path_count,
    uint8_t check_updates,
    SmkUpdateProgressCallback progress_callback,
    void *progress_context,
    SmkScanResult **out_result
);

SmkStatus smk_engine_set_roots(
    SmkEngine *engine,
    const SmkRootRegistration *roots_ptr,
    size_t root_count
);

SmkStatus smk_engine_replace_root_group(
    SmkEngine *engine,
    SmkUtf8Slice group_id,
    const SmkRootRegistration *roots_ptr,
    size_t root_count
);

SmkStatus smk_engine_insert_roots_into_group(
    SmkEngine *engine,
    SmkUtf8Slice group_id,
    const SmkRootRegistration *roots_ptr,
    size_t root_count
);

SmkStatus smk_engine_set_visible_root(
    SmkEngine *engine,
    SmkUtf8Slice root_id,
    uint8_t has_root_id
);

SmkStatus smk_engine_scan_registered_roots(
    SmkEngine *engine,
    uint32_t scan_mode,
    uint8_t check_updates,
    SmkScanResult **out_result
);

SmkStatus smk_engine_scan_roots(
    SmkEngine *engine,
    const SmkUtf8Slice *root_ids_ptr,
    size_t root_id_count,
    uint32_t scan_mode,
    uint8_t check_updates,
    SmkScanResult **out_result
);

SmkStatus smk_engine_cached_roots(
    SmkEngine *engine,
    const SmkUtf8Slice *root_ids_ptr,
    size_t root_id_count,
    uint32_t scan_mode,
    SmkScanResult **out_result
);

SmkStatus smk_engine_scan_registered_roots_with_progress(
    SmkEngine *engine,
    uint32_t scan_mode,
    uint8_t check_updates,
    SmkUpdateProgressCallback progress_callback,
    void *progress_context,
    SmkScanResult **out_result
);

SmkStatus smk_engine_scan_roots_with_progress(
    SmkEngine *engine,
    const SmkUtf8Slice *root_ids_ptr,
    size_t root_id_count,
    uint32_t scan_mode,
    uint8_t check_updates,
    SmkUpdateProgressCallback progress_callback,
    void *progress_context,
    SmkScanResult **out_result
);

SmkStatus smk_engine_check_update_item(
    SmkEngine *engine,
    const SmkScriptItem *item,
    SmkScanResult **out_result
);

SmkStatus smk_engine_check_update_item_with_progress(
    SmkEngine *engine,
    const SmkScriptItem *item,
    SmkUpdateProgressCallback progress_callback,
    void *progress_context,
    SmkScanResult **out_result
);

SmkStatus smk_engine_check_updates_for_items(
    SmkEngine *engine,
    const SmkScriptItem *items_ptr,
    size_t item_count,
    SmkScanResult **out_result
);

SmkStatus smk_engine_check_updates_for_items_with_progress(
    SmkEngine *engine,
    const SmkScriptItem *items_ptr,
    size_t item_count,
    SmkUpdateProgressCallback progress_callback,
    void *progress_context,
    SmkScanResult **out_result
);

SmkStatus smk_validate_script_id_uniqueness(
    const SmkScriptIdUniquenessItem *items_ptr,
    size_t item_count,
    SmkScriptIdUniquenessResult **out_result
);

SmkStatus smk_script_id_uniqueness_result_report(
    const SmkScriptIdUniquenessResult *result,
    SmkScriptIdUniquenessReport *out_report
);

SmkStatus smk_script_id_uniqueness_result_duplicates(
    const SmkScriptIdUniquenessResult *result,
    SmkScriptIdDuplicateSlice *out_duplicates
);

SmkStatus smk_script_id_uniqueness_result_item_ids(
    const SmkScriptIdUniquenessResult *result,
    SmkUtf8SliceSlice *out_item_ids
);

SmkStatus smk_script_id_uniqueness_result_file_paths(
    const SmkScriptIdUniquenessResult *result,
    SmkUtf8SliceSlice *out_file_paths
);

void smk_script_id_uniqueness_result_free(SmkScriptIdUniquenessResult *result);

SmkStatus smk_engine_load_cache_file(
    SmkEngine *engine,
    SmkUtf8Slice cache_path
);

SmkStatus smk_engine_save_cache_file(
    SmkEngine *engine,
    uint32_t scope,
    SmkUtf8Slice cache_path
);

SmkStatus smk_engine_start_watching(SmkEngine *engine);
SmkStatus smk_engine_start_watching_with_callback(
    SmkEngine *engine,
    SmkWatchNotificationCallback callback,
    void *context
);
SmkStatus smk_engine_stop_watching(SmkEngine *engine);

SmkStatus smk_engine_watcher_requires_restart(
    SmkEngine *engine,
    uint8_t *out_requires_restart
);

SmkStatus smk_engine_poll_watcher_scan(
    SmkEngine *engine,
    uint8_t *out_changed,
    SmkScanResult **out_result
);

/* Returns only root-scoped data affected by the watcher batch. Catalog and
 * update-check snapshots are omitted because they describe the full root set. */
SmkStatus smk_engine_poll_watcher_scan_dirty_only(
    SmkEngine *engine,
    uint8_t *out_changed,
    SmkScanResult **out_result
);

SmkStatus smk_scan_result_roots(
    const SmkScanResult *result,
    SmkRootSnapshotSlice *out_roots
);

SmkStatus smk_scan_result_catalog_info(
    const SmkScanResult *result,
    SmkCatalogInfo *out_info
);

SmkStatus smk_scan_result_registered_root_signatures(
    const SmkScanResult *result,
    SmkRegisteredRootSignatureSlice *out_roots
);

SmkStatus smk_scan_result_file_lists(
    const SmkScanResult *result,
    SmkFileListSnapshotSlice *out_file_lists
);

SmkStatus smk_scan_result_file_entries(
    const SmkScanResult *result,
    SmkFileEntrySlice *out_file_entries
);

SmkStatus smk_scan_result_file_list_directory_state_ranges(
    const SmkScanResult *result,
    SmkFileListDirectoryStateRangeSlice *out_ranges
);

SmkStatus smk_scan_result_directory_states(
    const SmkScanResult *result,
    SmkDirectoryStateEntrySlice *out_directory_states
);

SmkStatus smk_scan_result_items(
    const SmkScanResult *result,
    SmkScriptItemSlice *out_items
);

SmkStatus smk_scan_result_file_items(
    const SmkScanResult *result,
    SmkScriptItemSlice *out_items
);

SmkStatus smk_scan_result_candidate_records(
    const SmkScanResult *result,
    SmkCandidateRecordSlice *out_records
);

SmkStatus smk_scan_result_update_info(
    const SmkScanResult *result,
    SmkUpdateCheckInfo *out_info
);

SmkStatus smk_scan_result_update_statuses(
    const SmkScanResult *result,
    SmkUpdateStatusEntrySlice *out_statuses
);

SmkStatus smk_scan_result_update_resolutions(
    const SmkScanResult *result,
    SmkDistributionResolutionEntrySlice *out_resolutions
);

SmkStatus smk_scan_result_update_failures(
    const SmkScanResult *result,
    SmkUpdateFailureEntrySlice *out_failures
);

SmkStatus smk_scan_result_update_errors(
    const SmkScanResult *result,
    SmkUpdateErrorEntrySlice *out_errors
);

SmkStatus smk_scan_result_latest_url_history_urls(
    const SmkScanResult *result,
    SmkUtf8SliceSlice *out_urls
);

SmkStatus smk_scan_result_change_info(
    const SmkScanResult *result,
    SmkScanChangeInfo *out_info
);

SmkStatus smk_scan_result_file_entry_changes(
    const SmkScanResult *result,
    SmkFileEntryChangeSlice *out_changes
);

SmkStatus smk_scan_result_operation_info(
    const SmkScanResult *result,
    SmkOperationInfo *out_info
);

SmkStatus smk_scan_result_file_issues(
    const SmkScanResult *result,
    SmkFileIssueSlice *out_issues
);

SmkStatus smk_scan_result_watch_change_info(
    const SmkScanResult *result,
    SmkWatchChangeInfo *out_info
);

SmkStatus smk_scan_result_watch_events(
    const SmkScanResult *result,
    SmkWatchPathEventSlice *out_events
);

SmkStatus smk_scan_result_ignored_watch_paths(
    const SmkScanResult *result,
    SmkIgnoredWatchPathSlice *out_paths
);

SmkStatus smk_scan_result_watch_rename_candidates(
    const SmkScanResult *result,
    SmkWatchRenameCandidateSlice *out_candidates
);

SmkStatus smk_scan_result_watch_rescan_targets(
    const SmkScanResult *result,
    SmkWatchRescanTargetSlice *out_targets
);

void smk_scan_result_free(SmkScanResult *result);

SmkStatus smk_engine_write_script_metadata_file(
    SmkEngine *engine,
    const SmkScriptMetadataWriteRequest *request,
    SmkEditResult **out_result
);

SmkStatus smk_engine_write_script_metadata_file_if_unchanged(
    SmkEngine *engine,
    const SmkScriptMetadataWriteRequest *request,
    SmkUtf8Slice expected_source_fingerprint,
    SmkEditResult **out_result
);

SmkStatus smk_engine_read_script_metadata_draft_file(
    SmkEngine *engine,
    SmkUtf8Slice file_path,
    SmkEditResult **out_result
);

SmkStatus smk_engine_read_script_metadata_edit_preview_file(
    SmkEngine *engine,
    SmkUtf8Slice file_path,
    size_t max_bytes,
    SmkEditResult **out_result
);

SmkStatus smk_engine_render_distribution_metadata(
    SmkEngine *engine,
    const SmkDistributionMetadataDraft *records_ptr,
    size_t record_count,
    SmkEditResult **out_result
);

SmkStatus smk_engine_generate_edit_password_sha256(
    SmkEngine *engine,
    SmkUtf8Slice password,
    SmkEditResult **out_result
);

SmkStatus smk_engine_verify_edit_password_sha256(
    SmkEngine *engine,
    SmkUtf8Slice password,
    SmkUtf8Slice stored_value,
    uint8_t *out_is_match
);

SmkStatus smk_engine_scriptmeta_backup_generations(
    SmkEngine *engine,
    SmkUtf8Slice file_path,
    SmkUtf8Slice backup_root_path,
    SmkEditResult **out_result
);

SmkStatus smk_engine_create_scriptmeta_backup(
    SmkEngine *engine,
    SmkUtf8Slice file_path,
    SmkUtf8Slice backup_root_path,
    uint32_t reason,
    SmkEditResult **out_result
);

SmkStatus smk_engine_restore_scriptmeta_backup(
    SmkEngine *engine,
    SmkUtf8Slice file_path,
    SmkUtf8Slice backup_root_path,
    SmkUtf8Slice generation_id,
    SmkEditResult **out_result
);

SmkStatus smk_engine_clear_scriptmeta_backups(
    SmkEngine *engine,
    SmkUtf8Slice file_path,
    SmkUtf8Slice backup_root_path
);

SmkStatus smk_engine_reset_scriptmeta_backups_with_current_as_initial(
    SmkEngine *engine,
    SmkUtf8Slice file_path,
    SmkUtf8Slice backup_root_path,
    SmkEditResult **out_result
);

SmkStatus smk_edit_result_text(
    const SmkEditResult *result,
    SmkUtf8Slice *out_text
);

SmkStatus smk_edit_result_file_write_result(
    const SmkEditResult *result,
    SmkScriptMetadataFileWriteResult *out_info
);

SmkStatus smk_edit_result_metadata_edit_read_result(
    const SmkEditResult *result,
    SmkScriptMetadataEditReadResult *out_info
);

SmkStatus smk_edit_result_metadata_edit_preview_result(
    const SmkEditResult *result,
    SmkScriptMetadataEditPreviewResult *out_info
);

SmkStatus smk_edit_result_existing_lines(
    const SmkEditResult *result,
    SmkUtf8SliceSlice *out_lines
);

SmkStatus smk_edit_result_unknown_lines(
    const SmkEditResult *result,
    SmkUtf8SliceSlice *out_lines
);

SmkStatus smk_edit_result_backup_record(
    const SmkEditResult *result,
    uint8_t *out_has_record,
    SmkScriptMetaBackupRecord *out_record
);

SmkStatus smk_edit_result_backup_generations(
    const SmkEditResult *result,
    SmkScriptMetaBackupGenerationSlice *out_generations
);

void smk_edit_result_free(SmkEditResult *result);

#ifdef __cplusplus
}
#endif

#endif
