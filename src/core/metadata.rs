use std::{path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::RootId;

pub type ScriptMetaItemRef = Arc<ScriptMetaItem>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParserOptions {
    pub max_prefix_bytes: usize,
    pub require_closed_block: bool,
}

impl Default for ParserOptions {
    fn default() -> Self {
        Self {
            max_prefix_bytes: 128 * 1024,
            require_closed_block: true,
        }
    }
}

impl ParserOptions {
    #[must_use]
    pub fn scripta_compatible() -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetadata {
    pub script_id: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub target_app: Option<String>,
    pub min_target_version: Option<String>,
    pub meta_url: Option<Url>,
    pub latest_url: Option<Url>,
    pub latest_version: Option<String>,
    pub latest_page_url: Option<Url>,
    pub name: Option<String>,
    pub author: Option<String>,
    pub release_date: Option<String>,
    pub edit_password_sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptRuntimeKind {
    AppleScript,
    JavaScriptForAutomation,
    AdobeJavaScript,
    AdobeUxp,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptMetaEditState {
    #[default]
    Unknown,
    Unsupported,
    Obfuscated,
    ReadOnly,
    Appendable,
    Editable,
}

impl ScriptMetaEditState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Unsupported => "unsupported",
            Self::Obfuscated => "obfuscated",
            Self::ReadOnly => "read_only",
            Self::Appendable => "appendable",
            Self::Editable => "editable",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetaEditCapability {
    pub has_scriptmeta: bool,
    pub has_scriptmeta_edit_password: bool,
    pub is_file_locked: bool,
    pub is_read_only: bool,
    pub can_edit_scriptmeta: bool,
    pub can_append_scriptmeta: bool,
    pub scriptmeta_edit_state: ScriptMetaEditState,
}

impl ScriptMetaEditCapability {
    #[must_use]
    pub fn from_state(
        state: ScriptMetaEditState,
        has_scriptmeta: bool,
        has_scriptmeta_edit_password: bool,
        is_file_locked: bool,
        is_read_only: bool,
    ) -> Self {
        Self {
            has_scriptmeta,
            has_scriptmeta_edit_password,
            is_file_locked,
            is_read_only,
            can_edit_scriptmeta: state == ScriptMetaEditState::Editable,
            can_append_scriptmeta: state == ScriptMetaEditState::Appendable,
            scriptmeta_edit_state: state,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetaItem {
    pub root_id: RootId,
    pub file_path: PathBuf,
    pub identity_path: PathBuf,
    #[serde(default)]
    pub runtime_kind: Option<ScriptRuntimeKind>,
    #[serde(default)]
    pub shebang: Option<String>,
    pub script_id: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub target_app: Option<String>,
    pub min_target_version: Option<String>,
    pub meta_url: Option<Url>,
    pub name: Option<String>,
    pub author: Option<String>,
    pub release_date: Option<String>,
    pub edit_password_sha256: Option<String>,
    #[serde(default)]
    pub has_scriptmeta: bool,
    #[serde(default)]
    pub has_scriptmeta_edit_password: bool,
    #[serde(default)]
    pub is_file_locked: bool,
    #[serde(default)]
    pub is_read_only: bool,
    #[serde(default)]
    pub can_edit_scriptmeta: bool,
    #[serde(default)]
    pub can_append_scriptmeta: bool,
    #[serde(default)]
    pub scriptmeta_edit_state: ScriptMetaEditState,
}

impl ScriptMetaItem {
    #[must_use]
    pub fn from_metadata(
        root_id: RootId,
        file_path: PathBuf,
        identity_path: PathBuf,
        metadata: ScriptMetadata,
    ) -> Self {
        Self {
            root_id,
            file_path,
            identity_path,
            runtime_kind: None,
            shebang: None,
            script_id: metadata.script_id,
            version: metadata.version,
            description: metadata.description,
            target_app: metadata.target_app,
            min_target_version: metadata.min_target_version,
            meta_url: metadata.meta_url,
            name: metadata.name,
            author: metadata.author,
            release_date: metadata.release_date,
            edit_password_sha256: metadata.edit_password_sha256,
            has_scriptmeta: true,
            has_scriptmeta_edit_password: false,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: false,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: ScriptMetaEditState::Unknown,
        }
    }

    #[must_use]
    pub fn item_id(&self) -> String {
        self.file_path.to_string_lossy().into_owned()
    }

    #[must_use]
    pub fn is_update_checkable(&self) -> bool {
        self.version.is_some() && self.meta_url.is_some()
    }

    #[must_use]
    pub fn with_location(
        &self,
        root_id: RootId,
        file_path: PathBuf,
        identity_path: PathBuf,
    ) -> Self {
        let mut item = self.clone();
        item.root_id = root_id;
        item.file_path = file_path;
        item.identity_path = identity_path;
        item
    }

    #[must_use]
    pub fn with_script_file_info(
        mut self,
        runtime_kind: Option<ScriptRuntimeKind>,
        shebang: Option<String>,
    ) -> Self {
        self.runtime_kind = runtime_kind;
        self.shebang = shebang;
        self
    }

    #[must_use]
    pub fn with_scriptmeta_edit_capability(mut self, capability: ScriptMetaEditCapability) -> Self {
        self.has_scriptmeta = capability.has_scriptmeta;
        self.has_scriptmeta_edit_password = capability.has_scriptmeta_edit_password;
        self.is_file_locked = capability.is_file_locked;
        self.is_read_only = capability.is_read_only;
        self.can_edit_scriptmeta = capability.can_edit_scriptmeta;
        self.can_append_scriptmeta = capability.can_append_scriptmeta;
        self.scriptmeta_edit_state = capability.scriptmeta_edit_state;
        self
    }
}
