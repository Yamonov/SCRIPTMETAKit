use std::{ffi::c_void, ptr, slice};
#[cfg(feature = "native-watch")]
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};

use scriptmetakit_ffi::{
    SmkDistributionMetadataDraft, SmkDistributionResolutionEntrySlice, SmkEditResult, SmkEngine,
    SmkFileEntryChangeSlice, SmkFileEntrySlice, SmkFileIssueSlice, SmkFileListSnapshotSlice,
    SmkOperationInfo, SmkRootRegistration, SmkRootSnapshotSlice, SmkScanChangeInfo, SmkScanResult,
    SmkScriptItemSlice, SmkScriptMetaBackupGenerationSlice, SmkScriptMetaBackupRecord,
    SmkScriptMetadataDraft, SmkScriptMetadataEditPreviewResult, SmkScriptMetadataFileWriteResult,
    SmkScriptMetadataWriteRequest, SmkStatus, SmkUpdateCheckInfo, SmkUpdateProgress,
    SmkUpdateStatusEntrySlice, SmkUtf8Slice, smk_edit_result_backup_generations,
    smk_edit_result_backup_record, smk_edit_result_file_write_result, smk_edit_result_free,
    smk_edit_result_metadata_edit_preview_result, smk_edit_result_text,
    smk_engine_cancel_current_operation, smk_engine_check_update_item,
    smk_engine_check_updates_for_items, smk_engine_create_default, smk_engine_free,
    smk_engine_generate_edit_password_sha256, smk_engine_last_error,
    smk_engine_read_script_metadata_edit_preview_file, smk_engine_render_distribution_metadata,
    smk_engine_restore_scriptmeta_backup, smk_engine_scan_folder, smk_engine_scan_folders,
    smk_engine_scan_folders_with_progress, smk_engine_scan_registered_roots, smk_engine_scan_roots,
    smk_engine_scriptmeta_backup_generations, smk_engine_set_resolve_macos_alias,
    smk_engine_set_roots, smk_engine_set_visible_root, smk_engine_verify_edit_password_sha256,
    smk_engine_write_script_metadata_file, smk_scan_result_change_info,
    smk_scan_result_file_entries, smk_scan_result_file_entry_changes, smk_scan_result_file_issues,
    smk_scan_result_file_items, smk_scan_result_file_lists, smk_scan_result_free,
    smk_scan_result_items, smk_scan_result_operation_info, smk_scan_result_roots,
    smk_scan_result_update_info, smk_scan_result_update_resolutions,
    smk_scan_result_update_statuses,
};
#[cfg(feature = "native-watch")]
use scriptmetakit_ffi::{
    smk_engine_poll_watcher_scan, smk_engine_poll_watcher_scan_dirty_only,
    smk_engine_start_watching, smk_engine_start_watching_with_callback, smk_engine_stop_watching,
};

#[test]
fn scans_items_through_opaque_handle_and_slice() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Example.jsx");
    std::fs::write(
        &script_path,
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.ffi
// Version: 1.2.3
// Name: FFI Example
// SCRIPTMETA-END
"#,
    )
    .expect("script");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );
    assert!(!engine.is_null());

    let path = temp.path().to_string_lossy().into_owned();
    let mut scan_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, path bytes are valid for the call, and `scan_result` is writable.
    assert_eq!(
        unsafe { smk_engine_scan_folder(engine, path.as_ptr(), path.len(), &mut scan_result) },
        SmkStatus::Ok
    );
    assert!(!scan_result.is_null());

    let mut items = SmkScriptItemSlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `items` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_items(scan_result, &mut items) },
        SmkStatus::Ok
    );
    assert_eq!(items.len, 1);

    // SAFETY: `items` is borrowed from `scan_result` and the result is still alive.
    let items = unsafe { slice::from_raw_parts(items.ptr, items.len) };
    assert_eq!(utf8(items[0].script_id), "com.example.ffi");
    assert_eq!(utf8(items[0].version), "1.2.3");
    assert_eq!(utf8(items[0].name), "FFI Example");
    assert_eq!(utf8(items[0].runtime_kind), "adobe_java_script");
    assert_eq!(items[0].has_scriptmeta, 1);
    assert_eq!(items[0].can_edit_scriptmeta, 1);
    assert_eq!(items[0].can_append_scriptmeta, 0);
    assert_eq!(utf8(items[0].scriptmeta_edit_state), "editable");

    let mut roots = SmkRootSnapshotSlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `roots` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_roots(scan_result, &mut roots) },
        SmkStatus::Ok
    );
    assert_eq!(roots.len, 1);

    let mut file_lists = SmkFileListSnapshotSlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `file_lists` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_file_lists(scan_result, &mut file_lists) },
        SmkStatus::Ok
    );
    assert_eq!(file_lists.len, 1);

    let mut file_entries = SmkFileEntrySlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `file_entries` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_file_entries(scan_result, &mut file_entries) },
        SmkStatus::Ok
    );
    assert_eq!(file_entries.len, 1);
    // SAFETY: `file_entries` is borrowed from `scan_result` and the result is still alive.
    let file_entries = unsafe { slice::from_raw_parts(file_entries.ptr, file_entries.len) };
    assert_eq!(file_entries[0].has_scriptmeta, 1);
    assert_eq!(file_entries[0].can_edit_scriptmeta, 1);
    assert_eq!(file_entries[0].can_append_scriptmeta, 0);
    assert_eq!(utf8(file_entries[0].scriptmeta_edit_state), "editable");

    let mut operation = SmkOperationInfo::default();
    // SAFETY: `scan_result` is live and `operation` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_operation_info(scan_result, &mut operation) },
        SmkStatus::Ok
    );
    assert_eq!(utf8(operation.status), "finished");
    assert_eq!(operation.total_units, 1);
    assert_eq!(operation.cancelled, 0);

    let mut file_issues = SmkFileIssueSlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `file_issues` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_file_issues(scan_result, &mut file_issues) },
        SmkStatus::Ok
    );
    assert_eq!(file_issues.len, 0);

    let mut update_info = SmkUpdateCheckInfo {
        has_update_check: 1,
        checked_at: 1,
    };
    // SAFETY: `scan_result` is live and `update_info` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_update_info(scan_result, &mut update_info) },
        SmkStatus::Ok
    );
    assert_eq!(update_info.has_update_check, 0);

    let mut statuses = SmkUpdateStatusEntrySlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `statuses` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_update_statuses(scan_result, &mut statuses) },
        SmkStatus::Ok
    );
    assert_eq!(statuses.len, 0);

    // SAFETY: both handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_scan_result_free(scan_result);
        smk_engine_free(engine);
    }
}

