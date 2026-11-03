use scriptmetakit::{
    CacheScope, LanguagePreferences, MetadataEditExtensions, ParserOptions, RootRegistration,
    ScanMode, ScanRequest, ScriptMetaKitConfig, ScriptMetaKitEngine, ScriptMetaWriteMode,
    ScriptMetadataDraft, parse_distribution_metadata_for_script, parse_script_metadata,
    parse_script_metadata_with_options, read_script_metadata_draft_from_text,
    render_script_metadata_block, write_script_metadata_to_text,
};
use std::{fs, path::Path, sync::Arc};

const TEXT: &str = "/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.localized\nMeta-Default-language=de\n\
Name=DE_NAME\nName[en]=EN_NAME\nName[fr]=FR_NAME\nAuthor=DE_AUTHOR\n\
Tags=画像,image,\n#print,\nName[ja]=日本語名\nTags=image,export,\n\
Description-BEGIN\n[de]\nDE_DESCRIPTION\n[fr]\nFR_DESCRIPTION\n\
Tags=literal-example\nVersion=9.9\nDescription-END\nVersion=1.0\nSCRIPTMETA-END\n*/\n";

fn options(languages: &[&str]) -> ParserOptions {
    ParserOptions {
        preferred_languages: LanguagePreferences::new(languages).unwrap(),
        ..ParserOptions::default()
    }
}

#[test]
fn ordered_preferences_are_applied_to_each_field_and_unselected_text_is_not_returned() {
    let result = parse_script_metadata_with_options(TEXT, &options(&["ja", "en", "fr"])).unwrap();
    assert_eq!(result.name.as_deref(), Some("日本語名"));
    assert_eq!(
        result.description.as_deref(),
        Some("FR_DESCRIPTION\nTags=literal-example\nVersion=9.9")
    );
    assert_eq!(result.author.as_deref(), Some("DE_AUTHOR"));
    assert_eq!(result.version.as_deref(), Some("1.0"));
    assert_eq!(
        result.presentation.tags,
        ["画像", "image", "#print", "export"]
    );
    let encoded = serde_json::to_string(&result).unwrap();
    assert!(!encoded.contains("DE_DESCRIPTION"));
    assert!(!encoded.contains("EN_NAME"));
    assert!(!encoded.contains("FR_NAME"));
}

#[test]
fn unconfigured_read_uses_metadata_defaults_without_os_access() {
    let result = parse_script_metadata(TEXT).unwrap();
    assert_eq!(result.name.as_deref(), Some("DE_NAME"));
    assert_eq!(result.description.as_deref(), Some("DE_DESCRIPTION"));
}

#[test]
fn duplicate_locales_join_identical_values_without_deduplication() {
    let text = "SCRIPTMETA-BEGIN\nScript-ID=duplicate\nMeta-Default-language=ja\nName=名前\nName[en]=same\nName[EN]=same\nAuthor[en]=A\nAuthor[en]=A\nDescription-BEGIN\n[en]same\n[EN]same\nDescription-END\nSCRIPTMETA-END";
    let result = parse_script_metadata_with_options(text, &options(&["en"])).unwrap();
    assert_eq!(result.name.as_deref(), Some("same same"));
    assert_eq!(result.author.as_deref(), Some("A A"));
    assert_eq!(result.description.as_deref(), Some("same\n\nsame"));
    assert!(
        result
            .presentation
            .diagnostics
            .iter()
            .any(|issue| issue.code == "repeated_language_joined")
    );
}

#[test]
fn labels_are_scoped_and_escaped_labels_are_literal() {
    let text = TEXT.replace(
        "[fr]\nFR_DESCRIPTION",
        "[fr]\n\\[en\\] literal C:\\temp\\new",
    );
    let result = parse_script_metadata_with_options(&text, &options(&["fr"])).unwrap();
    assert!(
        result
            .description
            .unwrap()
            .starts_with("[en] literal C:\\temp\\new")
    );
    assert_eq!(
        result.presentation.tags,
        ["画像", "image", "#print", "export"]
    );
}

