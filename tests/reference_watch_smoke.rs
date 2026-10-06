#![cfg(any(target_os = "macos", windows))]
#![allow(unsafe_code)] // Windows fixtures write Shell links through COM.

use std::{fs, path::Path};

use scriptmetakit::{
    FileSystemEntry, RefreshRequest, RootId, RootRegistration, ScanMode, ScanRequest,
    ScriptMetaKitConfig, ScriptMetaKitEngine, ScriptMetaKitEvent,
    watcher::{RawChangeBatch, WatchPolicy},
};

#[cfg(target_os = "macos")]
fn create_reference(target: &Path, source: &Path) {
    use objc2_foundation::{NSURL, NSURLBookmarkCreationOptions};
    let target = if target.is_dir() {
        NSURL::from_directory_path(target)
    } else {
        NSURL::from_file_path(target)
    }
    .expect("target URL");
    let source = NSURL::from_file_path(source).expect("source URL");
    let data = target
        .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            NSURLBookmarkCreationOptions::SuitableForBookmarkFile,
            None,
            None,
        )
        .expect("bookmark data");
    NSURL::writeBookmarkData_toURL_options_error(&data, &source, 0).expect("write alias");
}

#[cfg(windows)]
fn create_reference(target: &Path, source: &Path) {
    use std::os::windows::ffi::OsStrExt;
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

fn reference_name() -> &'static str {
    if cfg!(windows) {
        "Reference.lnk"
    } else {
        "Reference"
    }
}

fn write_script(path: &Path, version: &str) {
    fs::write(path, format!(
        "/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.reference\nVersion={version}\nSCRIPTMETA-END\n*/\n"
    )).expect("write script");
}

fn engine(root: &Path) -> ScriptMetaKitEngine {
    let mut config = ScriptMetaKitConfig::new("Tests", "ReferenceWatch");
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

fn entry(engine: &ScriptMetaKitEngine, source: &Path) -> FileSystemEntry {
    engine
        .snapshot(&RootId::from("root"))
        .expect("snapshot")
        .children
        .as_ref()
        .expect("children")
        .iter()
        .find(|entry| entry.display_path == source)
        .expect("reference entry")
        .clone()
}

fn notify(engine: &mut ScriptMetaKitEngine, path: &Path) -> bool {
    let events = engine
        .mark_changed_paths(RawChangeBatch {
            paths: vec![path.to_path_buf()],
            overflowed: false,
        })
        .expect("route notification");
    events
        .iter()
        .any(|event| matches!(event, ScriptMetaKitEvent::RootMarkedDirty { .. }))
}

#[test]
fn file_references_share_a_parent_watch_and_ignore_unrelated_siblings() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let external = temp.path().join("external");
    fs::create_dir(&root).expect("root");
    fs::create_dir(&external).expect("external");
    let target = external.join("処理.jsx");
    write_script(&target, "1.0.0");
    let source = root.join(reference_name());
    create_reference(&target, &source);
    create_reference(
        &target,
        &root.join(if cfg!(windows) {
            "Second.lnk"
        } else {
            "Second"
        }),
    );
    let nested = external.join("child");
    fs::create_dir(&nested).expect("nested target directory");
    write_script(&nested.join("Nested.jsx"), "1.0.0");
    create_reference(
        &nested,
        &root.join(if cfg!(windows) {
            "Directory.lnk"
        } else {
            "Directory"
        }),
    );
    let mut engine = engine(&root);
    let plan = engine.watch_plan();
    let external = external.canonicalize().expect("canonical external");

    assert_eq!(
        plan.physical_roots
            .iter()
            .filter(|watch| watch.path == external)
            .count(),
        1
    );
    assert!(plan.physical_roots.iter().all(|watch| watch.path.is_dir()));
    assert!(
        !plan
            .physical_roots
            .iter()
            .any(|watch| watch.path == nested.canonicalize().unwrap())
    );
    assert!(!notify(&mut engine, &external.join("unrelated.jsx")));
    assert!(notify(&mut engine, &target));
    assert!(notify(&mut engine, &source));
}

#[test]
fn file_reference_watches_are_restored_from_the_existing_cache_format() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let external = temp.path().join("external");
    fs::create_dir(&root).expect("root");
    fs::create_dir(&external).expect("external");
    let target = external.join("Target.jsx");
    write_script(&target, "1.0.0");
    create_reference(&target, &root.join(reference_name()));
    let writer = engine(&root);
    let cache = writer
        .export_cache(scriptmetakit::CacheScope::FileList)
        .expect("cache");
    let mut reader = ScriptMetaKitEngine::new(writer.config().clone()).expect("reader");
    reader.set_roots(writer.roots().to_vec()).expect("roots");
    reader.load_cache(cache).expect("load cache");
    assert!(
        reader
            .watch_plan()
            .physical_roots
            .iter()
            .any(|watch| watch.path == external.canonicalize().unwrap())
    );
    assert!(notify(&mut reader, &target));
}

#[test]
fn retargeting_a_file_reference_replaces_the_external_watch() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let one = temp.path().join("one");
    let two = temp.path().join("two");
    for directory in [&root, &one, &two] {
        fs::create_dir(directory).expect("directory");
    }
    let first = one.join("First.jsx");
    let second = two.join("Second.jsx");
    write_script(&first, "1.0.0");
    write_script(&second, "2.0.0");
    let source = root.join(reference_name());
    create_reference(&first, &source);
    let mut engine = engine(&root);
    create_reference(&second, &source);
    assert!(notify(&mut engine, &source));
    engine
        .refresh_dirty_roots(RefreshRequest {
            mode: ScanMode::FileListAndMetadata,
        })
        .expect("refresh");
    assert_eq!(
        entry(&engine, &source).resolved_path,
        second.canonicalize().expect("target")
    );
    let plan = engine.watch_plan();
    assert!(
        plan.physical_roots
            .iter()
            .any(|watch| watch.path == two.canonicalize().unwrap())
    );
    assert!(
        !plan
            .physical_roots
            .iter()
            .any(|watch| watch.path == one.canonicalize().unwrap())
    );
}

