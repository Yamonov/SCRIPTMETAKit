use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

#[cfg(target_os = "macos")]
use std::os::darwin::fs::MetadataExt;

use serde::{Deserialize, Serialize};

use crate::core::{ScriptMetaEditCapability, ScriptMetaEditState, ScriptRuntimeKind};

mod adobe;
mod applescript;
pub(crate) mod compiled_osa;
mod uxp;

#[cfg(target_os = "macos")]
const MACOS_UF_IMMUTABLE: u32 = 0x0000_0002;
#[cfg(target_os = "macos")]
const MACOS_UF_APPEND: u32 = 0x0000_0004;
#[cfg(target_os = "macos")]
const MACOS_SF_IMMUTABLE: u32 = 0x0002_0000;
#[cfg(target_os = "macos")]
const MACOS_SF_APPEND: u32 = 0x0004_0000;
#[cfg(target_os = "macos")]
const MACOS_LOCK_FLAGS: u32 =
    MACOS_UF_IMMUTABLE | MACOS_UF_APPEND | MACOS_SF_IMMUTABLE | MACOS_SF_APPEND;
pub const SHEBANG_PROBE_BYTE_LIMIT: usize = 50;
pub const JSXBIN_PROBE_BYTE_LIMIT: usize = 8;
pub const DEFAULT_SCRIPT_EXTENSIONS: &[&str] = &[
    "js",
    "jsx",
    "jsxbin",
    "jsxinc",
    "scpt",
    "scptd",
    "applescript",
    "jxa",
    "idjs",
    "psjs",
];
pub const DEFAULT_SCRIPT_EXTENSIONS_TEXT: &str =
    "js\njsx\njsxbin\njsxinc\nscpt\nscptd\napplescript\njxa\nidjs\npsjs";

const FORMAT_SPECS: &[ScriptFormatSpec] = &[
    adobe::ADOBE_JAVASCRIPT,
    adobe::ADOBE_JSXBIN,
    applescript::APPLESCRIPT_TEXT,
    applescript::APPLESCRIPT_COMPILED,
    applescript::APPLESCRIPT_PACKAGE,
    applescript::JXA,
    uxp::ADOBE_UXP,
];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptFormatKind {
    AdobeJavaScript,
    AdobeJsxbin,
    AdobeUxp,
    AppleScriptText,
    AppleScriptCompiled,
    AppleScriptPackage,
    JavaScriptForAutomation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentSyntax {
    JavaScriptBlock,
    AppleScriptBlock,
    PlainText,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScriptFormatSpec {
    pub kind: ScriptFormatKind,
    pub extensions: &'static [&'static str],
    pub runtime_kind: Option<ScriptRuntimeKind>,
    pub comment_syntax: CommentSyntax,
    pub supports_scriptmeta_write: bool,
    pub is_package: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptFileInfo {
    pub runtime_kind: Option<ScriptRuntimeKind>,
    pub shebang: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptFileInspection {
    pub is_supported_script_path: bool,
    pub runtime_kind: Option<ScriptRuntimeKind>,
    pub shebang: Option<String>,
    pub comment_syntax: Option<CommentSyntax>,
    pub supports_inline_scriptmeta_editing: bool,
    pub is_file_locked: bool,
    pub is_read_only: bool,
    pub can_edit_scriptmeta: bool,
    pub can_append_scriptmeta: bool,
    pub scriptmeta_edit_state: ScriptMetaEditState,
}

#[must_use]
pub fn all_format_specs() -> &'static [ScriptFormatSpec] {
    FORMAT_SPECS
}

#[must_use]
pub fn supported_script_extensions() -> &'static [&'static str] {
    DEFAULT_SCRIPT_EXTENSIONS
}

#[must_use]
pub fn supported_script_extensions_text() -> &'static str {
    DEFAULT_SCRIPT_EXTENSIONS_TEXT
}

