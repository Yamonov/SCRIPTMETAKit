use crate::core::ScriptRuntimeKind;

use super::{CommentSyntax, ScriptFormatKind, ScriptFormatSpec};

pub(crate) const APPLESCRIPT_TEXT: ScriptFormatSpec = ScriptFormatSpec {
    kind: ScriptFormatKind::AppleScriptText,
    extensions: &["applescript"],
    runtime_kind: Some(ScriptRuntimeKind::AppleScript),
    comment_syntax: CommentSyntax::AppleScriptBlock,
    supports_scriptmeta_write: true,
    is_package: false,
};

pub(crate) const APPLESCRIPT_COMPILED: ScriptFormatSpec = ScriptFormatSpec {
    kind: ScriptFormatKind::AppleScriptCompiled,
    extensions: &["scpt"],
    runtime_kind: Some(ScriptRuntimeKind::AppleScript),
    comment_syntax: CommentSyntax::AppleScriptBlock,
    supports_scriptmeta_write: true,
    is_package: false,
};

pub(crate) const APPLESCRIPT_PACKAGE: ScriptFormatSpec = ScriptFormatSpec {
    kind: ScriptFormatKind::AppleScriptPackage,
    extensions: &["scptd"],
    runtime_kind: Some(ScriptRuntimeKind::AppleScript),
    comment_syntax: CommentSyntax::None,
    supports_scriptmeta_write: false,
    is_package: true,
};

pub(crate) const JXA: ScriptFormatSpec = ScriptFormatSpec {
    kind: ScriptFormatKind::JavaScriptForAutomation,
    extensions: &["jxa"],
    runtime_kind: Some(ScriptRuntimeKind::JavaScriptForAutomation),
    comment_syntax: CommentSyntax::JavaScriptBlock,
    supports_scriptmeta_write: true,
    is_package: false,
};