#[cfg(feature = "native-watch")]
fn wait_until(
    phase: &str,
    engine: &mut ScriptMetaKitEngine,
    watcher: &scriptmetakit::watcher::NativeWatcher,
    predicate: impl Fn(&ScriptMetaKitEngine) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        if let Some(batch) = watcher.try_recv() {
            engine.mark_changed_paths(batch).expect("native route");
            engine
                .refresh_dirty_roots(RefreshRequest {
                    mode: ScanMode::FileListAndMetadata,
                })
                .expect("native refresh");
            if predicate(engine) {
                return;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "reference change was not reconciled: {phase}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(feature = "native-watch")]
#[test]
fn native_file_reference_watch_detects_atomic_save_deletion_and_recovery() {
    use scriptmetakit::{PathResolutionStatus, watcher::NativeWatcher};
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let external = temp.path().join("external");
    fs::create_dir(&root).expect("root");
    fs::create_dir(&external).expect("external");
    let target = external.join("処理.jsx");
    write_script(&target, "1.0.0");
    let source = root.join(reference_name());
    create_reference(&target, &source);
    let mut engine = engine(&root);
    let watcher = NativeWatcher::start(&engine.watch_plan()).expect("watcher");
    let replacement = external.join("replacement.jsx");
    write_script(&replacement, "2.0.0");
    fs::rename(&replacement, &target).expect("atomic replacement");
    wait_until("atomic save", &mut engine, &watcher, |engine| {
        entry(engine, &source)
            .scriptmeta_item
            .as_ref()
            .and_then(|item| item.version.as_deref())
            == Some("2.0.0")
    });
    fs::remove_file(&target).expect("remove temporary fixture");
    wait_until("deletion", &mut engine, &watcher, |engine| {
        entry(engine, &source).resolution_status != PathResolutionStatus::Resolved
    });
    drop(watcher);
    let watcher = NativeWatcher::start(&engine.watch_plan()).expect("watcher after deletion");
    write_script(&target, "3.0.0");
    wait_until("recovery", &mut engine, &watcher, |engine| {
        entry(engine, &source)
            .scriptmeta_item
            .as_ref()
            .and_then(|item| item.version.as_deref())
            == Some("3.0.0")
    });
    fs::remove_file(&source).expect("remove temporary reference");
    assert!(notify(&mut engine, &source));
    engine
        .refresh_dirty_roots(RefreshRequest {
            mode: ScanMode::FileListAndMetadata,
        })
        .expect("refresh after removing reference");
    assert!(
        target.exists(),
        "removing a reference must preserve its target"
    );
    assert!(
        !engine
            .watch_plan()
            .physical_roots
            .iter()
            .any(|watch| watch.path == external.canonicalize().unwrap())
    );
}

#[test]
fn references_inside_a_referenced_folder_keep_physical_source_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let folder = temp.path().join("folder");
    fs::create_dir(&folder).expect("folder");
    let first = temp.path().join("first.jsx");
    let second = temp.path().join("second.jsx");
    write_script(&first, "1.0.0");
    write_script(&second, "2.0.0");
    let source = folder.join(reference_name());
    create_reference(&first, &source);
    let root = temp.path().join(if cfg!(windows) {
        "FolderReference.lnk"
    } else {
        "FolderReference"
    });
    create_reference(&folder, &root);
    let mut engine = engine(&root);
    create_reference(&second, &source);
    assert!(notify(&mut engine, &source));
    engine
        .refresh_dirty_roots(RefreshRequest {
            mode: ScanMode::FileListAndMetadata,
        })
        .expect("refresh");
    assert_eq!(
        entry(&engine, &root.join(reference_name())).resolved_path,
        second.canonicalize().unwrap()
    );
}

#[test]
fn scanner_options_keep_the_published_complete_initializer() {
    let options = scriptmetakit::ScannerOptions {
        max_depth: 24,
        max_nodes_per_root: 10_000,
        max_prefix_bytes: 128 * 1024,
        skip_hidden: true,
        skip_packages: true,
        follow_symlinks: true,
        resolve_macos_alias: true,
        reuse_unchanged_records: true,
        decompile_compiled_osa_during_scan: false,
        include_empty_directories: false,
        root_preflight: Default::default(),
        scan_timeout_per_root_millis: Some(30_000),
    };
    let decoded: scriptmetakit::ScannerOptions =
        serde_json::from_value(serde_json::to_value(&options).unwrap()).unwrap();
    assert_eq!(decoded, options);
}

#[cfg(unix)]
#[test]
fn symlink_retargeting_keeps_the_reference_source_path() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    fs::create_dir(&root).expect("root");
    let first = temp.path().join("first.jsx");
    let second = temp.path().join("second.jsx");
    write_script(&first, "1.0.0");
    write_script(&second, "2.0.0");
    let source = root.join("linked.jsx");
    symlink(&first, &source).expect("symlink");
    let mut engine = engine(&root);
    fs::remove_file(&source).expect("remove temporary symlink");
    symlink(&second, &source).expect("replacement symlink");
    assert!(notify(&mut engine, &source));
    engine
        .refresh_dirty_roots(RefreshRequest {
            mode: ScanMode::FileListAndMetadata,
        })
        .expect("refresh");
    assert_eq!(
        entry(&engine, &source).resolved_path,
        second.canonicalize().unwrap()
    );
}
