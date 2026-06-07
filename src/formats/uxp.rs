use crate::core::ScriptRuntimeKind;

use super::{CommentSyntax, ScriptFormatKind, ScriptFormatSpec};

pub(crate) const ADOBE_UXP: ScriptFormatSpec = ScriptFormatSpec {
    kind: ScriptFormatKind::AdobeUxp,
    extensions: &["idjs", "psjs"],
    runtime_kind: Some(ScriptRuntimeKind::AdobeUxp),
    comment_syntax: CommentSyntax::JavaScriptBlock,
    supports_scriptmeta_write: true,
    is_package: false,
};
