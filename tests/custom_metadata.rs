use std::{fs, path::Path};

use scriptmetakit::{
    CacheScope, CustomMetadataSelection, LanguagePreferences, ParserOptions, RootRegistration,
    ScanMode, ScanRequest, ScriptMetaKitConfig, ScriptMetaKitEngine, ScriptMetaWriteMode,
    parse_script_metadata, parse_script_metadata_with_options,
    read_script_metadata_draft_from_text, validate_custom_metadata_field,
    write_script_metadata_to_file, write_script_metadata_to_text,
};

const SOURCE: &str = "/*\nSCRIPTMETA-BEGIN\nScript-ID=example.custom\nMeta-Default-language=ja\nName=日本語\nName[en]=English\nTags=image,\nX-org.iwashi-Group=Production Tools\nX-ORG.IWASHI-GROUP=Production Tools\nX-org.iwashi-Order=0020\nX-org.iwashi-URL=https://example.com/my%20scripts?Name=English&Version=2.0\nX-org.iwashi-Empty=\nX-Custom=legacy\nUnknown-Key=keep\nDescription-BEGIN\n[ja]説明\n[en]Description\nX-org.iwashi-Inside=body only\nDescription-END\nSCRIPTMETA-END\n*/\n";

#[test]
fn host_comment_delimiters_fail_instead_of_rewriting_opaque_custom_values() {
    use scriptmetakit::{
        ScriptMetaCommentStyle, ScriptMetadataDraft, render_script_metadata_for_style,
    };
    for (style, value) in [
        (
            ScriptMetaCommentStyle::JavaScriptBlock,
            "https://example.com/*/image",
        ),
        (
            ScriptMetaCommentStyle::AppleScriptBlock,
            "literal (* opening",
        ),
        (
            ScriptMetaCommentStyle::AppleScriptBlock,
            "literal *) closing",
        ),
    ] {
        let mut draft = ScriptMetadataDraft {
            script_id: "custom.comments".into(),
            ..Default::default()
        };
        draft
            .custom_fields
            .insert("X-org.iwashi-note".into(), vec![value.into()]);
        let error = render_script_metadata_for_style(&draft, style).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("custom_metadata_comment_delimiter")
        );
        assert!(
            render_script_metadata_for_style(&draft, ScriptMetaCommentStyle::PlainText)
                .unwrap()
                .contains(value)
        );
    }
}

fn options(keys: &[&str]) -> ParserOptions {
    ParserOptions {
        custom_metadata_keys: CustomMetadataSelection::new(keys.iter().copied()).unwrap(),
        preferred_languages: LanguagePreferences::new(["ja", "en", "fr"]).unwrap(),
        ..Default::default()
    }
}

#[test]
fn default_reads_discard_custom_values_and_stop_tag_continuation() {
    let value = parse_script_metadata(SOURCE).unwrap();
    assert!(value.presentation.custom_fields.is_empty());
    assert_eq!(value.presentation.tags, ["image"]);
    let encoded = serde_json::to_string(&value).unwrap();
    assert!(!encoded.contains("Production Tools"));
    assert!(!encoded.contains("0020"));
    assert_eq!(value.name.as_deref(), Some("日本語"));
}

#[test]
fn unselected_malformed_values_are_not_retained_inside_diagnostics() {
    let source = SOURCE.replace("X-org.iwashi-Order=0020", "X-org.iwashi-Note:unselected_private_value\nX-org.iwashi-Other:another_private_value Name=Injected");
    let value = parse_script_metadata(&source).unwrap();
    assert_eq!(value.name.as_deref(), Some("日本語"));
    let encoded = serde_json::to_string(&value).unwrap();
    assert!(!encoded.contains("unselected_private_value"));
    assert!(!encoded.contains("another_private_value"));
}

#[test]
fn exact_selection_is_case_insensitive_and_keeps_duplicate_values() {
    let value =
        parse_script_metadata_with_options(SOURCE, &options(&["x-ORG.iwashi-GROUP"])).unwrap();
    assert_eq!(value.presentation.custom_fields.len(), 1);
    assert_eq!(
        value.presentation.custom_fields["X-org.iwashi-group"],
        ["Production Tools", "Production Tools"]
    );
}

