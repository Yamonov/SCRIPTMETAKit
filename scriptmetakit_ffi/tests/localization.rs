use scriptmetakit_ffi as ffi;
use std::{fs, ptr, slice};

const SOURCE: &str = "/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.ffi.localization\nMeta-Default-language=de\nName=German\nName[ja]=日本語\nName[en]=English\nName[fr]=Français\nAuthor=Auteur\nTags=画像,image,\nexport,\nTags=image,print\nDescription-BEGIN\n[de]German description\n[fr]French description\nTags=literal\nDescription-END\nSCRIPTMETA-END\n*/\n";

fn input(value: &str) -> ffi::SmkUtf8Slice {
    ffi::SmkUtf8Slice {
        ptr: value.as_ptr(),
        len: value.len(),
    }
}
fn text(value: ffi::SmkUtf8Slice) -> String {
    if value.len == 0 {
        return String::new();
    }
    assert!(!value.ptr.is_null());
    // SAFETY: all callers use slices from a still-live owned FFI result.
    String::from_utf8(unsafe { slice::from_raw_parts(value.ptr, value.len) }.to_vec()).unwrap()
}
struct Engine(*mut ffi::SmkEngine);
impl Engine {
    fn new() -> Self {
        let mut value = ptr::null_mut();
        // SAFETY: value is writable and the returned handle is owned by this guard.
        assert_eq!(
            unsafe { ffi::smk_engine_create_default(&mut value) },
            ffi::SmkStatus::Ok
        );
        Self(value)
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns the live engine.
        unsafe { ffi::smk_engine_free(self.0) }
    }
}
struct Scan(*mut ffi::SmkScanResult);
impl Drop for Scan {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns the scan result.
        unsafe { ffi::smk_scan_result_free(self.0) }
    }
}
struct Edit(*mut ffi::SmkEditResult);
impl Drop for Edit {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns the edit result.
        unsafe { ffi::smk_edit_result_free(self.0) }
    }
}

#[test]
fn ordered_language_and_presentation_slices_cross_the_additive_abi() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("example.jsx"), SOURCE).unwrap();
    let engine = Engine::new();
    let languages = [input("ja"), input("en"), input("fr")];
    let mut changed = 0;
    // SAFETY: inputs and output values remain live throughout the synchronous calls.
    unsafe {
        assert_eq!(
            ffi::smk_engine_set_preferred_languages(
                engine.0,
                languages.as_ptr(),
                languages.len(),
                &mut changed
            ),
            ffi::SmkStatus::Ok
        );
        assert_eq!(changed, 1);
        let equivalent = [input("JA"), input("en"), input("fr")];
        assert_eq!(
            ffi::smk_engine_set_preferred_languages(
                engine.0,
                equivalent.as_ptr(),
                equivalent.len(),
                &mut changed
            ),
            ffi::SmkStatus::Ok
        );
        assert_eq!(changed, 0);
        let path = temp.path().to_str().unwrap();
        let mut result = ptr::null_mut();
        assert_eq!(
            ffi::smk_engine_scan_folder(engine.0, path.as_ptr(), path.len(), &mut result),
            ffi::SmkStatus::Ok
        );
        let result = Scan(result);
        let mut preferences = ffi::SmkUtf8SliceSlice::default();
        assert_eq!(
            ffi::smk_scan_result_preferred_languages(result.0, &mut preferences),
            ffi::SmkStatus::Ok
        );
        assert_eq!(
            slice::from_raw_parts(preferences.ptr, preferences.len)
                .iter()
                .map(|value| text(*value))
                .collect::<Vec<_>>(),
            ["ja", "en", "fr"]
        );
        let mut items = ffi::SmkScriptItemSlice::default();
        assert_eq!(
            ffi::smk_scan_result_items(result.0, &mut items),
            ffi::SmkStatus::Ok
        );
        assert_eq!(items.len, 1);
        assert_eq!(text((*items.ptr).name), "日本語");
        assert_eq!(
            text((*items.ptr).description),
            "French description\nTags=literal"
        );
        let mut tags = ffi::SmkUtf8SliceSlice::default();
        assert_eq!(
            ffi::smk_scan_result_metadata_tags(result.0, &mut tags),
            ffi::SmkStatus::Ok
        );
        let all_tags = slice::from_raw_parts(tags.ptr, tags.len);
        for collection in 0..4 {
            let mut values = ffi::SmkMetadataPresentationSlice::default();
            assert_eq!(
                ffi::smk_scan_result_metadata_presentations(result.0, collection, &mut values),
                ffi::SmkStatus::Ok
            );
            assert_eq!(values.len, 1);
            let value = *values.ptr;
            assert_eq!(text(value.default_language), "de");
            assert_eq!(text(value.name_language), "ja");
            assert_eq!(text(value.description_language), "fr");
            assert_eq!(
                all_tags[value.first_tag_index..value.first_tag_index + value.tag_count]
                    .iter()
                    .map(|value| text(*value))
                    .collect::<Vec<_>>(),
                ["画像", "image", "export", "print"]
            );
        }
        let invalid = [input("jp")];
        assert_eq!(
            ffi::smk_engine_set_preferred_languages(engine.0, invalid.as_ptr(), 1, &mut changed),
            ffi::SmkStatus::InvalidArgument
        );
        assert_eq!(changed, 0);
    }
}