#[must_use]
pub fn format_for_path(
    path: &Path,
    prefix_bytes: Option<&[u8]>,
) -> Option<&'static ScriptFormatSpec> {
    let extension = normalized_extension(path)?;
    if extension.eq_ignore_ascii_case("jxa")
        || ((extension.eq_ignore_ascii_case("js") || extension.eq_ignore_ascii_case("applescript"))
            && has_jxa_osascript_shebang(prefix_bytes))
    {
        return Some(&applescript::JXA);
    }
    FORMAT_SPECS
        .iter()
        .find(|spec| extension_matches_any(extension, spec.extensions))
}

#[must_use]
pub fn format_kind_for_path(path: &Path, prefix_bytes: Option<&[u8]>) -> Option<ScriptFormatKind> {
    format_for_path(path, prefix_bytes).map(|spec| spec.kind)
}

#[must_use]
pub fn detect_script_file(path: &Path, prefix_text: Option<&str>) -> ScriptFileInfo {
    detect_script_file_from_bytes(path, prefix_text.map(str::as_bytes))
}

#[must_use]
pub fn detect_script_file_from_bytes(path: &Path, prefix_bytes: Option<&[u8]>) -> ScriptFileInfo {
    let shebang = prefix_bytes.and_then(extract_shebang);
    let runtime_kind = format_for_path(path, prefix_bytes).and_then(|spec| spec.runtime_kind);
    ScriptFileInfo {
        runtime_kind,
        shebang,
    }
}

#[must_use]
pub fn needs_shebang_probe(path: &Path) -> bool {
    normalized_extension(path).is_some_and(|extension| {
        extension.eq_ignore_ascii_case("js") || extension.eq_ignore_ascii_case("applescript")
    })
}

#[must_use]
pub fn needs_script_header_probe(path: &Path) -> bool {
    needs_shebang_probe(path) || needs_jsxbin_probe(path)
}

#[must_use]
pub fn script_header_probe_byte_limit(path: &Path) -> usize {
    if needs_shebang_probe(path) {
        SHEBANG_PROBE_BYTE_LIMIT
    } else if needs_jsxbin_probe(path) {
        JSXBIN_PROBE_BYTE_LIMIT
    } else {
        0
    }
}

#[must_use]
pub fn has_scriptmeta_tag(prefix_text: &str) -> bool {
    prefix_text.contains("SCRIPTMETA-BEGIN")
}

#[must_use]
pub fn is_script_package_path(path: &Path) -> bool {
    format_for_path(path, None).is_some_and(|spec| spec.is_package)
}

#[must_use]
pub fn is_compiled_osa_path(path: &Path) -> bool {
    format_kind_for_path(path, None) == Some(ScriptFormatKind::AppleScriptCompiled)
}

#[must_use]
pub fn comment_syntax_for_path(path: &Path) -> Option<CommentSyntax> {
    let syntax = format_for_path(path, None)?.comment_syntax;
    (syntax != CommentSyntax::None).then_some(syntax)
}

#[must_use]
pub fn supports_scriptmeta_write(path: &Path) -> bool {
    format_for_path(path, None).is_some_and(|spec| spec.supports_scriptmeta_write)
}

#[must_use]
pub fn inspect_script_file(path: &Path) -> ScriptFileInspection {
    let prefix_bytes = read_script_header_probe(path);
    let prefix_bytes = prefix_bytes.as_deref();
    let format = format_for_path(path, prefix_bytes);
    let info = detect_script_file_from_bytes(path, prefix_bytes);
    let capability = scriptmeta_edit_capability_from_file_list_probe(path, prefix_bytes);
    let supports_inline_scriptmeta_editing = format
        .is_some_and(|spec| spec.supports_scriptmeta_write)
        && capability.scriptmeta_edit_state != ScriptMetaEditState::Obfuscated;

    ScriptFileInspection {
        is_supported_script_path: format.is_some(),
        runtime_kind: info.runtime_kind,
        shebang: info.shebang,
        comment_syntax: format.and_then(|spec| {
            (spec.comment_syntax != CommentSyntax::None).then_some(spec.comment_syntax)
        }),
        supports_inline_scriptmeta_editing,
        is_file_locked: capability.is_file_locked,
        is_read_only: capability.is_read_only,
        can_edit_scriptmeta: capability.can_edit_scriptmeta,
        can_append_scriptmeta: capability.can_append_scriptmeta,
        scriptmeta_edit_state: capability.scriptmeta_edit_state,
    }
}

