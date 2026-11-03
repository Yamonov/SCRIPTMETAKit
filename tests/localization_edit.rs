use scriptmetakit::*;
use std::{fs, path::Path};

const SOURCE: &str = "/*\nSCRIPTMETA-BEGIN\nScript-ID=test.editor\nName=日本語名\nAuthor=作者\nMeta-Default-language=ja\nName[en]=English\nAuthor[en]=Author\nTags=image, 画像,\nprint\nX-Custom=keep\nDescription-BEGIN\n  日本語。\n\n[en]English.\nDescription-END\nSCRIPTMETA-END\n*/\nvar untouched = 1;\n";

#[test]
fn last_language_literal_labels_and_backslashes_become_plain_text() {
    for body in [
        "[en]使い方",
        "\n[fr]\n  本物の本文\n\n",
        "\\[en\\]\nC:\\path\\file",
    ] {
        let mut editor = MetadataLocalizationEditor::from_draft(&read(SOURCE)).unwrap();
        editor
            .set_text(
                MetadataLocalizedField::Description,
                Some("ja"),
                Some(body.into()),
            )
            .unwrap();
        let localized = save(SOURCE, &editor);
        let mut reopened = MetadataLocalizationEditor::from_draft(&read(&localized)).unwrap();
        reopened.remove_language("en").unwrap();
        reopened.remove_language("ja").unwrap();
        let plain = save(&localized, &reopened);
        assert!(!plain.contains("Meta-Default-language"));
        assert_eq!(read(&plain).description.as_deref(), Some(body));
        assert_eq!(
            parse_script_metadata(&plain)
                .unwrap()
                .description
                .as_deref(),
            Some(body)
        );
    }
}

#[test]
fn tags_and_language_lists_have_no_ui_language_count_limit() {
    let mut editor = MetadataLocalizationEditor::from_draft(&read(SOURCE)).unwrap();
    for language in [
        "de", "fr", "es", "it", "nl", "sv", "da", "fi", "pl", "pt", "ko", "zh",
    ] {
        editor.add_language(language).unwrap();
    }
    assert_eq!(
        MetadataLocalizationEditor::from_draft(&read(&save(SOURCE, &editor)))
            .unwrap()
            .languages()
            .len(),
        14
    );
}

fn read(source: &str) -> ScriptMetadataDraft {
    read_script_metadata_draft_from_text(source, Path::new("x.jsx"))
        .unwrap()
        .draft
}
fn save(source: &str, editor: &MetadataLocalizationEditor) -> String {
    write_script_metadata_to_text(
        source,
        Path::new("x.jsx"),
        &editor.applying_to(&read(source)).unwrap(),
        ScriptMetaWriteMode::ReplaceOnly,
    )
    .unwrap()
    .text
}

#[test]
fn default_change_moves_actual_values_without_relabeling() {
    let mut editor = MetadataLocalizationEditor::from_draft(&read(SOURCE)).unwrap();
    editor.set_default_language("en").unwrap();
    let text = save(SOURCE, &editor);
    let reread = MetadataLocalizationEditor::from_draft(&read(&text)).unwrap();
    assert_eq!(reread.default_language(), Some("en"));
    for (language, expected) in [("ja", "日本語名"), ("en", "English")] {
        assert_eq!(
            reread
                .text(MetadataLocalizedField::Name, Some(language))
                .unwrap(),
            Some(expected)
        );
    }
    assert_eq!(
        reread
            .text(MetadataLocalizedField::Description, Some("ja"))
            .unwrap(),
        Some("  日本語。\n")
    );
    assert!(text.contains("X-Custom=keep") && text.contains("Tags=image, 画像,\nprint"));
}

#[test]
fn last_language_removal_preserves_content_tags_unknown_fields_and_script() {
    let mut editor = MetadataLocalizationEditor::from_draft(&read(SOURCE)).unwrap();
    editor.remove_language("en").unwrap();
    editor.remove_language("ja").unwrap();
    let text = save(SOURCE, &editor);
    assert!(!text.contains("Meta-Default-language") && !text.contains("Name["));
    let draft = read(&text);
    assert_eq!(draft.name.as_deref(), Some("日本語名"));
    assert_eq!(draft.author.as_deref(), Some("作者"));
    assert_eq!(draft.description.as_deref(), Some("  日本語。\n"));
    assert_eq!(draft.extensions.unwrap().tags, ["image", "画像", "print"]);
    assert!(text.contains("X-Custom=keep") && text.contains("var untouched = 1;"));
}

#[test]
fn tags_alone_do_not_require_name_or_default_language() {
    let source = "SCRIPTMETA-BEGIN\nScript-ID=tags\nTags=image\nSCRIPTMETA-END";
    let draft = read(source);
    let written = write_script_metadata_to_text(
        source,
        Path::new("x.jsx"),
        &draft,
        ScriptMetaWriteMode::ReplaceOnly,
    )
    .unwrap();
    let parsed = parse_script_metadata(&written.text).unwrap();
    assert_eq!(parsed.presentation.tags, ["image"]);
    assert!(parsed.presentation.diagnostics.is_empty());
}