#[test]
fn configures_alias_resolution_through_ffi() {
    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );
    assert!(!engine.is_null());

    // SAFETY: `engine` is live and owned by this test.
    assert_eq!(
        unsafe { smk_engine_set_resolve_macos_alias(engine, 0) },
        SmkStatus::Ok
    );
    // SAFETY: `engine` is live and owned by this test.
    assert_eq!(
        unsafe { smk_engine_set_resolve_macos_alias(engine, 1) },
        SmkStatus::Ok
    );
    // SAFETY: `engine` is live and owned by this test.
    assert_eq!(
        unsafe { smk_engine_cancel_current_operation(engine) },
        SmkStatus::Ok
    );

    // SAFETY: `engine` was returned by `smk_engine_create_default` and has not been freed.
    unsafe {
        smk_engine_free(engine);
    }
}

#[test]
fn reads_script_metadata_edit_preview_through_ffi() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Preview.jsx");
    let source = format!(
        "{}\n/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.ffi.preview\nSCRIPTMETA-END\n*/\n",
        "alert('x');\n".repeat(512)
    );
    std::fs::write(&script_path, source.as_bytes()).expect("script");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let path = script_path.to_string_lossy().into_owned();
    let mut edit_result: *mut SmkEditResult = ptr::null_mut();
    // SAFETY: `engine` is live, path bytes remain valid for the call, and output is writable.
    assert_eq!(
        unsafe {
            smk_engine_read_script_metadata_edit_preview_file(
                engine,
                utf8_slice(&path),
                128,
                &mut edit_result,
            )
        },
        SmkStatus::Ok
    );
    assert!(!edit_result.is_null());

    let mut preview = SmkScriptMetadataEditPreviewResult::default();
    // SAFETY: `edit_result` is live and `preview` is a valid out pointer.
    assert_eq!(
        unsafe { smk_edit_result_metadata_edit_preview_result(edit_result, &mut preview) },
        SmkStatus::Ok
    );
    assert_eq!(preview.preview_byte_count, 128);
    assert_eq!(preview.file_size, source.len() as u64);
    assert_eq!(preview.has_file_size, 1);
    assert_eq!(utf8(preview.comment_style), "javascript_block");
    assert_eq!(preview.is_truncated, 1);
    assert_eq!(preview.requires_full_read, 1);
    assert_eq!(preview.has_scriptmeta_marker_in_preview, 0);
    assert!(!utf8(preview.preview_text).is_empty());
    assert!(!utf8(preview.file_state_fingerprint).is_empty());

    // SAFETY: both handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_edit_result_free(edit_result);
        smk_engine_free(engine);
    }
}

