//! Windows Shell links are resolved as filesystem references, never executed.

use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    marker::PhantomData,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    rc::Rc,
};

use windows::{
    Win32::{
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize, IPersistFile, STGM_READ,
        },
        UI::Shell::{IShellLinkW, ShellLink},
    },
    core::{Interface, PCWSTR},
};

use super::path_resolution::{PathResolutionStatus, ResolvedPath, path_error_status};
use crate::watcher::normalize_path;

const MAX_LINK_HOPS: usize = 32;
const MAX_LINK_BYTES: u64 = 1024 * 1024;

pub(super) fn resolve(mut resolved: ResolvedPath) -> ResolvedPath {
    let mut seen = BTreeSet::new();
    for hop in 0..=MAX_LINK_HOPS {
        let current = normalize_path(&resolved.resolved_path);
        if !seen.insert(current.clone()) {
            return resolved.with_status(
                PathResolutionStatus::Cycle,
                Some("Windows shortcut cycle".to_string()),
            );
        }
        if !super::path_resolution::is_windows_shortcut_path(&current) {
            return match fs::metadata(&current) {
                Ok(_) => {
                    resolved.resolved_path = current;
                    resolved.with_status(PathResolutionStatus::Resolved, None)
                }
                Err(error) => {
                    resolved.with_status(path_error_status(&error), Some(error.to_string()))
                }
            };
        }
        if hop == MAX_LINK_HOPS {
            break;
        }
        match read_target(&current) {
            Ok(target) => resolved.resolved_path = target,
            Err((status, message)) => return resolved.with_status(status, Some(message)),
        }
    }
    resolved.with_status(
        PathResolutionStatus::Cycle,
        Some("Windows shortcut chain exceeds 32 links".to_string()),
    )
}

fn read_target(path: &Path) -> Result<PathBuf, (PathResolutionStatus, String)> {
    let metadata = fs::metadata(path).map_err(|e| (path_error_status(&e), e.to_string()))?;
    if !metadata.is_file() || metadata.len() > MAX_LINK_BYTES {
        return Err((
            PathResolutionStatus::Unsupported,
            "Windows shortcut is not a supported link file".to_string(),
        ));
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if wide[..wide.len() - 1].contains(&0) {
        return Err((
            PathResolutionStatus::Unsupported,
            "Windows shortcut path contains a null character".to_string(),
        ));
    }
    let _apartment = ComApartment::enter().map_err(com_error)?;
    // SAFETY: COM is initialized on this thread. Both interfaces are scoped before
    // the apartment guard; all path buffers remain valid for each synchronous call.
    let link: IShellLinkW =
        unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.map_err(com_error)?;
    let persist: IPersistFile = link.cast().map_err(com_error)?;
    unsafe { persist.Load(PCWSTR(wide.as_ptr()), STGM_READ) }.map_err(com_error)?;
    let mut target = vec![0u16; 32768];
    // GetPath reads the stored filesystem target. Do not call Resolve: no UI,
    // target searching, MSI activation, link rewriting, or command arguments.
    unsafe { link.GetPath(&mut target, std::ptr::null_mut(), 0) }.map_err(com_error)?;
    let length = target.iter().position(|c| *c == 0).unwrap_or(target.len());
    if length == 0 || length >= target.len() - 1 {
        return Err((
            PathResolutionStatus::Unsupported,
            "Windows shortcut has no bounded filesystem target".to_string(),
        ));
    }
    let target = PathBuf::from(OsString::from_wide(&target[..length]));
    if !target.is_absolute() {
        return Err((
            PathResolutionStatus::Unsupported,
            "Windows shortcut target is not an absolute filesystem path".to_string(),
        ));
    }
    Ok(target)
}

fn com_error(error: windows::core::Error) -> (PathResolutionStatus, String) {
    let status = match error.code().0 as u32 {
        0x8007_0005 => PathResolutionStatus::PermissionDenied,
        0x8007_0002 | 0x8007_0003 => PathResolutionStatus::Broken,
        _ => PathResolutionStatus::Unsupported,
    };
    (status, error.to_string())
}

struct ComApartment {
    initialized: bool,
    _thread: PhantomData<Rc<()>>,
}
impl ComApartment {
    fn enter() -> windows::core::Result<Self> {
        // SAFETY: balances only successful initialization on the current thread.
        // An existing STA (RPC_E_CHANGED_MODE) may also use the Shell link interface.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.0 as u32 == 0x8001_0106 {
            return Ok(Self {
                initialized: false,
                _thread: PhantomData,
            });
        }
        result.ok()?;
        Ok(Self {
            initialized: true,
            _thread: PhantomData,
        })
    }
}
impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: this non-Send guard remains on the initialization thread.
            unsafe { CoUninitialize() };
        }
    }
}