#[test]
fn old_description_without_labels_is_default_text() {
    let text = "SCRIPTMETA-BEGIN\nScript-ID=legacy\nName=Legacy\nDescription-BEGIN\nA \\[label\\] and C:\\temp\\new\nDescription-END\nSCRIPTMETA-END";
    let result = parse_script_metadata_with_options(text, &options(&["ja", "en", "fr"])).unwrap();
    assert_eq!(
        result.description.as_deref(),
        Some("A \\[label\\] and C:\\temp\\new")
    );
}

fn implicit_description_source(default_language: Option<&str>, body: &str) -> String {
    let default = default_language
        .map(|language| format!("Meta-Default-language={language}\n"))
        .unwrap_or_default();
    format!(
        "/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.implicit\n{default}\
         Name=Default name\nVersion=1.0\nDescription-BEGIN\n{body}\n\
         Description-END\nSCRIPTMETA-END\n*/\n"
    )
}

#[test]
fn implicit_default_description_follows_preferences_and_metadata_field_order() {
    let source = implicit_description_source(
        Some("JA"),
        "\n日本語。\n\n次の段落。\n\n[en]EN_DESCRIPTION\n[fr]\nFR_DESCRIPTION",
    );
    let default_after_description = source.replace("Meta-Default-language=JA\n", "").replace(
        "Description-END",
        "Description-END\nMeta-Default-language=JA",
    );
    for source in [&source, &default_after_description] {
        for ending in ["\n", "\r\n", "\r"] {
            let source = source.replace('\n', ending);
            for (preferences, language, expected) in [
                (vec![], "ja", "日本語。\n\n次の段落。"),
                (vec!["ja", "en", "fr"], "ja", "日本語。\n\n次の段落。"),
                (vec!["en"], "en", "EN_DESCRIPTION"),
                (vec!["en-US"], "en", "EN_DESCRIPTION"),
                (vec!["fr", "en"], "fr", "FR_DESCRIPTION"),
                (vec!["es"], "ja", "日本語。\n\n次の段落。"),
            ] {
                let result =
                    parse_script_metadata_with_options(&source, &options(&preferences)).unwrap();
                assert_eq!(result.description.as_deref(), Some(expected));
                assert_eq!(
                    result.presentation.description_language.as_deref(),
                    Some(language)
                );
                assert!(result.presentation.diagnostics.is_empty());
            }
        }
    }
}

#[test]
fn implicit_default_description_requires_a_valid_metadata_default() {
    let body = "Default text\n[en]\nEN_DESCRIPTION";
    for language in [None, Some(""), Some("jp"), Some("ja_jp")] {
        let source = implicit_description_source(language, body);
        let result = parse_script_metadata_with_options(&source, &options(&["en"])).unwrap();
        assert_eq!(result.description.as_deref(), Some(body));
        assert_eq!(result.presentation.description_language, None);
    }
}

#[test]
fn default_description_without_valid_labels_keeps_literal_escapes() {
    let body = "[Note]\n\\[en\\] C:\\\\Test\\\\fixtures\n[jp]普通の本文";
    let source = implicit_description_source(Some("ja"), body);
    let result = parse_script_metadata_with_options(&source, &options(&["en"])).unwrap();
    assert_eq!(result.description.as_deref(), Some(body));
    assert_eq!(
        result.presentation.description_language.as_deref(),
        Some("ja")
    );
}

#[test]
fn implicit_and_explicit_default_sections_join_without_deduplication() {
    let source = implicit_description_source(Some("ja"), "same\n[ja]same\n[JA]same");
    let result = parse_script_metadata(&source).unwrap();
    assert_eq!(result.description.as_deref(), Some("same\n\nsame\n\nsame"));
    assert_eq!(
        result
            .presentation
            .diagnostics
            .iter()
            .filter(|issue| {
                issue.code == "repeated_language_joined" && issue.field == "Description"
            })
            .count(),
        2
    );
}