#[test]
fn scans_registered_roots_with_app_supplied_root_ids() {
    let temp = tempfile::tempdir().expect("tempdir");
    let first_root = temp.path().join("first");
    let second_root = temp.path().join("second");
    std::fs::create_dir_all(&first_root).expect("first root");
    std::fs::create_dir_all(&second_root).expect("second root");
    std::fs::write(
        first_root.join("One.jsx"),
        "// SCRIPTMETA-BEGIN\n// Script-ID: com.example.one\n// Version: 1.0.0\n// SCRIPTMETA-END\n",
    )
    .expect("one script");
    std::fs::write(
        second_root.join("Two.jsx"),
        "// SCRIPTMETA-BEGIN\n// Script-ID: com.example.two\n// Version: 2.0.0\n// SCRIPTMETA-END\n",
    )
    .expect("two script");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );
    assert!(!engine.is_null());

    let first_id = "scripta.photoshop";
    let second_id = "acemenu.background";
    let first_path = first_root.to_string_lossy().into_owned();
    let second_path = second_root.to_string_lossy().into_owned();
    let roots = [
        SmkRootRegistration {
            root_id: utf8_slice(first_id),
            path: utf8_slice(&first_path),
            display_name: utf8_slice("Photoshop Scripts"),
            purpose: 3,
            watch_policy: 2,
            cache_policy: 3,
            refresh_policy: 2,
            priority: 1,
        },
        SmkRootRegistration {
            root_id: utf8_slice(second_id),
            path: utf8_slice(&second_path),
            display_name: utf8_slice("ACEMenu Scripts"),
            purpose: 1,
            watch_policy: 0,
            cache_policy: 2,
            refresh_policy: 0,
            priority: 2,
        },
    ];

    // SAFETY: `engine` is live and root slices remain valid for the call.
    assert_eq!(
        unsafe { smk_engine_set_roots(engine, roots.as_ptr(), roots.len()) },
        SmkStatus::Ok
    );

    let mut scan_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, roots are configured, and `scan_result` is writable.
    assert_eq!(
        unsafe { smk_engine_scan_registered_roots(engine, 2, 0, &mut scan_result) },
        SmkStatus::Ok
    );
    assert!(!scan_result.is_null());

    let mut items = SmkScriptItemSlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `items` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_items(scan_result, &mut items) },
        SmkStatus::Ok
    );
    assert_eq!(items.len, 2);
    // SAFETY: `items` is borrowed from `scan_result` and the result is still alive.
    let items = unsafe { slice::from_raw_parts(items.ptr, items.len) };
    let root_ids = items
        .iter()
        .map(|item| utf8(item.root_id))
        .collect::<Vec<_>>();
    assert!(root_ids.iter().any(|value| value == first_id));
    assert!(root_ids.iter().any(|value| value == second_id));

    let selected_root_ids = [utf8_slice(second_id)];
    let mut selected_scan_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, root id slices remain valid for the call, and output is writable.
    assert_eq!(
        unsafe {
            smk_engine_scan_roots(
                engine,
                selected_root_ids.as_ptr(),
                selected_root_ids.len(),
                2,
                0,
                &mut selected_scan_result,
            )
        },
        SmkStatus::Ok
    );
    assert!(!selected_scan_result.is_null());

    let mut selected_items = SmkScriptItemSlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `selected_scan_result` is live and `selected_items` is writable.
    assert_eq!(
        unsafe { smk_scan_result_items(selected_scan_result, &mut selected_items) },
        SmkStatus::Ok
    );
    assert_eq!(selected_items.len, 1);
    // SAFETY: `selected_items` is borrowed from `selected_scan_result`.
    let selected_items = unsafe { slice::from_raw_parts(selected_items.ptr, selected_items.len) };
    assert_eq!(utf8(selected_items[0].root_id), second_id);
    assert_eq!(utf8(selected_items[0].script_id), "com.example.two");

    // SAFETY: `engine` is live and the root id slice is valid for the call.
    assert_eq!(
        unsafe { smk_engine_set_visible_root(engine, utf8_slice(second_id), 1) },
        SmkStatus::Ok
    );
    // SAFETY: `engine` is live; has_root_id=0 clears the visible root and ignores the empty slice.
    assert_eq!(
        unsafe { smk_engine_set_visible_root(engine, utf8_slice(""), 0) },
        SmkStatus::Ok
    );

    // SAFETY: both handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_scan_result_free(selected_scan_result);
        smk_scan_result_free(scan_result);
        smk_engine_free(engine);
    }
}

#[test]
fn keeps_file_list_direct_children_contiguous() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("Alpha").join("Nested")).expect("alpha dir");
    std::fs::create_dir_all(temp.path().join("Beta")).expect("beta dir");
    std::fs::create_dir_all(temp.path().join("Gamma")).expect("gamma dir");
    std::fs::write(
        temp.path().join("Alpha").join("Nested").join("Alpha.jsx"),
        "alert('alpha');",
    )
    .expect("alpha script");
    std::fs::write(temp.path().join("Beta").join("Beta.jsx"), "alert('beta');")
        .expect("beta script");
    std::fs::write(
        temp.path().join("Gamma").join("Gamma.jsx"),
        "alert('gamma');",
    )
    .expect("gamma script");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let path = temp.path().to_string_lossy().into_owned();
    let path_slice = SmkUtf8Slice {
        ptr: path.as_ptr(),
        len: path.len(),
    };
    let mut scan_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slice is valid for the call, and `scan_result` is writable.
    assert_eq!(
        unsafe { smk_engine_scan_folders(engine, &path_slice, 1, 0, &mut scan_result) },
        SmkStatus::Ok
    );
    assert!(!scan_result.is_null());

    let mut file_lists = SmkFileListSnapshotSlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `file_lists` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_file_lists(scan_result, &mut file_lists) },
        SmkStatus::Ok
    );
    assert_eq!(file_lists.len, 1);
    // SAFETY: `file_lists` is borrowed from `scan_result` and the result is still alive.
    let file_lists = unsafe { slice::from_raw_parts(file_lists.ptr, file_lists.len) };

    let mut file_entries = SmkFileEntrySlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `file_entries` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_file_entries(scan_result, &mut file_entries) },
        SmkStatus::Ok
    );
    assert!(file_entries.len >= 3);
    // SAFETY: `file_entries` is borrowed from `scan_result` and the result is still alive.
    let file_entries = unsafe { slice::from_raw_parts(file_entries.ptr, file_entries.len) };
    let root = file_lists[0];
    let root_children =
        &file_entries[root.first_child_index..root.first_child_index + root.child_count];
    let root_child_names: Vec<_> = root_children
        .iter()
        .map(|entry| {
            std::path::Path::new(&utf8(entry.display_path))
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(root_child_names, ["Alpha", "Beta", "Gamma"]);

    let alpha = &root_children[0];
    let alpha_children =
        &file_entries[alpha.first_child_index..alpha.first_child_index + alpha.child_count];
    assert_eq!(alpha_children.len(), 1);
    assert!(
        std::path::Path::new(&utf8(alpha_children[0].display_path))
            .ends_with(std::path::Path::new("Alpha").join("Nested"))
    );

    // SAFETY: both handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_scan_result_free(scan_result);
        smk_engine_free(engine);
    }
}