#[test]
fn applying_localization_to_nil_extensions_preserves_original_tags() {
    let mut draft = read(SOURCE);
    let mut editor = MetadataLocalizationEditor::from_draft(&draft).unwrap();
    editor.remove_language("en").unwrap();
    editor.clear_default_language().unwrap();
    draft.extensions = None;
    let result = editor.applying_to(&draft).unwrap();
    assert_eq!(result.extensions.unwrap().tags, ["image", "画像", "print"]);
}

#[test]
fn last_language_retains_whitespace_only_description_and_author() {
    let mut editor = MetadataLocalizationEditor::from_draft(&read(SOURCE)).unwrap();
    editor
        .set_text(
            MetadataLocalizedField::Author,
            Some("ja"),
            Some(" \t ".into()),
        )
        .unwrap();
    editor
        .set_text(
            MetadataLocalizedField::Description,
            Some("ja"),
            Some("\n \t\n".into()),
        )
        .unwrap();
    editor.remove_language("en").unwrap();
    editor.clear_default_language().unwrap();
    let text = save(SOURCE, &editor);
    assert!(text.contains("Author= \t \n"));
    assert_eq!(read(&text).description.as_deref(), Some("\n \t\n"));
}

#[test]
fn description_edit_does_not_add_blank_lines_to_untouched_empty_sections() {
    let source = SOURCE.replace("  日本語。\n\n[en]English.", "[ja]\n[en]English.\n[fr]\n");
    let mut editor = MetadataLocalizationEditor::from_draft(&read(&source)).unwrap();
    editor
        .set_text(
            MetadataLocalizedField::Description,
            Some("en"),
            Some("Changed".into()),
        )
        .unwrap();
    assert_eq!(
        editor
            .applying_to(&read(&source))
            .unwrap()
            .description
            .as_deref(),
        Some("[ja]\n[en]\nChanged\n[fr]\n")
    );
}

