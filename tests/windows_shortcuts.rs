#![cfg(windows)]
#![allow(unsafe_code)] // Test fixtures use the native Shell link writer.

use scriptmetakit::{
    ExtensionPolicy, FileSystemEntry, PathKind, PathResolutionStatus, RefreshRequest, RootId,
    RootRegistration, RootStatus, ScanMode, ScanRequest, ScannerOptions, ScriptMetaKitConfig,
    ScriptMetaKitEngine, resolve_registered_path,
    scanner::scan_file_list_root,
    watcher::{NativeWatcher, WatchPolicy},
};
use std::{
    fs,
    os::windows::ffi::OsStrExt,
    path::Path,
    thread,
    time::{Duration, Instant},
};
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

fn link(target: &Path, path: &Path) {
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
    let shell: IShellLinkW =
        unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.expect("ShellLink");
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe { shell.SetPath(PCWSTR(target.as_ptr())) }.expect("target");
    let persist: IPersistFile = shell.cast().expect("IPersistFile");
    unsafe { persist.Save(PCWSTR(path.as_ptr()), true) }.expect("save link");
}
fn find<'a>(entries: &'a [FileSystemEntry], display: &Path) -> Option<&'a FileSystemEntry> {
    entries.iter().find_map(|entry| {
        if entry.display_path == display {
            Some(entry)
        } else {
            find(&entry.children, display)
        }
    })
}
fn engine(root: &Path) -> ScriptMetaKitEngine {
    let mut config = ScriptMetaKitConfig::new("Tests", "WindowsReferences");
    config.watcher.watch_policy = WatchPolicy::AllRegistered;
    let mut engine = ScriptMetaKitEngine::new(config).expect("engine");
    engine
        .set_roots(vec![RootRegistration::file_list_and_metadata("root", root)])
        .expect("roots");
    engine
        .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
        .expect("scan");
    engine
}
fn process_change(engine: &mut ScriptMetaKitEngine, watcher: &NativeWatcher) {
    let start = Instant::now();
    loop {
        if let Some(batch) = watcher.try_recv() {
            engine.mark_changed_paths(batch).expect("route change");
            engine
                .refresh_dirty_roots(RefreshRequest {
                    mode: ScanMode::FileListAndMetadata,
                })
                .expect("refresh");
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(8),
            "native notification timeout"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn real_and_referenced_parent_folders_keep_real_and_referenced_children() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("実体 folder");
    let outside = temp.path().join("external");
    fs::create_dir_all(&real).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(real.join("real.jsx"), "'real';").unwrap();
    fs::write(outside.join("target.jsx"), "'target';").unwrap();
    link(
        &outside.join("target.jsx"),
        &real.join("referenced.jsx.lnk"),
    );
    link(&outside, &real.join("referenced-folder.lnk"));
    let parent_link = temp.path().join("parent.lnk");
    link(&real, &parent_link);
    for root in [&real, &parent_link] {
        let scanned = scan_file_list_root(
            &RootId::from("root"),
            root,
            &ScannerOptions::default(),
            &ExtensionPolicy::default(),
        );
        assert_eq!(scanned.root.status, RootStatus::Ready, "{root:?}");
        for (display, target) in [
            (root.join("real.jsx"), real.join("real.jsx")),
            (root.join("referenced.jsx.lnk"), outside.join("target.jsx")),
            (
                root.join("referenced-folder.lnk/target.jsx"),
                outside.join("target.jsx"),
            ),
        ] {
            let item =
                find(&scanned.children, &display).unwrap_or_else(|| panic!("missing {display:?}"));
            assert_eq!(item.resolved_path, target.canonicalize().unwrap());
        }
    }
}

#[test]
fn broken_file_reference_recovers_without_copying_the_target() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("later.jsx");
    let source = temp.path().join("later.lnk");
    link(&target, &source);
    let missing = resolve_registered_path(&source, &ScannerOptions::default(), None);
    assert_eq!(missing.resolution_status, PathResolutionStatus::Broken);
    fs::write(&target, "'returned';").unwrap();
    let returned = resolve_registered_path(&source, &ScannerOptions::default(), None);
    assert_eq!(
        (returned.path_kind, returned.resolution_status),
        (PathKind::WindowsShortcut, PathResolutionStatus::Resolved)
    );
}

#[test]
fn directory_reference_cycles_are_reported_without_recursing_forever() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    fs::create_dir_all(&root).unwrap();
    link(&root, &root.join("loop.lnk"));
    let scanned = scan_file_list_root(
        &RootId::from("root"),
        &root,
        &ScannerOptions::default(),
        &ExtensionPolicy::default(),
    );
    assert_eq!(
        find(&scanned.children, &root.join("loop.lnk"))
            .unwrap()
            .resolution_status,
        PathResolutionStatus::Cycle
    );
}

#[test]
fn shortcut_chains_accept_32_links_and_reject_33() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target.jsx");
    fs::write(&target, "'target';").expect("target");
    let mut previous = target.clone();
    for hop in 1..=33 {
        let source = temp.path().join(format!("{hop}.lnk"));
        link(&previous, &source);
        if hop >= 32 {
            let resolution = resolve_registered_path(&source, &ScannerOptions::default(), None);
            assert_eq!(
                resolution.resolution_status,
                if hop == 32 {
                    PathResolutionStatus::Resolved
                } else {
                    PathResolutionStatus::Cycle
                }
            );
        }
        previous = source;
    }
}