#[test]
fn returns_file_items_for_overlapping_registered_roots() {
    let temp = tempfile::tempdir().expect("tempdir");
    let parent = temp.path().join("Parent");
    let child = parent.join("JSX");
    std::fs::create_dir_all(&child).expect("child dir");
    std::fs::write(
        child.join("Shared.jsx"),
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.ffi-overlap
// Version: 1.0.0
// SCRIPTMETA-END
"#,
    )
    .expect("script");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let parent_path = parent.to_string_lossy().into_owned();
    let child_path = child.to_string_lossy().into_owned();
    let path_slices = [
        SmkUtf8Slice {
            ptr: parent_path.as_ptr(),
            len: parent_path.len(),
        },
        SmkUtf8Slice {
            ptr: child_path.as_ptr(),
            len: child_path.len(),
        },
    ];
    let mut scan_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slices are valid for the call, and `scan_result` is writable.
    assert_eq!(
        unsafe {
            smk_engine_scan_folders(
                engine,
                path_slices.as_ptr(),
                path_slices.len(),
                0,
                &mut scan_result,
            )
        },
        SmkStatus::Ok
    );
    assert!(!scan_result.is_null());

    let mut all_items = SmkScriptItemSlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `all_items` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_items(scan_result, &mut all_items) },
        SmkStatus::Ok
    );
    assert_eq!(all_items.len, 1);

    let mut file_items = SmkScriptItemSlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `file_items` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_file_items(scan_result, &mut file_items) },
        SmkStatus::Ok
    );
    assert_eq!(file_items.len, 2);
    // SAFETY: `file_items` is borrowed from `scan_result` and the result is still alive.
    let file_items = unsafe { slice::from_raw_parts(file_items.ptr, file_items.len) };
    let root_ids: std::collections::BTreeSet<_> =
        file_items.iter().map(|item| utf8(item.root_id)).collect();
    assert_eq!(root_ids.len(), 2);

    // SAFETY: both handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_scan_result_free(scan_result);
        smk_engine_free(engine);
    }
}

#[test]
fn scans_updates_through_multi_folder_ffi() {
    let temp = tempfile::tempdir().expect("tempdir");
    let first = temp.path().join("First");
    let second = temp.path().join("Second");
    std::fs::create_dir_all(&first).expect("first dir");
    std::fs::create_dir_all(&second).expect("second dir");
    let first_script_path = first.join("Example.jsx");
    let second_script_path = second.join("Example.jsx");
    let dist_path = temp.path().join("dist").join("SCRIPTMETA.txt");
    std::fs::create_dir_all(dist_path.parent().expect("dist parent")).expect("dist dir");
    let dist_url = url::Url::from_file_path(&dist_path).expect("file url");
    for script_path in [&first_script_path, &second_script_path] {
        std::fs::write(
            script_path,
            format!(
                r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.ffi.update
// Version: 2.0.0
// Meta-URL: {dist_url}
// Name: FFI Update Example
// SCRIPTMETA-END
"#
            ),
        )
        .expect("script");
    }
    std::fs::write(
        &dist_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.ffi.update
Latest-Version: 2.0.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("dist");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let first_path = first.to_string_lossy().into_owned();
    let second_path = second.to_string_lossy().into_owned();
    let path_slices = [utf8_slice(&first_path), utf8_slice(&second_path)];
    let mut scan_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slice is valid for the call, and `scan_result` is writable.
    assert_eq!(
        unsafe {
            smk_engine_scan_folders(
                engine,
                path_slices.as_ptr(),
                path_slices.len(),
                1,
                &mut scan_result,
            )
        },
        SmkStatus::Ok
    );
    assert!(!scan_result.is_null());

    let mut update_info = SmkUpdateCheckInfo {
        has_update_check: 0,
        checked_at: 0,
    };
    // SAFETY: `scan_result` is live and `update_info` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_update_info(scan_result, &mut update_info) },
        SmkStatus::Ok
    );
    assert_eq!(update_info.has_update_check, 1);

    let mut statuses = SmkUpdateStatusEntrySlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `statuses` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_update_statuses(scan_result, &mut statuses) },
        SmkStatus::Ok
    );
    assert_eq!(statuses.len, 2);
    // SAFETY: `statuses` is borrowed from `scan_result` and the result is still alive.
    let statuses = unsafe { slice::from_raw_parts(statuses.ptr, statuses.len) };
    let statuses_by_item_id = statuses
        .iter()
        .map(|status| (utf8(status.item_id), utf8(status.status)))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        statuses_by_item_id
            .get(&first_script_path.to_string_lossy().into_owned())
            .map(String::as_str),
        Some("up_to_date")
    );
    assert_eq!(
        statuses_by_item_id
            .get(&second_script_path.to_string_lossy().into_owned())
            .map(String::as_str),
        Some("up_to_date")
    );

    let mut resolutions = SmkDistributionResolutionEntrySlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `resolutions` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_update_resolutions(scan_result, &mut resolutions) },
        SmkStatus::Ok
    );
    assert_eq!(resolutions.len, 2);
    // SAFETY: `resolutions` is borrowed from `scan_result` and the result is still alive.
    let resolutions = unsafe { slice::from_raw_parts(resolutions.ptr, resolutions.len) };
    assert!(
        resolutions
            .iter()
            .all(|resolution| utf8(resolution.latest_version) == "2.0.0")
    );

    // SAFETY: both handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_scan_result_free(scan_result);
        smk_engine_free(engine);
    }
}

