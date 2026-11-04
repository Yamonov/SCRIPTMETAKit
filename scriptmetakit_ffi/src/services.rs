//! Low-frequency configuration and text tools. Scan results keep their typed ABI.
use super::*;
use serde::Serialize;

type ServiceResult<T> = Result<T, (SmkStatus, String)>;

fn utf8_slice<'a>(value: SmkUtf8Slice) -> ServiceResult<&'a str> {
    utf8_from_raw(value.ptr, value.len)
}

fn service_error(error: scriptmetakit::ScriptMetaKitError) -> (SmkStatus, String) {
    let status = match error {
        scriptmetakit::ScriptMetaKitError::InvalidConfig(_)
        | scriptmetakit::ScriptMetaKitError::Parse(_)
        | scriptmetakit::ScriptMetaKitError::Url(_) => SmkStatus::InvalidArgument,
        _ => SmkStatus::EngineError,
    };
    (status, error.to_string())
}

fn json<T: Serialize>(value: &T) -> ServiceResult<String> {
    serde_json::to_string(value).map_err(|error| (SmkStatus::EngineError, error.to_string()))
}

fn decode<T: serde::de::DeserializeOwned>(value: SmkUtf8Slice) -> ServiceResult<T> {
    serde_json::from_str(utf8_slice(value)?)
        .map_err(|error| (SmkStatus::InvalidArgument, error.to_string()))
}

fn service_text(
    output: *mut *mut SmkEditResult,
    operation: impl FnOnce() -> ServiceResult<String>,
) -> SmkStatus {
    ffi_guard(|| {
        let output = out_mut(output)?;
        *output = ptr::null_mut();
        match operation() {
            Ok(text) => {
                *output = Box::into_raw(Box::new(SmkEditResult::from_owned_text(text)));
                Ok(())
            }
            Err((status, message)) => {
                *output = Box::into_raw(Box::new(SmkEditResult::from_owned_text(message.clone())));
                Err((status, message))
            }
        }
    })
}

pub(super) fn default_configuration() -> ScriptMetaKitConfig {
    let mut config = ScriptMetaKitConfig::default();
    config.watcher.watch_policy = WatchPolicy::AllRegistered;
    config
}

#[unsafe(no_mangle)]
/// # Safety
/// `output` is writable. Free every returned handle, including error text, with smk_edit_result_free.
pub unsafe extern "C" fn smk_configuration_default_json(
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || json(&default_configuration()))
}