#[test]
fn implicit_description_escapes_headings_and_field_examples_remain_in_their_section() {
    let source = implicit_description_source(
        Some("ja"),
        r"[Note]
\[en\] is text
C:\\Test\\fixtures
Tags=literal,example
Version=9.9
[en]
EN_DESCRIPTION",
    );
    let result = parse_script_metadata(&source).unwrap();
    assert_eq!(
        result.description.as_deref(),
        Some(
            r"[Note]
[en] is text
C:\Test\fixtures
Tags=literal,example
Version=9.9"
        )
    );
    assert_eq!(result.version.as_deref(), Some("1.0"));
    assert!(result.presentation.tags.is_empty());
}

#[test]
fn empty_implicit_default_does_not_create_an_available_translation() {
    let source = implicit_description_source(Some("ja"), "\n  \n[en]EN_DESCRIPTION");
    let result =
        parse_script_metadata_with_options(&source, &options(&["ja", "en", "fr"])).unwrap();
    assert_eq!(result.description.as_deref(), Some("EN_DESCRIPTION"));
    assert_eq!(
        result.presentation.description_language.as_deref(),
        Some("en")
    );
    assert!(
        parse_script_metadata(&source)
            .unwrap()
            .description
            .is_none()
    );
}

#[test]
fn editing_preserves_implicit_default_and_all_explicit_description_sections() {
    let body = "JA_DESCRIPTION\n[en]\nEN_DESCRIPTION\n[fr]\nFR_DESCRIPTION";
    let source = implicit_description_source(Some("ja"), body);
    let mut read = read_script_metadata_draft_from_text(&source, Path::new("test.jsx")).unwrap();
    assert_eq!(read.draft.description.as_deref(), Some(body));
    read.draft.version = Some("2.0".into());
    let result = write_script_metadata_to_text(
        &source,
        Path::new("test.jsx"),
        &read.draft,
        ScriptMetaWriteMode::ReplaceOnly,
    )
    .unwrap();
    let reread = read_script_metadata_draft_from_text(&result.text, Path::new("test.jsx")).unwrap();
    assert_eq!(reread.draft.description.as_deref(), Some(body));
    assert!(!result.text.contains("[ja]"));
    for (language, expected) in [
        ("ja", "JA_DESCRIPTION"),
        ("en", "EN_DESCRIPTION"),
        ("fr", "FR_DESCRIPTION"),
    ] {
        let parsed =
            parse_script_metadata_with_options(&result.text, &options(&[language])).unwrap();
        assert_eq!(parsed.description.as_deref(), Some(expected));
    }
}

#[test]
fn implicit_default_selection_reloads_and_rejects_previous_projection_cache() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("test.jsx"),
        implicit_description_source(
            Some("ja"),
            "JA_DESCRIPTION\n[en]EN_DESCRIPTION\n[fr]FR_DESCRIPTION",
        ),
    )
    .unwrap();
    let mut config = ScriptMetaKitConfig::default();
    config.watcher.enabled = false;
    config.update_check.enabled = false;
    let mut engine = ScriptMetaKitEngine::new(config).unwrap();
    engine
        .set_roots(vec![RootRegistration::file_list_and_metadata(
            "test",
            directory.path(),
        )])
        .unwrap();
    for (languages, expected) in [
        (vec![], "JA_DESCRIPTION"),
        (vec!["en"], "EN_DESCRIPTION"),
        (vec!["fr"], "FR_DESCRIPTION"),
        (vec!["es"], "JA_DESCRIPTION"),
    ] {
        engine.set_preferred_languages(languages).unwrap();
        let result = engine
            .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
            .unwrap();
        assert_eq!(
            result.catalog_snapshot.as_ref().unwrap().file_items[0]
                .description
                .as_deref(),
            Some(expected)
        );
        let mut cache = engine.export_cache(CacheScope::All).unwrap();
        let encoded = serde_json::to_string(&cache).unwrap();
        for excluded in ["JA_DESCRIPTION", "EN_DESCRIPTION", "FR_DESCRIPTION"]
            .into_iter()
            .filter(|value| *value != expected)
        {
            assert!(!encoded.contains(excluded));
        }
        cache.schema.metadata_projection_version = 2;
        assert_eq!(
            engine.load_cache(cache).unwrap_err().code(),
            "cache_incompatible"
        );
    }
}