#[test]
fn checks_single_update_item_through_ffi() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Example.jsx");
    let dist_path = temp.path().join("SCRIPTMETA.txt");
    let dist_url = url::Url::from_file_path(&dist_path).expect("file url");
    std::fs::write(
        &script_path,
        format!(
            r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.ffi.single
// Version: 1.0.0
// Meta-URL: {dist_url}
// SCRIPTMETA-END
"#
        ),
    )
    .expect("script");
    std::fs::write(
        &dist_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.ffi.single
Latest-Version: 2.0.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("dist");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let path = temp.path().to_string_lossy().into_owned();
    let path_slice = utf8_slice(&path);
    let mut scan_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slice is valid for the call, and `scan_result` is writable.
    assert_eq!(
        unsafe { smk_engine_scan_folders(engine, &path_slice, 1, 0, &mut scan_result) },
        SmkStatus::Ok
    );

    let mut items = SmkScriptItemSlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `scan_result` is live and `items` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_items(scan_result, &mut items) },
        SmkStatus::Ok
    );
    assert_eq!(items.len, 1);

    let mut update_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `items.ptr` points to one item borrowed from a live scan result and output is writable.
    assert_eq!(
        unsafe { smk_engine_check_update_item(engine, items.ptr, &mut update_result) },
        SmkStatus::Ok
    );
    assert!(!update_result.is_null());

    let mut statuses = SmkUpdateStatusEntrySlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `update_result` is live and `statuses` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_update_statuses(update_result, &mut statuses) },
        SmkStatus::Ok
    );
    assert_eq!(statuses.len, 1);
    // SAFETY: `statuses` is borrowed from `update_result` and the result is still alive.
    let statuses = unsafe { slice::from_raw_parts(statuses.ptr, statuses.len) };
    assert_eq!(utf8(statuses[0].status), "update_available");

    // SAFETY: all handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_scan_result_free(update_result);
    }

    let mut batch_update_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `items.ptr` points to one item borrowed from a live scan result and output is writable.
    assert_eq!(
        unsafe {
            smk_engine_check_updates_for_items(
                engine,
                items.ptr,
                items.len,
                &mut batch_update_result,
            )
        },
        SmkStatus::Ok
    );
    assert!(!batch_update_result.is_null());

    let mut batch_statuses = SmkUpdateStatusEntrySlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `batch_update_result` is live and `batch_statuses` is a valid out pointer.
    assert_eq!(
        unsafe { smk_scan_result_update_statuses(batch_update_result, &mut batch_statuses) },
        SmkStatus::Ok
    );
    assert_eq!(batch_statuses.len, 1);
    // SAFETY: `batch_statuses` is borrowed from `batch_update_result` and the result is still alive.
    let batch_statuses = unsafe { slice::from_raw_parts(batch_statuses.ptr, batch_statuses.len) };
    assert_eq!(utf8(batch_statuses[0].status), "update_available");

    // SAFETY: all handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_scan_result_free(batch_update_result);
        smk_scan_result_free(scan_result);
        smk_engine_free(engine);
    }
}

#[test]
fn reports_update_progress_through_ffi_callback() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Example.jsx");
    let dist_path = temp.path().join("SCRIPTMETA.txt");
    let dist_url = url::Url::from_file_path(&dist_path).expect("file url");
    std::fs::write(
        &script_path,
        format!(
            r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.ffi.progress
// Version: 1.0.0
// Meta-URL: {dist_url}
// SCRIPTMETA-END
"#
        ),
    )
    .expect("script");
    std::fs::write(
        &dist_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.ffi.progress
Latest-Version: 2.0.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("dist");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let path = temp.path().to_string_lossy().into_owned();
    let path_slice = SmkUtf8Slice {
        ptr: path.as_ptr(),
        len: path.len(),
    };
    let mut phases = Vec::new();
    let mut scan_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slice and callback context are valid for this call.
    assert_eq!(
        unsafe {
            smk_engine_scan_folders_with_progress(
                engine,
                &path_slice,
                1,
                1,
                Some(collect_progress_phase),
                (&mut phases as *mut Vec<String>).cast::<c_void>(),
                &mut scan_result,
            )
        },
        SmkStatus::Ok
    );
    assert!(!scan_result.is_null());
    assert!(phases.iter().any(|phase| phase == "started"));
    assert!(phases.iter().any(|phase| phase == "checking"));
    assert!(phases.iter().any(|phase| phase == "finished_item"));
    assert!(phases.iter().any(|phase| phase == "finished"));

    // SAFETY: both handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_scan_result_free(scan_result);
        smk_engine_free(engine);
    }
}

#[test]
fn reports_file_changes_through_reused_engine() {
    let temp = tempfile::tempdir().expect("tempdir");
    let removed_path = temp.path().join("Removed.jsx");
    let modified_path = temp.path().join("Modified.jsx");
    let added_path = temp.path().join("Added.jsx");
    std::fs::write(&removed_path, "alert('removed');").expect("removed");
    std::fs::write(&modified_path, "alert('before');").expect("modified before");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let path = temp.path().to_string_lossy().into_owned();
    let path_slice = SmkUtf8Slice {
        ptr: path.as_ptr(),
        len: path.len(),
    };

    let mut first_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slice is valid for the call, and `first_result` is writable.
    assert_eq!(
        unsafe { smk_engine_scan_folders(engine, &path_slice, 1, 0, &mut first_result) },
        SmkStatus::Ok
    );
    assert!(!first_result.is_null());
    let mut first_change_info = SmkScanChangeInfo::default();
    // SAFETY: `first_result` is live and `first_change_info` is writable.
    assert_eq!(
        unsafe { smk_scan_result_change_info(first_result, &mut first_change_info) },
        SmkStatus::Ok
    );
    assert_eq!(first_change_info.has_change_summary, 0);
    // SAFETY: result handle was returned by this FFI crate and has not been freed.
    unsafe {
        smk_scan_result_free(first_result);
    }

    std::fs::remove_file(&removed_path).expect("remove");
    std::fs::write(&modified_path, "alert('after after');").expect("modified after");
    std::fs::write(&added_path, "alert('added');").expect("added");

    let mut second_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slice is valid for the call, and `second_result` is writable.
    assert_eq!(
        unsafe { smk_engine_scan_folders(engine, &path_slice, 1, 0, &mut second_result) },
        SmkStatus::Ok
    );
    assert!(!second_result.is_null());

    let mut change_info = SmkScanChangeInfo::default();
    // SAFETY: `second_result` is live and `change_info` is writable.
    assert_eq!(
        unsafe { smk_scan_result_change_info(second_result, &mut change_info) },
        SmkStatus::Ok
    );
    assert_eq!(change_info.has_change_summary, 1);
    assert_eq!(change_info.added_count, 1);
    assert_eq!(change_info.removed_count, 1);
    assert_eq!(change_info.modified_count, 1);

    let mut changes = SmkFileEntryChangeSlice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `second_result` is live and `changes` is writable.
    assert_eq!(
        unsafe { smk_scan_result_file_entry_changes(second_result, &mut changes) },
        SmkStatus::Ok
    );
    assert_eq!(changes.len, 3);
    // SAFETY: `changes` is borrowed from `second_result` and the result is still alive.
    let changes = unsafe { slice::from_raw_parts(changes.ptr, changes.len) };
    let kinds: std::collections::BTreeSet<_> =
        changes.iter().map(|change| utf8(change.kind)).collect();
    assert!(kinds.contains("added"));
    assert!(kinds.contains("removed"));
    assert!(kinds.contains("modified"));

    // SAFETY: both handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_scan_result_free(second_result);
        smk_engine_free(engine);
    }
}

