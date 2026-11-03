use scriptmetakit_ffi as ffi;
use std::{ptr, slice};

fn input(value: &str) -> ffi::SmkUtf8Slice {
    ffi::SmkUtf8Slice {
        ptr: value.as_ptr(),
        len: value.len(),
    }
}

fn response(
    call: impl FnOnce(*mut *mut ffi::SmkEditResult) -> ffi::SmkStatus,
) -> (ffi::SmkStatus, String) {
    let mut result = ptr::null_mut();
    let status = call(&mut result);
    assert!(!result.is_null());
    let mut value = ffi::SmkUtf8Slice::default();
    // SAFETY: the native result owns its text until the matching free below.
    unsafe {
        assert_eq!(
            ffi::smk_edit_result_text(result, &mut value),
            ffi::SmkStatus::Ok
        );
        let text = if value.len == 0 {
            String::new()
        } else {
            String::from_utf8(slice::from_raw_parts(value.ptr, value.len).to_vec()).unwrap()
        };
        ffi::smk_edit_result_free(result);
        (status, text)
    }
}

#[test]
fn service_errors_return_owned_text_and_never_require_an_engine() {
    // SAFETY: input slices remain live; response owns and frees each output.
    unsafe {
        let (status, text) = response(|out| ffi::smk_normalize_language_tag(input("jp"), out));
        assert_eq!(status, ffi::SmkStatus::InvalidArgument);
        assert!(text.contains("language tag"));
        let (status, _) = response(|out| ffi::smk_metadata_render_text(input("{}"), 0, out));
        assert_eq!(status, ffi::SmkStatus::InvalidArgument);
        let invalid_utf8 = [0xff];
        let (status, _) = response(|out| {
            ffi::smk_normalize_language_tag(
                ffi::SmkUtf8Slice {
                    ptr: invalid_utf8.as_ptr(),
                    len: 1,
                },
                out,
            )
        });
        assert_eq!(status, ffi::SmkStatus::InvalidUtf8);
        let (status, _) = response(|out| ffi::smk_engine_configuration_json(ptr::null_mut(), out));
        assert_eq!(status, ffi::SmkStatus::NullArgument);
        assert_eq!(
            ffi::smk_runtime_info_json(ptr::null_mut()),
            ffi::SmkStatus::NullArgument
        );
    }
}

#[test]
fn complete_configuration_is_validated_before_native_state_changes() {
    // SAFETY: all inputs, the engine and output slots remain live for each call.
    unsafe {
        let mut engine = ptr::null_mut();
        assert_eq!(
            ffi::smk_engine_create_default(&mut engine),
            ffi::SmkStatus::Ok
        );
        let (status, original) = response(|out| ffi::smk_engine_configuration_json(engine, out));
        assert_eq!(status, ffi::SmkStatus::Ok);
        let (_, defaults) = response(|out| ffi::smk_configuration_default_json(out));
        assert_eq!(original, defaults);
        let mut changed: serde_json::Value = serde_json::from_str(&original).unwrap();
        changed["parser"]["preferred_languages"] = serde_json::json!(["JA", "en", "fr"]);
        changed["update_check"]["max_concurrent_meta_url_checks"] = 0.into();
        let invalid = changed.to_string();
        let (status, _) =
            response(|out| ffi::smk_engine_apply_configuration_json(engine, input(&invalid), out));
        assert_eq!(status, ffi::SmkStatus::InvalidArgument);
        let (_, after) = response(|out| ffi::smk_engine_configuration_json(engine, out));
        assert_eq!(after, original);
        changed["update_check"]["max_concurrent_meta_url_checks"] = 4.into();
        let valid = changed.to_string();
        let (status, output) =
            response(|out| ffi::smk_engine_apply_configuration_json(engine, input(&valid), out));
        assert_eq!(status, ffi::SmkStatus::Ok);
        let applied: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(
            applied["configuration"]["parser"]["preferred_languages"],
            serde_json::json!(["ja", "en", "fr"])
        );
        assert_eq!(applied["change"]["metadata_invalidated"], true);
        ffi::smk_engine_free(engine);
    }
}