#[test]
fn legacy_scanner_options_resolve_windows_shortcuts_without_a_new_setting() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("a.jsx");
    let source = temp.path().join("a.lnk");
    fs::write(&file, "'a';").unwrap();
    link(&file, &source);
    let legacy = serde_json::to_value(ScannerOptions::default()).unwrap();
    assert!(legacy.get("resolve_windows_shortcuts").is_none());
    let options: ScannerOptions = serde_json::from_value(legacy).unwrap();
    assert_eq!(
        resolve_registered_path(&source, &options, None).resolution_status,
        PathResolutionStatus::Resolved
    );
}

#[test]
fn external_file_target_changes_are_delivered_through_the_native_watcher() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let target = outside.join("one.jsx");
    fs::write(&target, "'old';").unwrap();
    link(&target, &root.join("one.jsx.lnk"));
    let mut engine = engine(&root);
    let watcher = NativeWatcher::start(&engine.watch_plan()).expect("watcher");
    let body = "'new contents from the original file';";
    fs::write(&target, body).unwrap();
    process_change(&mut engine, &watcher);
    let snapshot = engine.snapshot(&RootId::from("root")).unwrap();
    assert_eq!(
        find(
            snapshot.children.as_deref().unwrap(),
            &root.join("one.jsx.lnk")
        )
        .unwrap()
        .file_size,
        Some(body.len() as u64)
    );
}

#[test]
fn registered_folder_shortcut_watches_its_target_and_retargeting() {
    let temp = tempfile::tempdir().unwrap();
    let one = temp.path().join("one");
    let two = temp.path().join("two");
    fs::create_dir_all(&one).unwrap();
    fs::create_dir_all(&two).unwrap();
    fs::write(one.join("one.jsx"), "'one';").unwrap();
    fs::write(two.join("two.jsx"), "'two';").unwrap();
    let root = temp.path().join("root.lnk");
    link(&one, &root);
    let mut engine = engine(&root);
    let watcher =
        NativeWatcher::start(&engine.watch_plan()).expect("watch file root through its directory");
    link(&two, &root);
    process_change(&mut engine, &watcher);
    let snapshot = engine.snapshot(&RootId::from("root")).unwrap();
    assert!(find(snapshot.children.as_deref().unwrap(), &root.join("two.jsx")).is_some());
    drop(watcher);
    let watcher = NativeWatcher::start(&engine.watch_plan()).expect("retargeted watcher");
    fs::write(two.join("added.jsx"), "'added';").unwrap();
    process_change(&mut engine, &watcher);
    assert!(
        find(
            engine
                .snapshot(&RootId::from("root"))
                .unwrap()
                .children
                .as_deref()
                .unwrap(),
            &root.join("added.jsx")
        )
        .is_some()
    );
}

#[test]
fn newly_added_broken_link_registers_a_watch_for_target_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let mut engine = engine(&root);
    let watcher = NativeWatcher::start(&engine.watch_plan()).unwrap();
    let target = outside.join("later.jsx");
    let reference = root.join("later.jsx.lnk");
    link(&target, &reference);
    process_change(&mut engine, &watcher);
    drop(watcher);
    let watcher = NativeWatcher::start(&engine.watch_plan()).unwrap();
    fs::write(target, "'returned';").unwrap();
    process_change(&mut engine, &watcher);
    assert_eq!(
        find(
            engine
                .snapshot(&RootId::from("root"))
                .unwrap()
                .children
                .as_deref()
                .unwrap(),
            &reference
        )
        .unwrap()
        .resolution_status,
        PathResolutionStatus::Resolved
    );
}

#[test]
fn an_initially_missing_physical_watch_does_not_block_other_roots_and_recovers() {
    let temp = tempfile::tempdir().unwrap();
    let working = temp.path().join("working");
    let missing = temp.path().join("missing");
    fs::create_dir_all(&working).unwrap();
    let mut plan = scriptmetakit::watcher::WatchPlan::empty();
    plan.physical_roots = vec![
        scriptmetakit::watcher::PhysicalWatchRoot {
            path: working.clone(),
            covers_root_ids: vec![RootId::from("working")],
        },
        scriptmetakit::watcher::PhysicalWatchRoot {
            path: missing.clone(),
            covers_root_ids: vec![RootId::from("missing")],
        },
    ];
    plan.debounce_delay_millis = 0;
    let watcher = NativeWatcher::start(&plan).expect("keep available watches");
    let start = Instant::now();
    // This case tests independence from a missing root. Native watch workers
    // start asynchronously, so wait for a notification from the working root.
    while watcher.try_recv().is_none() {
        assert!(start.elapsed() < Duration::from_secs(5));
        fs::write(working.join("visible.jsx"), "'visible';").unwrap();
        thread::sleep(Duration::from_millis(20));
    }
    fs::create_dir_all(&missing).unwrap();
    fs::write(missing.join("returned.jsx"), "'returned';").unwrap();
    let start = Instant::now();
    loop {
        if let Some(batch) = watcher.try_recv()
            && batch.overflowed
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "missing root did not reconcile after reopening"
        );
        thread::sleep(Duration::from_millis(20));
    }
}