#[must_use]
pub fn script_path_may_affect_metadata(path: &Path) -> bool {
    if path
        .components()
        .any(|component| component.as_os_str().as_encoded_bytes().starts_with(b"."))
    {
        return false;
    }

    let Some(extension) = normalized_extension(path) else {
        return true;
    };
    DEFAULT_SCRIPT_EXTENSIONS
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

#[must_use]
pub fn scriptmeta_edit_capability_from_file_list_probe(
    path: &Path,
    prefix_bytes: Option<&[u8]>,
) -> ScriptMetaEditCapability {
    scriptmeta_edit_capability(path, prefix_bytes, false, false, false)
}

#[must_use]
pub fn scriptmeta_edit_capability_from_metadata(
    path: &Path,
    prefix_bytes: Option<&[u8]>,
    has_scriptmeta: bool,
    has_scriptmeta_edit_password: bool,
) -> ScriptMetaEditCapability {
    scriptmeta_edit_capability(
        path,
        prefix_bytes,
        true,
        has_scriptmeta,
        has_scriptmeta_edit_password,
    )
}

#[must_use]
pub fn scriptmeta_edit_capability_from_cached_metadata(
    path: &Path,
    previous_state: ScriptMetaEditState,
    has_scriptmeta: bool,
    has_scriptmeta_edit_password: bool,
) -> ScriptMetaEditCapability {
    if previous_state == ScriptMetaEditState::Obfuscated {
        let file_state = file_write_state(path);
        return ScriptMetaEditCapability::from_state(
            ScriptMetaEditState::Obfuscated,
            has_scriptmeta,
            has_scriptmeta_edit_password,
            file_state.is_file_locked,
            file_state.is_read_only,
        );
    }

    scriptmeta_edit_capability(
        path,
        None,
        true,
        has_scriptmeta,
        has_scriptmeta_edit_password,
    )
}

fn scriptmeta_edit_capability(
    path: &Path,
    prefix_bytes: Option<&[u8]>,
    metadata_state_known: bool,
    has_scriptmeta: bool,
    has_scriptmeta_edit_password: bool,
) -> ScriptMetaEditCapability {
    let file_state = file_write_state(path);
    let state = if is_jsxbin_obfuscated_script(path, prefix_bytes) {
        ScriptMetaEditState::Obfuscated
    } else if !supports_scriptmeta_write(path) {
        ScriptMetaEditState::Unsupported
    } else if !metadata_state_known {
        ScriptMetaEditState::Unknown
    } else if file_state.is_read_only {
        ScriptMetaEditState::ReadOnly
    } else if has_scriptmeta {
        ScriptMetaEditState::Editable
    } else {
        ScriptMetaEditState::Appendable
    };

    ScriptMetaEditCapability::from_state(
        state,
        has_scriptmeta,
        has_scriptmeta_edit_password,
        file_state.is_file_locked,
        file_state.is_read_only,
    )
}

fn needs_jsxbin_probe(path: &Path) -> bool {
    normalized_extension(path).is_some_and(|extension| {
        ["js", "jsx", "jsxbin"]
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    })
}

fn is_jsxbin_obfuscated_script(path: &Path, prefix_bytes: Option<&[u8]>) -> bool {
    format_kind_for_path(path, prefix_bytes) == Some(ScriptFormatKind::AdobeJsxbin)
        || (needs_jsxbin_probe(path)
            && prefix_bytes.is_some_and(|bytes| bytes.starts_with(adobe::JSXBIN_SIGNATURE)))
}

fn read_script_header_probe(path: &Path) -> Option<Vec<u8>> {
    let byte_limit = script_header_probe_byte_limit(path);
    if byte_limit == 0 {
        return None;
    }

    let mut file = File::open(path).ok()?;
    let mut bytes = Vec::with_capacity(byte_limit);
    let limit = u64::try_from(byte_limit).ok()?;
    file.by_ref().take(limit).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn has_jxa_osascript_shebang(prefix_bytes: Option<&[u8]>) -> bool {
    let Some(shebang) = prefix_bytes.and_then(extract_shebang) else {
        return false;
    };
    let Some(command_line) = shebang.strip_prefix("#!") else {
        return false;
    };
    let tokens = command_line.split_whitespace().collect::<Vec<_>>();
    let Some((program, args)) = tokens.split_first() else {
        return false;
    };

    if is_osascript_program(program) {
        return osascript_args_request_jxa(args);
    }
    if is_env_program(program)
        && let Some(index) = args.iter().position(|token| is_osascript_program(token))
    {
        return osascript_args_request_jxa(&args[index + 1..]);
    }
    false
}

fn is_osascript_program(token: &str) -> bool {
    token.rsplit('/').next() == Some("osascript")
}

fn is_env_program(token: &str) -> bool {
    token.rsplit('/').next() == Some("env")
}

fn osascript_args_request_jxa(args: &[&str]) -> bool {
    args.windows(2).any(|pair| {
        matches!(pair[0], "-l" | "-lang" | "-language")
            && pair[1].eq_ignore_ascii_case("JavaScript")
    })
}

fn extract_shebang(bytes: &[u8]) -> Option<String> {
    if !bytes.starts_with(b"#!") {
        return None;
    }

    let end = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(bytes.len());
    let line = bytes[..end].strip_suffix(b"\r").unwrap_or(&bytes[..end]);
    let shebang = String::from_utf8_lossy(line).trim().to_string();
    (!shebang.is_empty()).then_some(shebang)
}

fn normalized_extension(path: &Path) -> Option<&str> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.trim().trim_start_matches('.'))
        .filter(|extension| !extension.is_empty())
}