#[test]
fn preserves_update_result_through_non_update_ffi_scan() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dist_path = temp.path().join("SCRIPTMETA.txt");
    let dist_url = url::Url::from_file_path(&dist_path).expect("dist url");
    std::fs::write(
        &dist_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.ffi.update.cache
Latest-Version: 2.0.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("dist");
    std::fs::write(
        temp.path().join("Example.jsx"),
        format!(
            r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.ffi.update.cache
// Version: 1.0.0
// Meta-URL: {dist_url}
// SCRIPTMETA-END
"#
        ),
    )
    .expect("script");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let path = temp.path().to_string_lossy().into_owned();
    let path_slice = SmkUtf8Slice {
        ptr: path.as_ptr(),
        len: path.len(),
    };

    let mut update_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slice is valid for the call, and `update_result` is writable.
    assert_eq!(
        unsafe { smk_engine_scan_folders(engine, &path_slice, 1, 1, &mut update_result) },
        SmkStatus::Ok
    );
    assert!(!update_result.is_null());
    let mut update_info = SmkUpdateCheckInfo::default();
    // SAFETY: `update_result` is live and `update_info` is writable.
    assert_eq!(
        unsafe { smk_scan_result_update_info(update_result, &mut update_info) },
        SmkStatus::Ok
    );
    assert_eq!(update_info.has_update_check, 1);
    // SAFETY: result handle was returned by this FFI crate and has not been freed.
    unsafe {
        smk_scan_result_free(update_result);
    }

    let mut scan_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slice is valid for the call, and `scan_result` is writable.
    assert_eq!(
        unsafe { smk_engine_scan_folders(engine, &path_slice, 1, 0, &mut scan_result) },
        SmkStatus::Ok
    );
    assert!(!scan_result.is_null());
    let mut preserved_info = SmkUpdateCheckInfo::default();
    // SAFETY: `scan_result` is live and `preserved_info` is writable.
    assert_eq!(
        unsafe { smk_scan_result_update_info(scan_result, &mut preserved_info) },
        SmkStatus::Ok
    );
    assert_eq!(preserved_info.has_update_check, 1);

    // SAFETY: handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_scan_result_free(scan_result);
        smk_engine_free(engine);
    }
}

#[cfg(feature = "native-watch")]
#[test]
fn starts_and_polls_native_watcher_through_ffi() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("Example.jsx"), "alert('ok');").expect("script");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let path = temp.path().to_string_lossy().into_owned();
    let path_slice = SmkUtf8Slice {
        ptr: path.as_ptr(),
        len: path.len(),
    };
    let mut scan_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slice is valid for the call, and `scan_result` is writable.
    assert_eq!(
        unsafe { smk_engine_scan_folders(engine, &path_slice, 1, 0, &mut scan_result) },
        SmkStatus::Ok
    );
    assert!(!scan_result.is_null());
    // SAFETY: result handle was returned by this FFI crate and has not been freed.
    unsafe {
        smk_scan_result_free(scan_result);
    }

    // SAFETY: `engine` is live and already has roots from the scan above.
    assert_eq!(unsafe { smk_engine_start_watching(engine) }, SmkStatus::Ok);

    let mut changed = 1_u8;
    let mut changed_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live and output pointers are writable.
    assert_eq!(
        unsafe { smk_engine_poll_watcher_scan(engine, &mut changed, &mut changed_result) },
        SmkStatus::Ok
    );
    assert_eq!(changed, 0);
    assert!(changed_result.is_null());

    // SAFETY: `engine` is live.
    assert_eq!(unsafe { smk_engine_stop_watching(engine) }, SmkStatus::Ok);

    // SAFETY: handle was returned by this FFI crate and has not been freed.
    unsafe {
        smk_engine_free(engine);
    }
}

