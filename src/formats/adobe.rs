use crate::core::ScriptRuntimeKind;

use super::{CommentSyntax, ScriptFormatKind, ScriptFormatSpec};

pub(crate) const ADOBE_JAVASCRIPT: ScriptFormatSpec = ScriptFormatSpec {
    kind: ScriptFormatKind::AdobeJavaScript,
    extensions: &["js", "jsx", "jsxinc"],
    runtime_kind: Some(ScriptRuntimeKind::AdobeJavaScript),
    comment_syntax: CommentSyntax::JavaScriptBlock,
    supports_scriptmeta_write: true,
    is_package: false,
};

pub(crate) const ADOBE_JSXBIN: ScriptFormatSpec = ScriptFormatSpec {
    kind: ScriptFormatKind::AdobeJsxbin,
    extensions: &["jsxbin"],
    runtime_kind: Some(ScriptRuntimeKind::AdobeJavaScript),
    comment_syntax: CommentSyntax::JavaScriptBlock,
    supports_scriptmeta_write: false,
    is_package: false,
};

pub(crate) const JSXBIN_SIGNATURE: &[u8] = b"@JSXBIN";