#[test]
fn parse_and_edit_outputs_preserve_selected_and_full_language_boundaries() {
    let source = "/*\nSCRIPTMETA-BEGIN\nScript-ID=services\nMeta-Default-language=ja\nName=日本語\nName[en]=English\nTags=a,b\nDescription-BEGIN\n日本語本文\n[en]English body\nDescription-END\nSCRIPTMETA-END\n*/";
    let languages = [input("en")];
    // SAFETY: all borrowed input bytes are live and response owns the output handles.
    unsafe {
        let (status, selected) = response(|out| {
            ffi::smk_metadata_parse_text(input(source), languages.as_ptr(), 1, 1, out)
        });
        assert_eq!(status, ffi::SmkStatus::Ok);
        assert!(selected.contains("English body"));
        assert!(!selected.contains("日本語本文"));
        let (status, edit) = response(|out| {
            ffi::smk_metadata_read_draft_text(input(source), input("sample.jsx"), out)
        });
        assert_eq!(status, ffi::SmkStatus::Ok);
        assert!(edit.contains("日本語本文"));
        assert!(edit.contains("English body"));
        let edit: serde_json::Value = serde_json::from_str(&edit).unwrap();
        let draft = edit["draft"].to_string();
        let (status, rendered) =
            response(|out| ffi::smk_metadata_render_text(input(&draft), 2, out));
        assert_eq!(status, ffi::SmkStatus::Ok);
        assert!(rendered.starts_with("(*"));
    }
}

#[test]
fn json_editing_accepts_empty_and_normalizable_urls_like_the_typed_api() {
    for (url, expected) in [
        ("", None),
        ("example.com/metadata", Some("https://example.com/metadata")),
    ] {
        let draft = serde_json::json!({"script_id":"example", "name":"Example", "version":"", "meta_url":url}).to_string();
        // SAFETY: the JSON remains live through the synchronous native call.
        let (status, text) =
            response(|out| unsafe { ffi::smk_metadata_render_text(input(&draft), 0, out) });
        assert_eq!(status, ffi::SmkStatus::Ok);
        let parsed = scriptmetakit::parse_script_metadata(&text).unwrap();
        assert_eq!(parsed.meta_url.as_ref().map(|url| url.as_str()), expected);
        assert!(parsed.version.is_none());
    }
    let invalid = r#"{"script_id":"example","meta_url":"javascript:alert(1)"}"#;
    // SAFETY: all input/output pointers are live for the call.
    let (status, _) =
        response(|out| unsafe { ffi::smk_metadata_render_text(input(invalid), 0, out) });
    assert_eq!(status, ffi::SmkStatus::InvalidArgument);
}

#[test]
fn cache_revision_query_matches_full_results_without_serializing_metadata() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("sample.jsx"), "/*\nSCRIPTMETA-BEGIN\nScript-ID=revisions\nName=CONTENT_MUST_NOT_ESCAPE\nSCRIPTMETA-END\n*/").unwrap();
    let path = directory.path().to_str().unwrap();
    // SAFETY: each owned engine/result is freed after all borrowed slices are read.
    unsafe {
        let mut engine = ptr::null_mut();
        assert_eq!(
            ffi::smk_engine_create_default(&mut engine),
            ffi::SmkStatus::Ok
        );
        let mut scanned = ptr::null_mut();
        assert_eq!(
            ffi::smk_engine_scan_folder(engine, path.as_ptr(), path.len(), &mut scanned),
            ffi::SmkStatus::Ok
        );
        ffi::smk_scan_result_free(scanned);
        let mut full = ptr::null_mut();
        assert_eq!(
            ffi::smk_engine_cached_roots(engine, ptr::null(), 0, 2, &mut full),
            ffi::SmkStatus::Ok
        );
        let mut revisions = ffi::SmkRootSnapshotRevisionSlice::default();
        assert_eq!(
            ffi::smk_scan_result_root_revisions(full, &mut revisions),
            ffi::SmkStatus::Ok
        );
        assert_eq!(revisions.len, 1);
        let (status, json) =
            response(|out| ffi::smk_engine_cache_revisions_json(engine, ptr::null(), 0, 2, out));
        assert_eq!(status, ffi::SmkStatus::Ok);
        assert!(!json.contains("CONTENT_MUST_NOT_ESCAPE"));
        let values: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            values[0]["state_revision"]["sequence"],
            (*revisions.ptr).sequence
        );
        assert!(values[0]["file_list_revision"].is_object());
        ffi::smk_scan_result_free(full);
        ffi::smk_engine_free(engine);
    }
}