#[cfg(feature = "native-watch")]
#[test]
fn notifies_when_native_watcher_receives_change() {
    extern "C" fn watch_callback(context: *mut c_void) {
        if context.is_null() {
            return;
        }
        // SAFETY: the test passes a live `AtomicUsize` pointer and keeps it
        // alive until after the watcher is stopped.
        let counter = unsafe { &*(context as *const AtomicUsize) };
        counter.fetch_add(1, Ordering::SeqCst);
    }

    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("Example.jsx"), "alert('ok');").expect("script");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let path = temp.path().to_string_lossy().into_owned();
    let path_slice = SmkUtf8Slice {
        ptr: path.as_ptr(),
        len: path.len(),
    };
    let mut scan_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slice is valid for the call, and `scan_result` is writable.
    assert_eq!(
        unsafe { smk_engine_scan_folders(engine, &path_slice, 1, 0, &mut scan_result) },
        SmkStatus::Ok
    );
    assert!(!scan_result.is_null());
    // SAFETY: result handle was returned by this FFI crate and has not been freed.
    unsafe {
        smk_scan_result_free(scan_result);
    }

    let notification_count = AtomicUsize::new(0);
    // SAFETY: `engine` is live and already has roots from the scan above. The
    // context points to `notification_count`, which outlives the watcher.
    assert_eq!(
        unsafe {
            smk_engine_start_watching_with_callback(
                engine,
                Some(watch_callback),
                &notification_count as *const AtomicUsize as *mut c_void,
            )
        },
        SmkStatus::Ok
    );

    thread::sleep(Duration::from_millis(250));
    std::fs::write(temp.path().join("Added.jsx"), "alert('added');").expect("added");

    let deadline = Instant::now() + Duration::from_secs(5);
    while notification_count.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert!(notification_count.load(Ordering::SeqCst) > 0);

    let mut changed = 0_u8;
    let mut changed_result: *mut SmkScanResult = ptr::null_mut();
    while changed == 0 && Instant::now() < deadline {
        // SAFETY: `engine` is live and output pointers are writable.
        assert_eq!(
            unsafe {
                smk_engine_poll_watcher_scan_dirty_only(engine, &mut changed, &mut changed_result)
            },
            SmkStatus::Ok
        );
        if changed == 0 {
            thread::sleep(Duration::from_millis(50));
        }
    }
    assert_eq!(changed, 1);
    assert!(!changed_result.is_null());

    let mut change_info = SmkScanChangeInfo::default();
    // SAFETY: `changed_result` is live and `change_info` is writable.
    assert_eq!(
        unsafe { smk_scan_result_change_info(changed_result, &mut change_info) },
        SmkStatus::Ok
    );
    assert_eq!(change_info.has_change_summary, 1);
    assert!(change_info.added_count >= 1);

    // SAFETY: handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_scan_result_free(changed_result);
        smk_engine_stop_watching(engine);
        smk_engine_free(engine);
    }
}

#[test]
fn stores_last_error_on_invalid_argument() {
    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let mut scan_result: *mut SmkScanResult = ptr::null_mut();
    // SAFETY: empty input is allowed as a checked invalid argument, and `scan_result` is writable.
    assert_eq!(
        unsafe { smk_engine_scan_folder(engine, ptr::null(), 0, &mut scan_result) },
        SmkStatus::InvalidArgument
    );
    assert!(scan_result.is_null());

    let mut error = SmkUtf8Slice {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: `engine` is live and `error` is a valid out pointer.
    assert_eq!(
        unsafe { smk_engine_last_error(engine, &mut error) },
        SmkStatus::Ok
    );
    assert_eq!(utf8(error), "folder path is empty");

    // SAFETY: `engine` was returned by this FFI crate and has not been freed.
    unsafe {
        smk_engine_free(engine);
    }
}

#[test]
fn writes_script_metadata_and_restores_backup_through_ffi() {
    let temp = tempfile::tempdir().expect("tempdir");
    let backup_root = temp.path().join("Backups");
    let script_path = temp.path().join("Example.jsx");
    std::fs::write(&script_path, "alert('before');\n").expect("script");

    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let file_path = script_path.to_string_lossy().into_owned();
    let backup_root_path = backup_root.to_string_lossy().into_owned();
    let script_id = "com.example.editkit";
    let version = "1.0.0";
    let name = "EditKit Example";
    let draft = SmkScriptMetadataDraft {
        script_id: utf8_slice(script_id),
        version: utf8_slice(version),
        name: utf8_slice(name),
        ..SmkScriptMetadataDraft::default()
    };
    let request = SmkScriptMetadataWriteRequest {
        file_path: utf8_slice(&file_path),
        backup_root_path: utf8_slice(&backup_root_path),
        write_mode: 0,
        draft,
    };
    let mut edit_result: *mut SmkEditResult = ptr::null_mut();
    // SAFETY: `engine` is live, request slices are valid for this call, and output is writable.
    assert_eq!(
        unsafe { smk_engine_write_script_metadata_file(engine, &request, &mut edit_result) },
        SmkStatus::Ok
    );
    assert!(!edit_result.is_null());

    let mut write_result = SmkScriptMetadataFileWriteResult::default();
    // SAFETY: `edit_result` is live and output is writable.
    assert_eq!(
        unsafe { smk_edit_result_file_write_result(edit_result, &mut write_result) },
        SmkStatus::Ok
    );
    assert_eq!(utf8(write_result.operation), "inserted");
    assert_eq!(write_result.has_backup, 1);
    assert!(!utf8(write_result.backup.id).is_empty());
    let updated = std::fs::read_to_string(&script_path).expect("updated");
    assert!(updated.contains("SCRIPTMETA-BEGIN"));
    assert!(updated.contains("Script-ID=com.example.editkit"));
    // SAFETY: handle was returned by this FFI crate and has not been freed.
    unsafe {
        smk_edit_result_free(edit_result);
    }

    let mut generations_result: *mut SmkEditResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slices are valid for this call, and output is writable.
    assert_eq!(
        unsafe {
            smk_engine_scriptmeta_backup_generations(
                engine,
                utf8_slice(&file_path),
                utf8_slice(&backup_root_path),
                &mut generations_result,
            )
        },
        SmkStatus::Ok
    );
    assert!(!generations_result.is_null());
    let mut generation_slice = SmkScriptMetaBackupGenerationSlice::default();
    // SAFETY: `generations_result` is live and output is writable.
    assert_eq!(
        unsafe { smk_edit_result_backup_generations(generations_result, &mut generation_slice) },
        SmkStatus::Ok
    );
    // SAFETY: the slice is borrowed from a live edit result.
    let generations = unsafe { slice::from_raw_parts(generation_slice.ptr, generation_slice.len) };
    assert!(generations.len() >= 2);
    let original_generation_id = generations
        .iter()
        .find(|generation| generation.is_current_file == 0)
        .map(|generation| utf8(generation.id))
        .expect("original generation");
    // SAFETY: handle was returned by this FFI crate and has not been freed.
    unsafe {
        smk_edit_result_free(generations_result);
    }

    let mut restore_result: *mut SmkEditResult = ptr::null_mut();
    // SAFETY: `engine` is live, path slices are valid for this call, and output is writable.
    assert_eq!(
        unsafe {
            smk_engine_restore_scriptmeta_backup(
                engine,
                utf8_slice(&file_path),
                utf8_slice(&backup_root_path),
                utf8_slice(&original_generation_id),
                &mut restore_result,
            )
        },
        SmkStatus::Ok
    );
    assert!(!restore_result.is_null());
    let mut has_record = 0_u8;
    let mut record = SmkScriptMetaBackupRecord::default();
    // SAFETY: `restore_result` is live and output pointers are writable.
    assert_eq!(
        unsafe { smk_edit_result_backup_record(restore_result, &mut has_record, &mut record) },
        SmkStatus::Ok
    );
    assert_eq!(has_record, 1);
    assert_eq!(utf8(record.reason), "before_restore");
    let restored = std::fs::read_to_string(&script_path).expect("restored");
    assert!(!restored.contains("SCRIPTMETA-BEGIN"));
    assert!(restored.contains("alert('before');"));

    // SAFETY: handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_edit_result_free(restore_result);
        smk_engine_free(engine);
    }
}