#[test]
fn all_selection_keeps_urls_empty_values_and_ignores_description_keys() {
    let value = parse_script_metadata_with_options(SOURCE, &options(&["X-"])).unwrap();
    let fields = value.presentation.custom_fields;
    assert_eq!(fields.len(), 4);
    assert_eq!(fields["X-org.iwashi-empty"], [""]);
    assert_eq!(fields["X-org.iwashi-order"], ["0020"]);
    assert_eq!(
        fields["X-org.iwashi-url"],
        ["https://example.com/my%20scripts?Name=English&Version=2.0"]
    );
    assert!(!fields.contains_key("X-org.iwashi-inside"));
    assert!(
        value
            .presentation
            .diagnostics
            .iter()
            .any(|issue| issue.field == "X-Custom" && issue.line.is_some())
    );
}

#[test]
fn unselected_malformed_fields_cannot_override_standard_metadata() {
    let source = SOURCE.replace("X-org.iwashi-Order=0020", "X-org.iwashi-Note=Note Name=Injected\nX-org..iwashi-Order=bad Version=invalid\nX-org.iwashi-Space= Name=Injected");
    let value = parse_script_metadata_with_options(&source, &options(&["X-"])).unwrap();
    assert_eq!(value.name.as_deref(), Some("日本語"));
    assert!(value.version.is_none());
    assert!(
        !value
            .presentation
            .custom_fields
            .contains_key("X-org.iwashi-note")
    );
    assert!(
        value
            .presentation
            .diagnostics
            .iter()
            .any(|issue| issue.code == "custom_metadata_legacy_field")
    );
}

#[test]
fn dotted_custom_keys_and_plain_x_tags_respect_line_comments() {
    for prefix in ["// ", "-- ", "# ", "* "] {
        let body = [
            "Script-ID=comments",
            "Tags=image,",
            "x-tools,",
            "X-org.iwashi-Group=Production",
            "Name=Actual",
        ]
        .map(|line| format!("{prefix}{line}"))
        .join("\n");
        let source = format!("SCRIPTMETA-BEGIN\n{body}\nSCRIPTMETA-END");
        let value = parse_script_metadata_with_options(&source, &options(&["X-"])).unwrap();
        assert_eq!(value.presentation.tags, ["image", "x-tools"]);
        assert_eq!(
            value.presentation.custom_fields["X-org.iwashi-group"],
            ["Production"]
        );
    }
}

#[test]
fn validators_accept_literal_punctuation_and_reject_legacy_field_patterns() {
    for value in [
        "",
        "Production Tools",
        "0020",
        "Name=Literal",
        "https://example.com/?Name=A&Version=2",
        r#""a,b" C:\tmp\file"#,
    ] {
        assert!(
            validate_custom_metadata_field("X-org.iwashi-Note", value).is_empty(),
            "{value}"
        );
    }
    for value in [
        "a\nb",
        "a\rb",
        "Note Name=A",
        "Note\tVersion : invalid",
        "\"Note Name=A\"",
        "'Note Name=A'",
        "https://example.com/scriptmeta-end",
        "description-BEGIN",
    ] {
        assert!(
            !validate_custom_metadata_field("X-org.iwashi-Note", value).is_empty(),
            "{value}"
        );
    }
    for key in [
        "X-",
        "X-Name",
        "X-org..iwashi-Name",
        "X-org-iwashi.Name",
        "X-.org-Name",
        "X-org.-Name",
        "X-org-Name-",
        "X-org-Name:",
        "X-org-Description-END",
        "X-作成者-Name",
    ] {
        assert!(
            !validate_custom_metadata_field(key, "value").is_empty(),
            "{key}"
        );
    }
}