#[unsafe(no_mangle)]
/// # Safety
/// Engine is live; output follows the text-service ownership contract.
pub unsafe extern "C" fn smk_engine_configuration_json(
    engine: *mut SmkEngine,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        let engine = engine_mut(engine)?;
        json(engine.engine.config())
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Engine and root-id slices are live for the call. Output owns revision JSON,
/// without script text or file trees. This is not a source-freshness decision.
pub unsafe extern "C" fn smk_engine_cache_revisions_json(
    engine: *mut SmkEngine,
    root_ids: *const SmkUtf8Slice,
    count: usize,
    mode: u32,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        let request = ScanRequest {
            root_ids: root_ids_from_raw(root_ids, count)?,
            mode: scan_mode_from_u32(mode)?,
        };
        let mut state = engine_mut(engine)?;
        let result = state.engine.cached_scan_result(request);
        #[derive(Serialize)]
        struct Revision<'a> {
            root_id: &'a str,
            state_revision: &'a scriptmetakit::SnapshotRevision,
            file_list_revision: Option<&'a scriptmetakit::SnapshotRevision>,
        }
        let revisions: Vec<_> = result
            .roots
            .iter()
            .map(|root| Revision {
                root_id: root.root_id.as_ref(),
                state_revision: &root.state_revision,
                file_list_revision: result
                    .file_list_snapshots
                    .iter()
                    .find(|snapshot| {
                        snapshot.root.root_id == root.root_id && snapshot.children.is_some()
                    })
                    .map(|snapshot| &snapshot.content_revision),
            })
            .collect();
        json(&revisions)
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Engine and UTF-8 input are live for this call; output follows the text-service ownership contract.
pub unsafe extern "C" fn smk_engine_apply_configuration_json(
    engine: *mut SmkEngine,
    configuration: SmkUtf8Slice,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    let mut notification = empty_pending_watch_notification();
    let status = service_text(output, || {
        let configuration: ScriptMetaKitConfig = decode(configuration)?;
        ScriptMetaKitEngine::validate_configuration(&configuration).map_err(service_error)?;
        let mut state = engine_mut(engine)?;
        state.clear_error();
        let mut change = scriptmetakit::EngineConfigurationChange::default();
        notification = apply_engine_reconfiguration(&mut state, |candidate| {
            change = candidate
                .apply_configuration(configuration)
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .map_err(|message| {
            state.set_error(&message);
            (SmkStatus::EngineError, message)
        })?;
        #[derive(Serialize)]
        struct Applied<'a> {
            configuration: &'a ScriptMetaKitConfig,
            change: scriptmetakit::EngineConfigurationChange,
        }
        json(&Applied {
            configuration: state.engine.config(),
            change,
        })
    });
    if status == SmkStatus::Ok {
        emit_pending_watch_notification(notification);
    }
    status
}

#[unsafe(no_mangle)]
/// # Safety
/// Inputs are readable UTF-8 slices; language array has count elements. Output owns JSON metadata.
pub unsafe extern "C" fn smk_metadata_parse_text(
    source: SmkUtf8Slice,
    languages: *const SmkUtf8Slice,
    language_count: usize,
    require_closed_block: u8,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    unsafe {
        smk_metadata_parse_text_v2(
            source,
            languages,
            language_count,
            require_closed_block,
            ptr::null(),
            0,
            output,
        )
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// Source and each array element are readable UTF-8 for the call. Output owns JSON.
pub unsafe extern "C" fn smk_metadata_parse_text_v2(
    source: SmkUtf8Slice,
    languages: *const SmkUtf8Slice,
    language_count: usize,
    require_closed_block: u8,
    custom_keys: *const SmkUtf8Slice,
    custom_key_count: usize,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        let languages = utf8_strings_from_raw(languages, language_count)?;
        let options = scriptmetakit::ParserOptions {
            custom_metadata_keys: scriptmetakit::CustomMetadataSelection::new(
                utf8_strings_from_raw(custom_keys, custom_key_count)?,
            )
            .map_err(service_error)?,
            require_closed_block: require_closed_block != 0,
            preferred_languages: scriptmetakit::LanguagePreferences::new(languages)
                .map_err(service_error)?,
            ..Default::default()
        };
        json(
            &scriptmetakit::parse_script_metadata_with_options(utf8_slice(source)?, &options)
                .map_err(service_error)?,
        )
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Key/value are readable UTF-8. Output owns a JSON array of validation diagnostics.
pub unsafe extern "C" fn smk_validate_custom_metadata_field(
    key: SmkUtf8Slice,
    value: SmkUtf8Slice,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        json(&scriptmetakit::validate_custom_metadata_field(
            utf8_slice(key)?,
            utf8_slice(value)?,
        ))
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Key is readable UTF-8. Output owns the canonical key or an error message.
pub unsafe extern "C" fn smk_normalize_custom_metadata_key(
    key: SmkUtf8Slice,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        scriptmetakit::normalize_custom_metadata_key(utf8_slice(key)?).map_err(service_error)
    })
}

#[derive(Serialize)]
struct TextDraft<'a> {
    file_path: &'a Path,
    draft: &'a KitScriptMetadataDraft,
    comment_style: &'static str,
    line_ending: &'a str,
    has_existing_block: bool,
    existing_lines: &'a [String],
    unknown_lines: &'a [String],
    existing_block_text: Option<&'a str>,
    source_fingerprint: &'a str,
    diagnostics: &'a [scriptmetakit::MetadataDiagnostic],
}

#[unsafe(no_mangle)]
/// # Safety
/// Source/name are readable UTF-8; name selects syntax without file I/O. Output owns a full draft JSON.
pub unsafe extern "C" fn smk_metadata_read_draft_text(
    source: SmkUtf8Slice,
    file_name: SmkUtf8Slice,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        let read = scriptmetakit::read_script_metadata_draft_from_text(
            utf8_slice(source)?,
            Path::new(utf8_slice(file_name)?),
        )
        .map_err(service_error)?;
        json(&TextDraft {
            file_path: &read.file_path,
            draft: &read.draft,
            comment_style: script_meta_comment_style(read.comment_style),
            line_ending: &read.line_ending,
            has_existing_block: read.has_existing_block,
            existing_lines: &read.existing_lines,
            unknown_lines: &read.unknown_lines,
            existing_block_text: read.existing_block_text.as_deref(),
            source_fingerprint: &read.source_fingerprint,
            diagnostics: &read.diagnostics,
        })
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Draft is readable UTF-8 JSON. Style: 0 plain, 1 JavaScript block, 2 AppleScript block.
pub unsafe extern "C" fn smk_metadata_render_text(
    draft: SmkUtf8Slice,
    style: u32,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        let draft: KitScriptMetadataDraft = decode(draft)?;
        let style = match style {
            0 => scriptmetakit::ScriptMetaCommentStyle::PlainText,
            1 => scriptmetakit::ScriptMetaCommentStyle::JavaScriptBlock,
            2 => scriptmetakit::ScriptMetaCommentStyle::AppleScriptBlock,
            _ => return Err((SmkStatus::InvalidArgument, "invalid comment style".into())),
        };
        scriptmetakit::render_script_metadata_for_style(&draft, style).map_err(service_error)
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Inputs are readable UTF-8. Only the supplied text is edited; no filesystem writes occur.
pub unsafe extern "C" fn smk_metadata_write_text(
    source: SmkUtf8Slice,
    file_name: SmkUtf8Slice,
    draft: SmkUtf8Slice,
    mode: u32,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        let draft: KitScriptMetadataDraft = decode(draft)?;
        let written = scriptmetakit::write_script_metadata_to_text(
            utf8_slice(source)?,
            Path::new(utf8_slice(file_name)?),
            &draft,
            script_meta_write_mode(mode)?,
        )
        .map_err(service_error)?;
        Ok(written.text)
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Inputs are readable UTF-8. Empty script_id selects all records; otherwise Kit selection rules apply.
pub unsafe extern "C" fn smk_distribution_parse_text(
    source: SmkUtf8Slice,
    script_id: SmkUtf8Slice,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        let source = utf8_slice(source)?;
        let script_id = utf8_slice(script_id)?;
        let records = if script_id.is_empty() {
            scriptmetakit::parse_distribution_metadata_records(source).map_err(service_error)?
        } else {
            vec![
                scriptmetakit::parse_distribution_metadata_for_script(source, script_id)
                    .map_err(service_error)?,
            ]
        };
        json(&records)
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Value is readable UTF-8; output contains the normalized BCP 47 tag or error text.
pub unsafe extern "C" fn smk_normalize_language_tag(
    value: SmkUtf8Slice,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        scriptmetakit::normalize_language_tag(utf8_slice(value)?).map_err(service_error)
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Values contains count readable UTF-8 slices. Output is a normalized tag-array JSON.
pub unsafe extern "C" fn smk_normalize_metadata_tags(
    values: *const SmkUtf8Slice,
    count: usize,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        json(
            &scriptmetakit::normalize_metadata_tags(utf8_strings_from_raw(values, count)?)
                .map_err(service_error)?,
        )
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Value is a readable UTF-8 comma-delimited tag list. Output is a normalized tag-array JSON.
pub unsafe extern "C" fn smk_parse_metadata_tag_list(
    value: SmkUtf8Slice,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        json(
            &scriptmetakit::normalize_metadata_tags(utf8_slice(value)?.split(','))
                .map_err(service_error)?,
        )
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Value is readable UTF-8. Output is a JSON Boolean.
pub unsafe extern "C" fn smk_validate_metadata_tag(
    value: SmkUtf8Slice,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        json(&scriptmetakit::is_valid_metadata_tag(utf8_slice(value)?))
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Value is readable UTF-8. Empty output means the URL is not accepted by Kit.
pub unsafe extern "C" fn smk_normalize_metadata_url(
    value: SmkUtf8Slice,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        Ok(scriptmetakit::normalize_metadata_url(utf8_slice(value)?)
            .map(|url| url.to_string())
            .unwrap_or_default())
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Output is writable and owns runtime-info JSON from this loaded binary.
pub unsafe extern "C" fn smk_runtime_info_json(output: *mut *mut SmkEditResult) -> SmkStatus {
    service_text(output, || {
        json(&serde_json::json!({
            "api_version": 1, "package_name": scriptmetakit::package_name(),
            "package_version": scriptmetakit::package_version(),
            "architecture": std::env::consts::ARCH, "platform": std::env::consts::OS,
            "native_watch": cfg!(feature = "native-watch"), "http_updates": cfg!(feature = "blocking-http"),
        }))
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Engine is live; scope uses existing cache-scope values. Output is empty on success.
pub unsafe extern "C" fn smk_engine_invalidate_cache(
    engine: *mut SmkEngine,
    scope: u32,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        let mut state = engine_mut(engine)?;
        state.engine.invalidate_cache(
            cache_scope_from_u32(scope)?,
            scriptmetakit::CacheInvalidationReason {
                code: "consumer_requested".into(),
                message: "consumer requested cache invalidation".into(),
            },
        );
        Ok(String::new())
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Engine and path slices are live for this call. Marks changes without executing scripts or scanning.
pub unsafe extern "C" fn smk_engine_notify_changed_paths(
    engine: *mut SmkEngine,
    paths: *const SmkUtf8Slice,
    count: usize,
    overflowed: u8,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        let paths = utf8_path_slices(paths, count)?;
        let mut state = engine_mut(engine)?;
        state
            .engine
            .mark_changed_paths(scriptmetakit::RawChangeBatch {
                paths,
                overflowed: overflowed != 0,
            })
            .map_err(service_error)?;
        Ok(String::new())
    })
}

#[unsafe(no_mangle)]
/// # Safety
/// Engine/output are live. Returned scan results follow existing scan-result ownership.
pub unsafe extern "C" fn smk_engine_refresh_dirty_roots(
    engine: *mut SmkEngine,
    mode: u32,
    output: *mut *mut SmkScanResult,
) -> SmkStatus {
    let mut notification = empty_pending_watch_notification();
    let status = ffi_guard(|| {
        let output = out_mut(output)?;
        *output = ptr::null_mut();
        let mut state = engine_mut(engine)?;
        state.clear_error();
        let result = state
            .engine
            .refresh_dirty_roots(scriptmetakit::RefreshRequest {
                mode: scan_mode_from_u32(mode)?,
            })
            .map_err(|error| {
                state.set_error(&error.to_string());
                service_error(error)
            })?;
        notification = synchronize_running_watch_plan(&mut state).map_err(|error| {
            state.set_error(&error);
            (SmkStatus::EngineError, error)
        })?;
        *output = Box::into_raw(Box::new(SmkScanResult::from_scan_result(result, None)));
        Ok(())
    });
    if status == SmkStatus::Ok {
        emit_pending_watch_notification(notification);
    }
    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

/// Builds an exact-language editing model from a full editing draft, without file I/O.
/// # Safety
/// Input is readable UTF-8 JSON. Free every returned result, including errors.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smk_metadata_localization_read(
    draft: SmkUtf8Slice,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        let draft: KitScriptMetadataDraft = decode(draft)?;
        json(
            &scriptmetakit::MetadataLocalizationEditor::from_draft(&draft)
                .map_err(service_error)?,
        )
    })
}

/// Applies one explicit editing operation. No OS preferences or display fallback are used.
/// # Safety
/// Inputs are readable UTF-8 JSON. Free every returned result, including errors.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smk_metadata_localization_edit(
    editor: SmkUtf8Slice,
    operation: SmkUtf8Slice,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        let mut editor: scriptmetakit::MetadataLocalizationEditor = decode(editor)?;
        editor.edit(decode(operation)?).map_err(service_error)?;
        json(&editor)
    })
}

/// Produces a save draft retaining common fields and Tags. Commit with the conditional file API.
/// # Safety
/// Inputs are readable UTF-8 JSON. Free every returned result, including errors.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smk_metadata_localization_apply(
    editor: SmkUtf8Slice,
    draft: SmkUtf8Slice,
    output: *mut *mut SmkEditResult,
) -> SmkStatus {
    service_text(output, || {
        let editor: scriptmetakit::MetadataLocalizationEditor = decode(editor)?;
        json(&editor.applying_to(&decode(draft)?).map_err(service_error)?)
    })
}
