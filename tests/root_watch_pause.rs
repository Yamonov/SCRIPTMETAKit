use std::{fs, path::Path};

use scriptmetakit::{
    RawChangeBatch, RefreshRequest, RootId, RootRegistration, RootStatus, ScanMode, ScanRequest,
    ScriptMetaKitConfig, ScriptMetaKitEngine, ScriptMetaKitEvent, WatchPolicy,
};

fn write_script(path: &Path, id: &str, version: &str) {
    fs::write(path, format!("// SCRIPTMETA-BEGIN\n// Script-ID={id}\n// Version={version}\n// Name=Example\n// SCRIPTMETA-END\n")).unwrap();
}

fn fixture() -> (
    tempfile::TempDir,
    ScriptMetaKitEngine,
    Vec<RootRegistration>,
) {
    let directory = tempfile::tempdir().unwrap();
    let roots = ["a", "b"]
        .map(|id| {
            let path = directory.path().join(id);
            fs::create_dir(&path).unwrap();
            write_script(&path.join("script.jsx"), id, "1.0");
            RootRegistration::file_list_and_metadata(id, path)
        })
        .to_vec();
    let mut config = ScriptMetaKitConfig::new("RootWatchPause", "tests");
    config.watcher.watch_policy = WatchPolicy::AllRegistered;
    let mut engine = ScriptMetaKitEngine::new(config).unwrap();
    engine.set_roots(roots.clone()).unwrap();
    engine
        .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
        .unwrap();
    (directory, engine, roots)
}

#[test]
fn paused_root_ignores_events_and_overflow_while_other_root_updates() {
    let (directory, mut engine, roots) = fixture();
    let a = RootId::from("a");
    let b = RootId::from("b");
    engine
        .mark_changed_paths(RawChangeBatch {
            paths: vec![roots[0].path.clone()],
            overflowed: false,
        })
        .unwrap();
    engine
        .pause_root_watching(std::slice::from_ref(&a))
        .unwrap();
    engine
        .pause_root_watching(std::slice::from_ref(&a))
        .unwrap();
    write_script(&roots[0].path.join("script.jsx"), "a", "2.0");
    write_script(&roots[1].path.join("script.jsx"), "b", "2.0");
    let events = engine
        .mark_changed_paths(RawChangeBatch {
            paths: vec![directory.path().to_path_buf()],
            overflowed: true,
        })
        .unwrap();
    for event in events {
        if let ScriptMetaKitEvent::ChangeDetected { batch } = event {
            assert!(batch.affected_roots.iter().all(|root| root.root_id == b));
            assert!(batch.events.iter().all(|event| event.root_id != a));
        }
    }
    let refreshed = engine
        .refresh_dirty_roots(RefreshRequest {
            mode: ScanMode::FileListAndMetadata,
        })
        .unwrap();
    assert_eq!(
        refreshed
            .roots
            .iter()
            .map(|root| root.root_id.as_ref())
            .collect::<Vec<_>>(),
        ["b"]
    );
    let catalog = engine.catalog_snapshot().unwrap();
    let version = |id: &str| {
        catalog
            .file_items
            .iter()
            .find(|item| item.script_id == id)
            .unwrap()
            .version
            .as_ref()
            .unwrap()
            .to_string()
    };
    assert_eq!(version("a"), "1.0");
    assert_eq!(version("b"), "2.0");
    assert!(
        engine
            .watch_plan()
            .logical_roots
            .iter()
            .all(|root| root.root_id != a)
    );
    assert!(
        engine
            .watch_plan()
            .logical_roots
            .iter()
            .any(|root| root.root_id == b)
    );
}

#[test]
fn resume_reads_same_size_and_mtime_changes_and_rebuilds_the_tree() {
    let (_directory, mut engine, roots) = fixture();
    let id = RootId::from("a");
    let path = roots[0].path.join("script.jsx");
    let removed = roots[0].path.join("removed.jsx");
    write_script(&removed, "removed", "1.0");
    engine
        .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
        .unwrap();
    let before = fs::metadata(&path).unwrap();
    engine
        .pause_root_watching(std::slice::from_ref(&id))
        .unwrap();
    write_script(&path, "a", "2.0");
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(before.modified().unwrap()))
        .unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), before.len());
    assert_eq!(
        fs::metadata(&path).unwrap().modified().unwrap(),
        before.modified().unwrap()
    );
    fs::remove_file(removed).unwrap();
    let nested = roots[0].path.join("nested");
    fs::create_dir(&nested).unwrap();
    write_script(&nested.join("added.jsx"), "added", "1.0");
    let result = engine
        .resume_root_watching(std::slice::from_ref(&id))
        .unwrap();
    assert_eq!(result.roots.len(), 1);
    assert_eq!(result.roots[0].status, RootStatus::Ready);
    assert!(!engine.is_root_watching_paused(&id));
    let catalog = result.catalog_snapshot.unwrap();
    assert_eq!(
        catalog
            .file_items
            .iter()
            .find(|item| item.script_id == "a")
            .unwrap()
            .version
            .as_ref()
            .unwrap()
            .as_str(),
        "2.0"
    );
    assert!(
        catalog
            .file_items
            .iter()
            .any(|item| item.script_id == "added")
    );
    assert!(
        catalog
            .file_items
            .iter()
            .all(|item| item.script_id != "removed")
    );
    assert!(
        result.file_list_snapshots[0]
            .children
            .as_ref()
            .unwrap()
            .iter()
            .any(|entry| entry.display_path == nested)
    );
}