#[test]
fn distribution_description_and_tags_cannot_change_update_fields() {
    let text = "SCRIPTMETA-DIST-BEGIN<br>Script-ID=one<br>Version=1.0<br>Tags=image,<br>Name=tag-boundary<br>Description-BEGIN<br>[en]<br>Version=9.9<br>Script-ID=not-a-record<br>Description-END<br>SCRIPTMETA-DIST-END";
    let result = parse_distribution_metadata_for_script(text, "one").unwrap();
    assert_eq!(result.latest_version.as_deref(), Some("1.0"));
}

#[test]
fn language_change_invalidates_memory_and_rejects_old_language_cache() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("test.jsx");
    fs::write(&path, TEXT).unwrap();
    let mut engine = ScriptMetaKitEngine::new(ScriptMetaKitConfig::default()).unwrap();
    engine
        .set_roots(vec![RootRegistration::file_list_and_metadata(
            "root",
            directory.path(),
        )])
        .unwrap();
    engine.set_preferred_languages(["ja", "en", "fr"]).unwrap();
    let before = engine
        .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
        .unwrap();
    assert_eq!(before.preferred_languages.as_slice(), &["ja", "en", "fr"]);
    let cache = engine.export_cache(CacheScope::All).unwrap();
    engine.set_preferred_languages(["en", "fr"]).unwrap();
    assert!(
        engine
            .cached_scan_result(ScanRequest::all(ScanMode::FileListAndMetadata))
            .catalog_snapshot
            .is_none()
    );
    assert!(engine.load_cache(cache).is_err());
    let after = engine
        .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
        .unwrap();
    let item = &after.catalog_snapshot.as_ref().unwrap().file_items[0];
    assert_eq!(item.name.as_deref(), Some("EN_NAME"));
    assert!(!Arc::ptr_eq(
        item,
        &before.catalog_snapshot.as_ref().unwrap().file_items[0]
    ));
}

#[test]
fn invalid_preferences_leave_current_state_unchanged() {
    let mut engine = ScriptMetaKitEngine::new(ScriptMetaKitConfig::default()).unwrap();
    engine.set_preferred_languages(["ja", "en", "fr"]).unwrap();
    assert!(engine.set_preferred_languages(["jp"]).is_err());
    assert_eq!(engine.preferred_languages().as_slice(), &["ja", "en", "fr"]);
}

#[test]
fn editing_name_preserves_all_languages_and_multiple_tags() {
    let mut read = read_script_metadata_draft_from_text(TEXT, Path::new("test.jsx")).unwrap();
    read.draft
        .extensions
        .as_mut()
        .unwrap()
        .names
        .insert("en".into(), "Edited English".into());
    let result = write_script_metadata_to_text(
        TEXT,
        Path::new("test.jsx"),
        &read.draft,
        ScriptMetaWriteMode::ReplaceOnly,
    )
    .unwrap();
    assert!(result.text.contains("Name[fr]=FR_NAME"));
    assert!(result.text.contains("DE_DESCRIPTION"));
    assert!(result.text.contains("FR_DESCRIPTION"));
    assert!(result.text.contains("#print,"));
    assert_eq!(
        parse_script_metadata_with_options(&result.text, &options(&["en"]))
            .unwrap()
            .name
            .as_deref(),
        Some("Edited English")
    );
}

#[test]
fn clearing_tags_removes_all_occurrences_but_keeps_description_examples() {
    let mut read = read_script_metadata_draft_from_text(TEXT, Path::new("test.jsx")).unwrap();
    read.draft.extensions.as_mut().unwrap().tags.clear();
    let written = write_script_metadata_to_text(
        TEXT,
        Path::new("test.jsx"),
        &read.draft,
        ScriptMetaWriteMode::ReplaceOnly,
    )
    .unwrap();
    assert!(
        parse_script_metadata(&written.text)
            .unwrap()
            .presentation
            .tags
            .is_empty()
    );
    assert!(written.text.contains("Tags=literal-example"));
}