#[test]
fn renders_distribution_metadata_through_ffi() {
    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let script_id = "com.example.dist";
    let version = "2.0.0";
    let latest_url = "https://example.com/SCRIPTMETA.txt";
    let records = [SmkDistributionMetadataDraft {
        script_id: utf8_slice(script_id),
        version: utf8_slice(version),
        latest_url: utf8_slice(latest_url),
        ..SmkDistributionMetadataDraft::default()
    }];
    let mut result: *mut SmkEditResult = ptr::null_mut();
    // SAFETY: `engine` is live, records are valid for this call, and output is writable.
    assert_eq!(
        unsafe {
            smk_engine_render_distribution_metadata(engine, records.as_ptr(), 1, &mut result)
        },
        SmkStatus::Ok
    );
    assert!(!result.is_null());
    let mut text = SmkUtf8Slice::default();
    // SAFETY: `result` is live and output is writable.
    assert_eq!(
        unsafe { smk_edit_result_text(result, &mut text) },
        SmkStatus::Ok
    );
    let text = utf8(text);
    assert!(text.contains("SCRIPTMETA-DIST-BEGIN"));
    assert!(text.contains("Script-ID=com.example.dist"));
    assert!(text.contains("Latest-URL=https://example.com/SCRIPTMETA.txt"));

    // SAFETY: handles were returned by this FFI crate and have not been freed.
    unsafe {
        smk_edit_result_free(result);
        smk_engine_free(engine);
    }
}

#[test]
fn ffi_generates_and_verifies_edit_password_sha256() {
    let mut engine: *mut SmkEngine = ptr::null_mut();
    // SAFETY: `engine` is a valid out pointer for the duration of this call.
    assert_eq!(
        unsafe { smk_engine_create_default(&mut engine) },
        SmkStatus::Ok
    );

    let mut result: *mut SmkEditResult = ptr::null_mut();
    // SAFETY: `engine` is live and output is writable.
    assert_eq!(
        unsafe {
            smk_engine_generate_edit_password_sha256(engine, utf8_slice("secret"), &mut result)
        },
        SmkStatus::Ok
    );
    assert!(!result.is_null());

    let mut stored = SmkUtf8Slice::default();
    // SAFETY: `result` is live and output is writable.
    assert_eq!(
        unsafe { smk_edit_result_text(result, &mut stored) },
        SmkStatus::Ok
    );

    let stored_text = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(stored.ptr, stored.len))
    };
    let mut is_match = 0;
    // SAFETY: `engine` is live and output is writable.
    assert_eq!(
        unsafe {
            smk_engine_verify_edit_password_sha256(
                engine,
                utf8_slice("secret"),
                utf8_slice(stored_text),
                &mut is_match,
            )
        },
        SmkStatus::Ok
    );
    assert_eq!(is_match, 1);

    assert_eq!(
        unsafe {
            smk_engine_verify_edit_password_sha256(
                engine,
                utf8_slice("secret"),
                utf8_slice("invalid"),
                &mut is_match,
            )
        },
        SmkStatus::InvalidArgument
    );

    // SAFETY: result and engine were returned by this crate and are live.
    unsafe {
        smk_edit_result_free(result);
        smk_engine_free(engine);
    }
}

fn utf8(value: SmkUtf8Slice) -> String {
    if value.ptr.is_null() || value.len == 0 {
        return String::new();
    }
    // SAFETY: tests only read slices returned by live FFI handles.
    let bytes = unsafe { slice::from_raw_parts(value.ptr, value.len) };
    String::from_utf8(bytes.to_vec()).expect("utf8")
}

fn utf8_slice(value: &str) -> SmkUtf8Slice {
    SmkUtf8Slice {
        ptr: value.as_ptr(),
        len: value.len(),
    }
}

extern "C" fn collect_progress_phase(progress: *const SmkUpdateProgress, context: *mut c_void) {
    if progress.is_null() || context.is_null() {
        return;
    }
    // SAFETY: the test passes a live `Vec<String>` context for the duration of the callback.
    let phases = unsafe { &mut *context.cast::<Vec<String>>() };
    // SAFETY: the callback receives a live progress pointer for this call.
    let progress = unsafe { &*progress };
    phases.push(utf8(progress.phase));
    assert!(progress.completed_items <= progress.total_items);
}