#[test]
fn incomplete_batch_resume_keeps_every_requested_root_paused_until_retry() {
    let (directory, mut engine, roots) = fixture();
    let ids = vec![RootId::from("a"), RootId::from("b")];
    engine.pause_root_watching(&ids).unwrap();
    fs::rename(&roots[0].path, directory.path().join("moved-a")).unwrap();
    let result = engine.resume_root_watching(&ids).unwrap();
    assert_eq!(
        result
            .roots
            .iter()
            .find(|root| root.root_id == ids[0])
            .unwrap()
            .status,
        RootStatus::Missing
    );
    assert!(ids.iter().all(|id| engine.is_root_watching_paused(id)));
    assert!(engine.watch_plan().physical_roots.is_empty());
    assert!(
        engine
            .refresh_dirty_roots(RefreshRequest {
                mode: ScanMode::FileListAndMetadata
            })
            .unwrap()
            .roots
            .is_empty()
    );
    fs::rename(directory.path().join("moved-a"), &roots[0].path).unwrap();
    let retried = engine.resume_root_watching(&ids).unwrap();
    assert!(
        retried
            .roots
            .iter()
            .all(|root| root.status == RootStatus::Ready)
    );
    assert!(ids.iter().all(|id| !engine.is_root_watching_paused(id)));
}

#[test]
fn invalid_ids_are_atomic_and_root_reregistration_preserves_suspension() {
    let (_directory, mut engine, roots) = fixture();
    let a = RootId::from("a");
    assert!(engine.pause_root_watching(&[]).is_err());
    assert!(
        engine
            .pause_root_watching(&[a.clone(), RootId::from("unknown")])
            .is_err()
    );
    assert!(!engine.is_root_watching_paused(&a));
    engine
        .pause_root_watching(std::slice::from_ref(&a))
        .unwrap();
    assert!(
        engine
            .resume_root_watching(&[a.clone(), RootId::from("unknown")])
            .is_err()
    );
    assert!(engine.is_root_watching_paused(&a));
    engine.set_roots(roots.clone()).unwrap();
    assert!(engine.is_root_watching_paused(&a));
    let mut config = engine.config().clone();
    config.watcher.debounce_delay_millis += 1;
    engine.apply_configuration(config).unwrap();
    assert!(
        engine
            .clone_for_reconfiguration()
            .is_root_watching_paused(&a)
    );
    engine.set_roots(vec![roots[1].clone()]).unwrap();
    engine.set_roots(roots).unwrap();
    assert!(!engine.is_root_watching_paused(&a));
}

#[test]
fn pausing_nested_root_does_not_disable_its_registered_parent() {
    let (directory, mut engine, mut roots) = fixture();
    let parent = RootRegistration::file_list_and_metadata("parent", directory.path());
    roots.push(parent);
    engine.set_roots(roots).unwrap();
    engine.pause_root_watching(&[RootId::from("a")]).unwrap();
    let events = engine
        .mark_changed_paths(RawChangeBatch {
            paths: vec![directory.path().join("a/script.jsx")],
            overflowed: true,
        })
        .unwrap();
    assert!(events.iter().any(|event| matches!(event, ScriptMetaKitEvent::RootMarkedDirty { root_id } if root_id.as_ref() == "parent")));
    assert!(events.iter().all(|event| !matches!(event, ScriptMetaKitEvent::RootMarkedDirty { root_id } if root_id.as_ref() == "a")));
}

#[test]
fn cancelled_resume_leaves_the_root_paused_and_can_be_retried() {
    let (_directory, mut engine, _roots) = fixture();
    let id = RootId::from("a");
    engine
        .pause_root_watching(std::slice::from_ref(&id))
        .unwrap();
    let cancellation = engine.cancellation_token();
    let reservation = cancellation.reserve_next_operation();
    cancellation.cancel_current_or_reserved();
    let result = engine
        .resume_root_watching(std::slice::from_ref(&id))
        .unwrap();
    drop(reservation);
    assert_eq!(result.roots[0].status, RootStatus::Cancelled);
    assert!(engine.is_root_watching_paused(&id));
    assert_eq!(
        engine
            .resume_root_watching(std::slice::from_ref(&id))
            .unwrap()
            .roots[0]
            .status,
        RootStatus::Ready
    );
    assert!(!engine.is_root_watching_paused(&id));
}

#[test]
fn incomplete_file_list_cannot_be_reported_as_a_successful_metadata_reload() {
    let (_directory, mut engine, roots) = fixture();
    write_script(&roots[0].path.join("second.jsx"), "second", "1.0");
    engine.pause_root_watching(&[RootId::from("a")]).unwrap();
    let mut config = engine.config().clone();
    config.scanner.max_nodes_per_root = 1;
    engine.apply_configuration(config).unwrap();
    let result = engine.resume_root_watching(&[RootId::from("a")]).unwrap();
    assert_ne!(result.file_list_snapshots[0].root.status, RootStatus::Ready);
    assert_ne!(result.roots[0].status, RootStatus::Ready);
    assert!(engine.is_root_watching_paused(&RootId::from("a")));
    let mut config = engine.config().clone();
    config.scanner.max_nodes_per_root = 100;
    engine.apply_configuration(config).unwrap();
    let retried = engine.resume_root_watching(&[RootId::from("a")]).unwrap();
    assert_eq!(retried.roots[0].status, RootStatus::Ready);
    assert_eq!(retried.catalog_snapshot.unwrap().file_items.len(), 2);
}