#[test]
fn selection_normalization_and_deserialization_validate_every_key() {
    let a = CustomMetadataSelection::new([
        "X-org.iwashi-Order",
        "x-org.iwashi-GROUP",
        "X-org.iwashi-ORDER",
    ])
    .unwrap();
    let b: CustomMetadataSelection =
        serde_json::from_str(r#"["X-org.iwashi-group", "X-org.iwashi-order"]"#).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        CustomMetadataSelection::new(["X-", "X-org.iwashi-Group"])
            .unwrap()
            .keys(),
        ["X-"]
    );
    assert!(CustomMetadataSelection::new(["X-", "X-*"]).is_err());
    assert!(serde_json::from_str::<CustomMetadataSelection>(r#"["X-org..iwashi-Group"]"#).is_err());
}

#[test]
fn ordinary_edits_preserve_all_custom_unknown_and_localized_data() {
    let source = SOURCE.replace(
        "X-org.iwashi-Order=0020",
        "X-org.iwashi-Note=Note Name=Unsafe",
    );
    let mut read = read_script_metadata_draft_from_text(&source, Path::new("test.jsx")).unwrap();
    assert_eq!(
        read.draft.custom_fields["X-org.iwashi-note"],
        ["Note Name=Unsafe"]
    );
    read.draft.name = Some("変更".into());
    let written = write_script_metadata_to_text(
        &source,
        Path::new("test.jsx"),
        &read.draft,
        ScriptMetaWriteMode::ReplaceOnly,
    )
    .unwrap()
    .text;
    for line in [
        "X-org.iwashi-Note=Note Name=Unsafe",
        "X-ORG.IWASHI-GROUP=Production Tools",
        "X-Custom=legacy",
        "Unknown-Key=keep",
        "Name[en]=English",
        "Tags=image,",
    ] {
        assert!(written.contains(line), "{line}");
    }
    assert_eq!(
        parse_script_metadata(&written).unwrap().name.as_deref(),
        Some("変更")
    );
}

#[test]
fn custom_updates_are_key_scoped_and_distinguish_empty_from_removal() {
    let mut read = read_script_metadata_draft_from_text(SOURCE, Path::new("test.jsx")).unwrap();
    read.draft.custom_fields.clear();
    read.draft.custom_fields.insert(
        "x-org.iwashi-GROUP".into(),
        vec![" Updated ".into(), "Updated".into()],
    );
    read.draft
        .custom_fields
        .insert("X-org.iwashi-Order".into(), Vec::new());
    read.draft
        .custom_fields
        .insert("X-org.iwashi-New".into(), vec![String::new()]);
    let written = write_script_metadata_to_text(
        SOURCE,
        Path::new("test.jsx"),
        &read.draft,
        ScriptMetaWriteMode::ReplaceOnly,
    )
    .unwrap()
    .text;
    let fields = parse_script_metadata_with_options(&written, &options(&["X-"]))
        .unwrap()
        .presentation
        .custom_fields;
    assert_eq!(fields["X-org.iwashi-group"], ["Updated", "Updated"]);
    assert_eq!(fields["X-org.iwashi-new"], [""]);
    assert!(!fields.contains_key("X-org.iwashi-order"));
    assert!(written.contains("X-org.iwashi-URL="));
    assert!(written.contains("X-Custom=legacy"));
}

#[test]
fn editing_or_removing_a_custom_boundary_never_turns_unknown_text_into_tags() {
    let source = "/*\nSCRIPTMETA-BEGIN\nScript-ID=custom.boundary\nName=Before\nTags=image,\nX-org.iwashi-Group=Production\nopaque text\nDescription-BEGIN\nBody\nDescription-END\nSCRIPTMETA-END\n*/";
    let mut draft = read_script_metadata_draft_from_text(source, Path::new("test.jsx"))
        .unwrap()
        .draft;
    draft.name = Some("After".into());
    for remove in [false, true] {
        if remove {
            draft
                .custom_fields
                .insert("X-org.iwashi-group".into(), Vec::new());
        }
        let written = write_script_metadata_to_text(
            source,
            Path::new("test.jsx"),
            &draft,
            ScriptMetaWriteMode::ReplaceOnly,
        )
        .unwrap()
        .text;
        assert_eq!(
            parse_script_metadata(&written).unwrap().presentation.tags,
            ["image"]
        );
        assert!(written.lines().any(|line| line == "opaque text"));
    }
}

#[test]
fn unsafe_custom_updates_leave_the_source_file_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("test.jsx");
    fs::write(&path, SOURCE).unwrap();
    let mut draft = read_script_metadata_draft_from_text(SOURCE, &path)
        .unwrap()
        .draft;
    draft.custom_fields.insert(
        "X-org.iwashi-note".into(),
        vec!["Note Name=Injected".into()],
    );
    assert!(
        write_script_metadata_to_file(&path, &draft, ScriptMetaWriteMode::ReplaceOnly, None)
            .is_err()
    );
    assert_eq!(fs::read_to_string(path).unwrap(), SOURCE);
}

#[test]
fn embedded_outer_ending_markers_fail_instead_of_truncating_metadata() {
    let source = SOURCE.replace(
        "X-org.iwashi-Order=0020",
        "X-org.iwashi-URL=https://example.com/SCRIPTMETA-END",
    );
    assert!(parse_script_metadata(&source).is_err());
    assert!(read_script_metadata_draft_from_text(&source, Path::new("test.jsx")).is_err());
    let draft = read_script_metadata_draft_from_text(SOURCE, Path::new("test.jsx"))
        .unwrap()
        .draft;
    assert!(
        write_script_metadata_to_text(
            &source,
            Path::new("test.jsx"),
            &draft,
            ScriptMetaWriteMode::ReplaceOnly
        )
        .is_err()
    );
}

#[test]
fn expanding_selection_reloads_and_narrowing_projects_the_cached_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("test.jsx");
    fs::write(&path, SOURCE).unwrap();
    let mut engine = ScriptMetaKitEngine::new(ScriptMetaKitConfig::default()).unwrap();
    engine
        .set_roots(vec![RootRegistration::file_list_and_metadata(
            "root",
            directory.path(),
        )])
        .unwrap();
    engine.set_custom_metadata_keys(["X-"]).unwrap();
    engine
        .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
        .unwrap();
    let all_cache = engine.export_cache(CacheScope::All).unwrap();
    fs::remove_file(&path).unwrap();
    let change = engine
        .set_custom_metadata_keys(["X-org.iwashi-Group"])
        .unwrap();
    assert!(!change.refresh_required);
    let cached = engine.cached_scan_result(ScanRequest::all(ScanMode::FileListAndMetadata));
    let fields = &cached.catalog_snapshot.as_ref().unwrap().file_items[0]
        .presentation
        .custom_fields;
    assert_eq!(fields.len(), 1);
    assert!(engine.load_cache(all_cache).is_err());
    let cache = serde_json::to_string(&engine.export_cache(CacheScope::All).unwrap()).unwrap();
    assert!(!cache.contains("0020"));
    assert!(
        !engine
            .set_custom_metadata_keys(["x-ORG.IWASHI-group"])
            .unwrap()
            .changed
    );
    fs::write(&path, SOURCE.replace("0020", "0030")).unwrap();
    assert!(
        engine
            .set_custom_metadata_keys(["X-"])
            .unwrap()
            .refresh_required
    );
    let fresh = engine
        .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
        .unwrap();
    assert_eq!(
        fresh.catalog_snapshot.unwrap().file_items[0]
            .presentation
            .custom_fields["X-org.iwashi-order"],
        ["0030"]
    );
}

#[test]
fn invalid_selection_does_not_change_a_live_engine() {
    let mut engine = ScriptMetaKitEngine::new(ScriptMetaKitConfig::default()).unwrap();
    engine
        .set_custom_metadata_keys(["X-org.iwashi-Group"])
        .unwrap();
    let before = engine.config().clone();
    assert!(
        engine
            .set_custom_metadata_keys(["X-", "X-invalid"])
            .is_err()
    );
    assert_eq!(engine.config(), &before);
}