fn extension_matches_any(extension: &str, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

#[derive(Clone, Copy, Debug, Default)]
struct FileWriteState {
    is_file_locked: bool,
    is_read_only: bool,
}

fn file_write_state(path: &Path) -> FileWriteState {
    let Ok(metadata) = fs::metadata(path) else {
        return FileWriteState {
            is_file_locked: false,
            is_read_only: true,
        };
    };
    #[cfg(target_os = "macos")]
    let is_file_locked = metadata.st_flags() & MACOS_LOCK_FLAGS != 0;
    #[cfg(not(target_os = "macos"))]
    let is_file_locked = false;
    let is_read_only =
        is_file_locked || metadata.permissions().readonly() || parent_directory_is_read_only(path);
    FileWriteState {
        is_file_locked,
        is_read_only,
    }
}

fn parent_directory_is_read_only(path: &Path) -> bool {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::metadata(parent)
        .map(|metadata| metadata.permissions().readonly())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::{ScriptMetaEditState, file_write_state, scriptmeta_edit_capability};
    use std::{fs, path::Path};

    #[test]
    fn missing_file_is_reported_read_only() {
        let state = file_write_state(Path::new("/tmp/scriptmetakit-missing-file-for-write-state"));

        assert!(state.is_read_only);
    }

    #[cfg(unix)]
    #[test]
    fn read_only_parent_makes_file_read_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("tempdir");
        let script_path = directory.path().join("script.jsx");
        fs::write(&script_path, "alert('hello');\n").expect("write script");
        let original_permissions = fs::metadata(directory.path())
            .expect("metadata")
            .permissions();
        let mut read_only_permissions = original_permissions.clone();
        read_only_permissions.set_mode(0o555);
        fs::set_permissions(directory.path(), read_only_permissions).expect("set readonly");

        let state = file_write_state(&script_path);

        fs::set_permissions(directory.path(), original_permissions).expect("restore permissions");
        assert!(state.is_read_only);
    }

    #[test]
    fn read_only_file_is_not_editable() {
        let directory = tempfile::tempdir().expect("tempdir");
        let script_path = directory.path().join("script.jsx");
        fs::write(&script_path, "alert('hello');\n").expect("write script");
        let mut permissions = fs::metadata(&script_path).expect("metadata").permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&script_path, permissions).expect("set readonly");

        let capability = scriptmeta_edit_capability(&script_path, None, true, true, false);

        assert_eq!(
            capability.scriptmeta_edit_state,
            ScriptMetaEditState::ReadOnly
        );
        assert!(!capability.can_edit_scriptmeta);
        assert!(!capability.can_append_scriptmeta);
    }
}