#[test]
fn new_fields_require_default_name_and_language_but_legacy_writes_remain_valid() {
    let mut draft = ScriptMetadataDraft {
        script_id: "legacy".into(),
        ..ScriptMetadataDraft::default()
    };
    assert!(render_script_metadata_block(&draft).is_ok());
    draft.extensions = Some(MetadataEditExtensions {
        tags: vec!["image".into()],
        ..MetadataEditExtensions::default()
    });
    assert!(render_script_metadata_block(&draft).is_err());
    draft.name = Some("Default".into());
    assert!(render_script_metadata_block(&draft).is_err());
    draft.extensions.as_mut().unwrap().default_language = Some("ja".into());
    assert!(render_script_metadata_block(&draft).is_ok());
}

#[test]
fn legacy_edit_caller_preserves_extensions_from_the_original() {
    let mut read = read_script_metadata_draft_from_text(TEXT, Path::new("test.jsx")).unwrap();
    read.draft.extensions = None;
    read.draft.version = Some("1.1".into());
    let written = write_script_metadata_to_text(
        TEXT,
        Path::new("test.jsx"),
        &read.draft,
        ScriptMetaWriteMode::ReplaceOnly,
    )
    .unwrap();
    assert!(written.text.contains("Name[en]=EN_NAME"));
    assert!(written.text.contains("Name[fr]=FR_NAME"));
    assert!(written.text.contains("FR_DESCRIPTION"));
}

#[test]
fn malformed_description_is_not_dropped_by_an_unrelated_edit() {
    let malformed = TEXT.replace("Description-END\n", "");
    let read = read_script_metadata_draft_from_text(&malformed, Path::new("test.jsx")).unwrap();
    assert!(
        write_script_metadata_to_text(
            &malformed,
            Path::new("test.jsx"),
            &read.draft,
            ScriptMetaWriteMode::ReplaceOnly
        )
        .is_err()
    );
}

#[test]
fn localized_writer_rejects_line_and_outer_marker_injection() {
    let read = read_script_metadata_draft_from_text(TEXT, Path::new("test.jsx")).unwrap();
    let mut draft = read.draft.clone();
    draft.name = Some("name\nVersion=9.9".into());
    assert!(render_script_metadata_block(&draft).is_err());
    draft = read.draft;
    draft.description = Some("[en]\nexample SCRIPTMETA-END text".into());
    assert!(render_script_metadata_block(&draft).is_err());
}

#[test]
fn one_thousand_items_and_their_cache_keep_only_selected_text() {
    let directory = tempfile::tempdir().unwrap();
    for index in 0..1000 {
        fs::write(directory.path().join(format!("{index:04}.jsx")), format!(
            "/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.thousand.{index}\n\
             Meta-Default-language=de\nName=UNSELECTED_NAME_{index}\nName[en]=English {index}\n\
             Tags=画像,image\nDescription-BEGIN\n[de]UNSELECTED_DESCRIPTION_{index}\n\
             [en]Selected {index}\n[fr]UNSELECTED_FRENCH_{index}\nDescription-END\nSCRIPTMETA-END\n*/"
        )).unwrap();
    }
    let mut engine = ScriptMetaKitEngine::new(ScriptMetaKitConfig::default()).unwrap();
    engine.set_preferred_languages(["ja", "en", "fr"]).unwrap();
    engine
        .set_roots(vec![RootRegistration::file_list_and_metadata(
            "thousand",
            directory.path(),
        )])
        .unwrap();
    let result = engine
        .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
        .unwrap();
    assert_eq!(
        result.catalog_snapshot.as_ref().unwrap().file_items.len(),
        1000
    );
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("UNSELECTED_")
    );
    let cache = engine.export_cache(CacheScope::All).unwrap();
    assert!(
        !serde_json::to_string(&cache)
            .unwrap()
            .contains("UNSELECTED_")
    );
}