#[test]
fn edit_extensions_roundtrip_without_losing_other_languages() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("example.jsx");
    fs::write(&file, SOURCE).unwrap();
    let path = file.to_str().unwrap();
    let engine = Engine::new();
    // SAFETY: guards retain read slices during the synchronous write call.
    unsafe {
        let mut result = ptr::null_mut();
        assert_eq!(
            ffi::smk_engine_read_script_metadata_draft_file(engine.0, input(path), &mut result),
            ffi::SmkStatus::Ok
        );
        let read_result = Edit(result);
        let mut read = ffi::SmkScriptMetadataEditReadResult::default();
        assert_eq!(
            ffi::smk_edit_result_metadata_edit_read_result(read_result.0, &mut read),
            ffi::SmkStatus::Ok
        );
        let mut extensions = ffi::SmkMetadataEditExtensions::default();
        assert_eq!(
            ffi::smk_edit_result_metadata_extensions(read_result.0, &mut extensions),
            ffi::SmkStatus::Ok
        );
        assert_eq!(extensions.has_extensions, 1);
        assert_eq!(text(extensions.default_language), "de");
        let mut names = slice::from_raw_parts(extensions.names.ptr, extensions.names.len).to_vec();
        let english = names
            .iter_mut()
            .find(|item| text(item.language) == "en")
            .unwrap();
        english.value = input("Updated English");
        names.push(ffi::SmkLocalizedText {
            language: input("it"),
            value: input(""),
        });
        names.push(ffi::SmkLocalizedText {
            language: input("es"),
            value: input("  \t"),
        });
        extensions.names = ffi::SmkLocalizedTextSlice {
            ptr: names.as_ptr(),
            len: names.len(),
        };
        let request = ffi::SmkScriptMetadataWriteRequest {
            file_path: input(path),
            draft: read.draft,
            write_mode: 2,
            ..ffi::SmkScriptMetadataWriteRequest::default()
        };
        let mut written = ptr::null_mut();
        assert_eq!(
            ffi::smk_engine_write_script_metadata_file_if_unchanged_v2(
                engine.0,
                &request,
                &extensions,
                read.source_fingerprint,
                &mut written
            ),
            ffi::SmkStatus::Ok
        );
        let _written = Edit(written);
        let source = fs::read_to_string(&file).unwrap();
        assert!(source.contains("Name[en]=Updated English"));
        assert!(source.contains("Name[it]=\n"));
        assert!(source.contains("Name[es]=  \t\n"));
        assert!(source.contains("Name[fr]=Français"));
        assert!(source.contains("German description"));
        assert!(source.contains("French description"));
        assert!(source.contains("Tags=literal"));
    }
}
