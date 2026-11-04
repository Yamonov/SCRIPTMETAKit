use std::{os::windows::ffi::OsStrExt, path::Path, sync::Mutex};

use windows::{
    Win32::{
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize, IPersistFile,
        },
        UI::Shell::{IShellLinkW, ShellLink},
    },
    core::{Interface, PCWSTR},
};

pub fn create_reference(target: &Path, source: &Path) {
    // Concurrent SetPath calls aborted inside Windows.FileExplorer.Common.dll
    // on the test host. Serialize fixture creation, not Kit reads or watchers.
    static CREATION_LOCK: Mutex<()> = Mutex::new(());
    let _creation = CREATION_LOCK.lock().expect("shortcut fixture lock");

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .expect("COM");
    let _apartment = Apartment;
    let link: IShellLinkW =
        unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.expect("Shell link");
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe { link.SetPath(PCWSTR(target.as_ptr())) }.expect("link target");
    let file: IPersistFile = link.cast().expect("persist interface");
    unsafe { file.Save(PCWSTR(source.as_ptr()), true) }.expect("save link");
}