#[test]
fn empty_languages_and_whitespace_survive_save_and_reopen() {
    let mut editor = MetadataLocalizationEditor::from_draft(&read(SOURCE)).unwrap();
    editor.add_language("fr").unwrap();
    editor.add_language("de").unwrap();
    editor
        .set_text(
            MetadataLocalizedField::Author,
            Some("de"),
            Some("  \t".into()),
        )
        .unwrap();
    editor
        .set_text(
            MetadataLocalizedField::Description,
            Some("de"),
            Some("\n \t\n".into()),
        )
        .unwrap();
    let text = save(SOURCE, &editor);
    assert!(text.contains("Name[fr]="));
    let reopened = MetadataLocalizationEditor::from_draft(&read(&text)).unwrap();
    assert_eq!(reopened.languages(), ["ja", "de", "en", "fr"]);
    assert_eq!(
        reopened
            .text(MetadataLocalizedField::Author, Some("de"))
            .unwrap(),
        Some("  \t")
    );
    assert_eq!(
        reopened
            .text(MetadataLocalizedField::Description, Some("de"))
            .unwrap(),
        Some("\n \t\n")
    );
    assert_eq!(
        reopened
            .text(MetadataLocalizedField::Name, Some("fr-FR"))
            .unwrap(),
        None
    );
    let parsed = parse_script_metadata_with_options(
        &text,
        &ParserOptions {
            preferred_languages: LanguagePreferences::new(["fr", "de", "en"]).unwrap(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(parsed.name.as_deref(), Some("English"));
}

#[test]
fn description_update_preserves_other_sections_and_literal_labels() {
    let source = SOURCE.replace(
        "[en]English.",
        "  [en]  English.\n\n    indented\n\\[fr\\] literal\n\n",
    );
    let mut editor = MetadataLocalizationEditor::from_draft(&read(&source)).unwrap();
    let other = editor
        .text(MetadataLocalizedField::Description, Some("en"))
        .unwrap()
        .unwrap()
        .to_owned();
    editor
        .set_text(
            MetadataLocalizedField::Description,
            Some("ja"),
            Some("\n[fr]本物の本文\nC:\\temp\\x\n\n".into()),
        )
        .unwrap();
    let written = save(&source, &editor);
    assert!(written.contains("  [en]  English.\n\n    indented\n\\[fr\\] literal\n\n"));
    let reopened = MetadataLocalizationEditor::from_draft(&read(&written)).unwrap();
    assert_eq!(
        reopened
            .text(MetadataLocalizedField::Description, Some("en"))
            .unwrap(),
        Some(other.as_str())
    );
    assert_eq!(
        reopened
            .text(MetadataLocalizedField::Description, Some("ja"))
            .unwrap(),
        Some("\n[fr]本物の本文\nC:\\temp\\x\n\n")
    );
}

#[test]
fn unrelated_edits_do_not_reformat_repeated_description_sections() {
    let source = SOURCE.replace(
        "[en]English.",
        "[en]English.\n[ja]日本語２\n[en]\nOther\n\n",
    );
    let draft = read(&source);
    let mut editor = MetadataLocalizationEditor::from_draft(&draft).unwrap();
    editor
        .set_text(
            MetadataLocalizedField::Name,
            Some("en"),
            Some("New name".into()),
        )
        .unwrap();
    assert_eq!(
        editor.applying_to(&draft).unwrap().description,
        draft.description
    );
}

#[test]
fn repeated_language_update_replaces_only_that_languages_sections() {
    let source = SOURCE.replace("[en]English.", "[en]English.\n[ja]日本語２\n[en]Other\n");
    let mut editor = MetadataLocalizationEditor::from_draft(&read(&source)).unwrap();
    editor
        .set_text(
            MetadataLocalizedField::Description,
            Some("en"),
            Some("Replacement\n\n".into()),
        )
        .unwrap();
    let written = save(&source, &editor);
    assert_eq!(written.matches("[en]").count(), 3); // Name, Author, one Description section.
    assert!(written.contains("[ja]日本語２"));
    let reopened = MetadataLocalizationEditor::from_draft(&read(&written)).unwrap();
    assert_eq!(
        reopened
            .text(MetadataLocalizedField::Description, Some("en"))
            .unwrap(),
        Some("Replacement\n\n")
    );
}

#[test]
fn ordinary_text_has_no_inferred_language_and_first_default_is_explicit() {
    let draft =
        read("SCRIPTMETA-BEGIN\nScript-ID=old\nName=Original\nDescription=Body\nSCRIPTMETA-END");
    let mut editor = MetadataLocalizationEditor::from_draft(&draft).unwrap();
    assert!(editor.languages().is_empty());
    assert_eq!(editor.default_language(), None);
    assert_eq!(
        editor
            .text(MetadataLocalizedField::Name, Some("ja"))
            .unwrap(),
        None
    );
    editor.set_default_language("fr").unwrap();
    assert_eq!(
        editor
            .text(MetadataLocalizedField::Name, Some("fr"))
            .unwrap(),
        Some("Original")
    );
    editor.clear_default_language().unwrap();
    assert_eq!(
        editor.text(MetadataLocalizedField::Name, None).unwrap(),
        Some("Original")
    );
}

#[test]
fn deletion_of_default_does_not_invent_a_replacement_or_allow_invalid_save() {
    let draft = read(SOURCE);
    let mut editor = MetadataLocalizationEditor::from_draft(&draft).unwrap();
    editor.remove_language("ja").unwrap();
    assert_eq!(editor.default_language(), None);
    assert!(editor.applying_to(&draft).is_err());
    editor.set_default_language("en").unwrap();
    assert_eq!(
        editor.applying_to(&draft).unwrap().name.as_deref(),
        Some("English")
    );
}

#[test]
fn failed_operations_preserve_every_value() {
    let draft = read(SOURCE);
    let mut editor = MetadataLocalizationEditor::from_draft(&draft).unwrap();
    let before = serde_json::to_value(&editor).unwrap();
    assert!(editor.clear_default_language().is_err());
    assert!(editor.set_default_language("ja_jp").is_err());
    assert!(
        editor
            .set_text(
                MetadataLocalizedField::Name,
                Some("fr"),
                Some("unused".into())
            )
            .is_err()
    );
    assert_eq!(serde_json::to_value(&editor).unwrap(), before);
}

#[test]
fn missing_default_name_is_not_filled_from_another_language() {
    let draft = read(SOURCE);
    let mut editor = MetadataLocalizationEditor::from_draft(&draft).unwrap();
    editor.set_default_language("fr").unwrap();
    assert_eq!(
        editor
            .text(MetadataLocalizedField::Name, Some("fr"))
            .unwrap(),
        None
    );
    assert!(
        editor
            .applying_to(&draft)
            .unwrap_err()
            .to_string()
            .contains("Name is required")
    );
}

#[test]
fn conditional_save_retains_unknown_fields_and_backups_and_rejects_external_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("x.jsx");
    fs::write(&path, SOURCE.replace('\n', "\r\n")).unwrap();
    let read = read_script_metadata_draft_from_file(&path).unwrap();
    let mut editor = MetadataLocalizationEditor::from_draft(&read.draft).unwrap();
    editor.remove_language("en").unwrap();
    editor.clear_default_language().unwrap();
    let draft = editor.applying_to(&read.draft).unwrap();
    let backup = ScriptMetaBackupOptions {
        root_directory: temp.path().join("backups"),
    };
    let result = write_script_metadata_to_file_if_unchanged(
        &path,
        &draft,
        ScriptMetaWriteMode::ReplaceOnly,
        Some(&backup),
        Some(&read.source_fingerprint),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(&result.backup.unwrap().backup_file_path).unwrap(),
        SOURCE.replace('\n', "\r\n")
    );
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("X-Custom=keep\r\n") && !saved.contains("Meta-Default-language"));
    fs::write(&path, format!("{saved}// external")).unwrap();
    assert!(matches!(
        write_script_metadata_to_file_if_unchanged(
            &path,
            &draft,
            ScriptMetaWriteMode::ReplaceOnly,
            Some(&backup),
            Some(&read.source_fingerprint)
        ),
        Err(ScriptMetaKitError::Conflict(_))
    ));
    assert!(fs::read_to_string(&path).unwrap().ends_with("// external"));
}
