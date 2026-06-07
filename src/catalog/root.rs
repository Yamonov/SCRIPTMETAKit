use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::RootId;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RootRegistration {
    pub root_id: RootId,
    pub path: PathBuf,
    pub display_name: Option<String>,
    pub purpose: RootPurpose,
    pub watch_policy: crate::watcher::WatchPolicy,
    pub cache_policy: CachePolicy,
    pub refresh_policy: RefreshPolicy,
    pub priority: RootPriority,
}

impl RootRegistration {
    #[must_use]
    pub fn user_initiated(
        root_id: impl Into<RootId>,
        path: impl Into<PathBuf>,
        purpose: RootPurpose,
    ) -> Self {
        Self {
            root_id: root_id.into(),
            path: path.into(),
            display_name: None,
            purpose,
            watch_policy: crate::watcher::WatchPolicy::AllRegistered,
            cache_policy: CachePolicy::MemoryAndPersistent,
            refresh_policy: RefreshPolicy::OnFileEvent,
            priority: RootPriority::UserInitiated,
        }
    }

    #[must_use]
    pub fn file_list_and_metadata(root_id: impl Into<RootId>, path: impl Into<PathBuf>) -> Self {
        Self::user_initiated(root_id, path, RootPurpose::FileListAndMetadata)
    }

    #[must_use]
    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootPurpose {
    FileList,
    MetadataCatalog,
    UpdateCheck,
    FileListAndMetadata,
}

impl RootPurpose {
    #[must_use]
    pub fn includes_file_list(self) -> bool {
        matches!(self, Self::FileList | Self::FileListAndMetadata)
    }

    #[must_use]
    pub fn includes_metadata(self) -> bool {
        matches!(
            self,
            Self::MetadataCatalog | Self::UpdateCheck | Self::FileListAndMetadata
        )
    }

    #[must_use]
    pub fn from_scan_mode(mode: crate::catalog::ScanMode) -> Self {
        match mode {
            crate::catalog::ScanMode::FileListOnly => Self::FileList,
            crate::catalog::ScanMode::MetadataOnly => Self::MetadataCatalog,
            crate::catalog::ScanMode::FileListAndMetadata => Self::FileListAndMetadata,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CachePolicy {
    Disabled,
    MemoryOnly,
    PersistentCatalogOnly,
    MemoryAndPersistent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshPolicy {
    ManualOnly,
    OnVisible,
    OnFileEvent,
    OnFileEventDeferred,
    Scheduled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootPriority {
    VisibleWhenSelected,
    UserInitiated,
    Background,
}

#[must_use]
pub fn path_based_root_id(path: impl AsRef<Path>) -> RootId {
    let path = path.as_ref();
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
        .into()
}
