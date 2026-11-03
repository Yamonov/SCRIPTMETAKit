use scriptmetakit::{
    CacheScope, RootRegistration, ScanMode, ScanRequest, ScriptMetaKitConfig, ScriptMetaKitEngine,
    core::{ScriptTextEncoding, encode_script_text},
    read_script_metadata_edit_preview_from_file,
};
use std::{fs, path::Path};

const HEADER: &str = "/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.prefix\nName=日本語のスクリプト\nVersion=1.2\nDescription-BEGIN\n文字コードの検証です。\nDescription-END\nSCRIPTMETA-END\n*/\n";

fn engine(root: &Path, prefix_bytes: usize) -> ScriptMetaKitEngine {
    let mut config = ScriptMetaKitConfig::new("PrefixDecodingTests", "test");
    config.parser.max_prefix_bytes = prefix_bytes;
    config.watcher.enabled = false;
    config.update_check.enabled = false;
    let mut engine = ScriptMetaKitEngine::new(config).unwrap();
    engine
        .set_roots(vec![RootRegistration::file_list_and_metadata("root", root)])
        .unwrap();
    engine
}

#[test]
fn default_scan_limit_preserves_metadata_before_split_utf8_characters() {
    let directory = tempfile::tempdir().unwrap();
    let limit = 128 * 1024;
    let mut file_count = 0;
    for character in ['¢', '日', '🦀'] {
        for included_bytes in 1..character.len_utf8() {
            let mut source = HEADER.to_owned();
            source.push_str(&" ".repeat(limit - HEADER.len() - included_bytes));
            source.push(character);
            source.push_str("\n// after the scan limit\n");
            fs::write(
                directory.path().join(format!("case-{file_count}.jsx")),
                source,
            )
            .unwrap();
            file_count += 1;
        }
    }
    let mut engine = engine(directory.path(), limit);
    for _ in 0..2 {
        let result = engine
            .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
            .unwrap();
        let catalog = result.catalog_snapshot.unwrap();
        assert_eq!(catalog.file_items.len(), file_count);
        for item in &catalog.file_items {
            assert_eq!(item.name.as_deref(), Some("日本語のスクリプト"));
            assert_eq!(item.description.as_deref(), Some("文字コードの検証です。"));
            assert!(item.has_scriptmeta && item.can_edit_scriptmeta && !item.can_append_scriptmeta);
        }
    }
}

#[test]
fn scan_and_edit_preview_preserve_each_supported_encoding_at_partial_character_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Example.jsx");
    for encoding in [
        ScriptTextEncoding::Utf8,
        ScriptTextEncoding::Utf8Bom,
        ScriptTextEncoding::Utf16LittleEndian,
        ScriptTextEncoding::Utf16LittleEndianBom,
        ScriptTextEncoding::Utf16BigEndian,
        ScriptTextEncoding::Utf16BigEndianBom,
        ScriptTextEncoding::EucJp,
        ScriptTextEncoding::ShiftJis,
    ] {
        let character = if matches!(
            encoding,
            ScriptTextEncoding::EucJp | ScriptTextEncoding::ShiftJis
        ) {
            "語"
        } else {
            "🦀"
        };
        let prefix_len = encode_script_text(HEADER, encoding).unwrap().len();
        let character_end = encode_script_text(&format!("{HEADER}{character}"), encoding)
            .unwrap()
            .len();
        let source = encode_script_text(
            &format!("{HEADER}{character}\n// remaining text\n"),
            encoding,
        )
        .unwrap();
        fs::write(&path, &source).unwrap();
        for limit in prefix_len + 1..character_end {
            let result = engine(directory.path(), limit)
                .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
                .unwrap();
            let catalog = result.catalog_snapshot.unwrap();
            assert_eq!(catalog.file_items.len(), 1, "{encoding:?}, limit={limit}");
            assert_eq!(
                catalog.file_items[0].name.as_deref(),
                Some("日本語のスクリプト")
            );
            let preview = read_script_metadata_edit_preview_from_file(&path, limit).unwrap();
            assert_eq!(preview.preview_text, HEADER, "{encoding:?}, limit={limit}");
            assert!(preview.is_truncated && preview.has_scriptmeta_marker_in_preview);
            assert_eq!(preview.preview_byte_count, limit);
        }
        assert_eq!(fs::read(&path).unwrap(), source);
    }
}

#[test]
fn metadata_beyond_the_requested_limit_is_not_inferred() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Example.jsx");
    fs::write(&path, HEADER).unwrap();
    let limit = HEADER.find("SCRIPTMETA-END").unwrap() + 5;
    let result = engine(directory.path(), limit)
        .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
        .unwrap();
    assert!(result.catalog_snapshot.unwrap().file_items.is_empty());
    let preview = read_script_metadata_edit_preview_from_file(&path, limit).unwrap();
    assert!(preview.is_truncated && !preview.has_scriptmeta_marker_in_preview);
}

#[test]
fn whole_file_preview_at_the_byte_limit_retains_complete_text() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Example.jsx");
    let source = format!("{HEADER}🦀");
    fs::write(&path, &source).unwrap();
    let preview = read_script_metadata_edit_preview_from_file(&path, source.len()).unwrap();
    assert_eq!(preview.preview_text, source);
    assert!(!preview.is_truncated);
}

#[test]
fn cache_from_before_partial_decoding_fix_requires_source_reload() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("Example.jsx"), HEADER).unwrap();
    let mut writer = engine(directory.path(), 128 * 1024);
    writer
        .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
        .unwrap();
    let current = writer.export_cache(CacheScope::All).unwrap();
    let mut old = current.clone();
    old.schema.metadata_projection_version = 1;
    let mut reader = engine(directory.path(), 128 * 1024);
    assert_eq!(
        reader.load_cache(old).unwrap_err().code(),
        "cache_incompatible"
    );
    reader.load_cache(current).unwrap();
    let result = reader
        .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
        .unwrap();
    assert_eq!(result.catalog_snapshot.unwrap().file_items.len(), 1);
}
