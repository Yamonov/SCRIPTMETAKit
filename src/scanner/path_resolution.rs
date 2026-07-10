use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    scanner::{ExtensionPolicy, ScannerOptions},
    watcher::normalize_path,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathKind {
    #[default]
    Normal,
    Symlink,
    MacosAlias,
    WindowsShortcut,
}

impl PathKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Symlink => "symlink",
            Self::MacosAlias => "macos_alias",
            Self::WindowsShortcut => "windows_shortcut",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathResolutionStatus {
    #[default]
    NotRequested,
    Resolved,
    Broken,
    Unsupported,
    Cycle,
    PermissionDenied,
}

impl PathResolutionStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotRequested => "not_requested",
            Self::Resolved => "resolved",
            Self::Broken => "broken",
            Self::Unsupported => "unsupported",
            Self::Cycle => "cycle",
            Self::PermissionDenied => "permission_denied",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedPath {
    pub display_path: PathBuf,
    pub source_path: PathBuf,
    pub resolved_path: PathBuf,
    pub path_kind: PathKind,
    pub resolution_status: PathResolutionStatus,
    pub resolution_message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScannablePathResolution {
    pub display_path: PathBuf,
    pub source_path: PathBuf,
    pub resolved_path: PathBuf,
    pub path_kind: PathKind,
    pub resolution_status: PathResolutionStatus,
    pub resolution_message: Option<String>,
}

impl From<ResolvedPath> for ScannablePathResolution {
    fn from(resolved: ResolvedPath) -> Self {
        Self {
            display_path: resolved.display_path,
            source_path: resolved.source_path,
            resolved_path: resolved.resolved_path,
            path_kind: resolved.path_kind,
            resolution_status: resolved.resolution_status,
            resolution_message: resolved.resolution_message,
        }
    }
}

impl ResolvedPath {
    pub(crate) fn with_status(
        mut self,
        resolution_status: PathResolutionStatus,
        resolution_message: impl Into<Option<String>>,
    ) -> Self {
        self.resolution_status = resolution_status;
        self.resolution_message = resolution_message.into();
        self
    }

    pub(crate) fn is_unfollowed_symlink(&self) -> bool {
        self.path_kind == PathKind::Symlink
            && self.resolution_status == PathResolutionStatus::NotRequested
    }
}

pub fn resolve_registered_path(
    path: &Path,
    options: &ScannerOptions,
    extensions: Option<&ExtensionPolicy>,
) -> ScannablePathResolution {
    resolve_scannable_path(path.to_path_buf(), path.to_path_buf(), options, extensions).into()
}

pub(crate) fn resolve_scannable_path(
    display_path: PathBuf,
    source_path: PathBuf,
    options: &ScannerOptions,
    extensions: Option<&ExtensionPolicy>,
) -> ResolvedPath {
    let symlink_metadata = fs::symlink_metadata(&source_path);
    if symlink_metadata
        .as_ref()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return resolve_symlink_path(display_path, source_path, options);
    }

    if options.resolve_macos_alias
        && should_probe_macos_alias_file(&source_path, symlink_metadata.as_ref().ok(), extensions)
        && is_macos_alias_file(&source_path)
    {
        return resolve_macos_alias_path(display_path, source_path);
    }

    let resolved_path = if options.follow_symlinks {
        normalize_path(&source_path)
    } else {
        source_path.clone()
    };
    ResolvedPath {
        display_path,
        source_path,
        resolved_path,
        path_kind: PathKind::Normal,
        resolution_status: PathResolutionStatus::NotRequested,
        resolution_message: None,
    }
}

fn should_probe_macos_alias_file(
    path: &Path,
    metadata: Option<&fs::Metadata>,
    extensions: Option<&ExtensionPolicy>,
) -> bool {
    if !metadata.is_some_and(|metadata| metadata.file_type().is_file()) {
        return false;
    }

    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return true;
    };
    extensions.is_none_or(|extensions| extensions.contains_extension(extension))
}

fn resolve_symlink_path(
    display_path: PathBuf,
    source_path: PathBuf,
    options: &ScannerOptions,
) -> ResolvedPath {
    if !options.follow_symlinks {
        return ResolvedPath {
            display_path,
            resolved_path: source_path.clone(),
            source_path,
            path_kind: PathKind::Symlink,
            resolution_status: PathResolutionStatus::NotRequested,
            resolution_message: None,
        };
    }

    match source_path.canonicalize() {
        Ok(resolved_path) => ResolvedPath {
            display_path,
            source_path,
            resolved_path,
            path_kind: PathKind::Symlink,
            resolution_status: PathResolutionStatus::Resolved,
            resolution_message: None,
        },
        Err(error) => ResolvedPath {
            display_path,
            resolved_path: source_path.clone(),
            source_path,
            path_kind: PathKind::Symlink,
            resolution_status: path_error_status(&error),
            resolution_message: Some(error.to_string()),
        },
    }
}

fn is_macos_alias_file(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut magic = [0_u8; 12];
    let Ok(byte_count) = file.read(&mut magic) else {
        return false;
    };

    byte_count >= magic.len() && &magic[0..4] == b"book" && &magic[8..12] == b"mark"
}

#[cfg(target_os = "macos")]
fn resolve_macos_alias_path(display_path: PathBuf, source_path: PathBuf) -> ResolvedPath {
    use objc2_foundation::{NSURL, NSURLBookmarkResolutionOptions};

    let Some(alias_url) = NSURL::from_file_path(&source_path) else {
        return ResolvedPath {
            display_path,
            resolved_path: source_path.clone(),
            source_path,
            path_kind: PathKind::MacosAlias,
            resolution_status: PathResolutionStatus::Unsupported,
            resolution_message: Some("alias path could not be converted to NSURL".to_string()),
        };
    };

    let options =
        NSURLBookmarkResolutionOptions::WithoutUI | NSURLBookmarkResolutionOptions::WithoutMounting;
    match NSURL::URLByResolvingAliasFileAtURL_options_error(&alias_url, options) {
        Ok(resolved_url) => {
            let Some(resolved_path) = resolved_url.to_file_path() else {
                return ResolvedPath {
                    display_path,
                    resolved_path: source_path.clone(),
                    source_path,
                    path_kind: PathKind::MacosAlias,
                    resolution_status: PathResolutionStatus::Unsupported,
                    resolution_message: Some("resolved alias URL was not a file path".to_string()),
                };
            };
            ResolvedPath {
                display_path,
                source_path,
                resolved_path: normalize_path(&resolved_path),
                path_kind: PathKind::MacosAlias,
                resolution_status: PathResolutionStatus::Resolved,
                resolution_message: None,
            }
        }
        Err(error) => ResolvedPath {
            display_path,
            resolved_path: source_path.clone(),
            source_path,
            path_kind: PathKind::MacosAlias,
            resolution_status: PathResolutionStatus::Broken,
            resolution_message: Some(format!("{error:?}")),
        },
    }
}

#[cfg(not(target_os = "macos"))]
fn resolve_macos_alias_path(display_path: PathBuf, source_path: PathBuf) -> ResolvedPath {
    ResolvedPath {
        display_path,
        resolved_path: source_path.clone(),
        source_path,
        path_kind: PathKind::MacosAlias,
        resolution_status: PathResolutionStatus::Unsupported,
        resolution_message: Some(
            "macOS alias resolution is not available on this platform".to_string(),
        ),
    }
}

pub(crate) fn path_error_status(error: &std::io::Error) -> PathResolutionStatus {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        PathResolutionStatus::PermissionDenied
    } else {
        PathResolutionStatus::Broken
    }
}
