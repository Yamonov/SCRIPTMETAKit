use std::path::Path;

use scriptmetakit::{
    CustomMetadataSelection, LanguagePreferences, ParserOptions, ScriptMetaWriteMode,
    parse_script_metadata_with_options, read_script_metadata_draft_from_text,
    write_script_metadata_to_text,
};

#[test]
fn complete_authoring_examples_are_valid_and_roundtrip_in_every_sample_language() {
    let guide = include_str!("../docs/MetadataAuthoring.md");
    let mut examples = 0;
    for (language, filename) in [
        ("jsx", "example.jsx"),
        ("applescript", "example.applescript"),
    ] {
        let opening = format!("```{language}\n");
        for block in guide.split(&opening).skip(1) {
            let source = block.split("```").next().unwrap();
            let read = read_script_metadata_draft_from_text(source, Path::new(filename)).unwrap();
            let written = write_script_metadata_to_text(
                source,
                Path::new(filename),
                &read.draft,
                ScriptMetaWriteMode::ReplaceOnly,
            )
            .unwrap()
            .text;
            for language in ["ja", "en", "fr"] {
                let options = ParserOptions {
                    preferred_languages: LanguagePreferences::new([language]).unwrap(),
                    custom_metadata_keys: CustomMetadataSelection::new(["X-"]).unwrap(),
                    ..Default::default()
                };
                let before = parse_script_metadata_with_options(source, &options).unwrap();
                let after = parse_script_metadata_with_options(&written, &options).unwrap();
                assert!(
                    before.presentation.diagnostics.is_empty(),
                    "{filename} {language}: {:?}",
                    before.presentation.diagnostics
                );
                assert_eq!(before, after, "{filename} {language}");
            }
            examples += 1;
        }
    }
    assert_eq!(examples, 3);
}
