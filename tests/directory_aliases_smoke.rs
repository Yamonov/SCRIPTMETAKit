use std::{
    fs,
    path::{Path, PathBuf},
};

use scriptmetakit::{
    CacheScope, ExtensionPolicy, FileListSnapshot, FileSystemEntry, PathResolutionStatus,
    RawChangeBatch, RefreshRequest, RootId, RootPurpose, RootRegistration, RootStatus, ScanMode,
    ScanRequest, ScannerOptions, ScriptMetaKitConfig, ScriptMetaKitEngine,
    scanner::{
        DirectoryScanOutput, scan_file_list_root, scan_file_list_root_with_dirty_directories,
    },
};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    target: PathBuf,
    references: Vec<PathBuf>,
}

impl Fixture {
    fn new(link: fn(&Path, &Path)) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let target = root.join("Real");
        fs::create_dir_all(&target).expect("target directory");
        fs::write(target.join("Action.jsx"), "// fixture").expect("script");
        let mut references = vec![target.clone()];
        for name in ["First", "Second"] {
            let reference = root.join(name);
            link(&target, &reference);
            references.push(reference);
        }
        Self {
            _temp: temp,
            root,
            target,
            references,
        }
    }

    fn scan(&self) -> DirectoryScanOutput {
        scan_file_list_root(
            &RootId::from("root"),
            &self.root,
            &ScannerOptions::default(),
            &ExtensionPolicy::default(),
        )
    }

    fn assert_file_in_every_reference(&self, entries: &[FileSystemEntry], name: &str) {
        for reference in &self.references {
            let directory = find_display(entries, reference).expect("visible directory");
            assert_ne!(
                directory.resolution_status,
                PathResolutionStatus::Cycle,
                "{reference:?}"
            );
            let script = find_display(&directory.children, &reference.join(name))
                .unwrap_or_else(|| panic!("missing {name} under {reference:?}"));
            assert_eq!(
                script
                    .resolved_path
                    .canonicalize()
                    .expect("resolved script"),
                self.target
                    .join(name)
                    .canonicalize()
                    .expect("target script")
            );
        }
    }
}

#[cfg(unix)]
fn link_directory(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("directory symlink");
}

#[cfg(windows)]
fn link_directory(target: &Path, link: &Path) {
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("create directory junction");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "macos")]
fn alias_directory(target: &Path, alias: &Path) {
    use objc2_foundation::{NSURL, NSURLBookmarkCreationOptions};
    let target = NSURL::from_directory_path(target).expect("target URL");
    let alias = NSURL::from_file_path(alias).expect("alias URL");
    let data = target
        .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            NSURLBookmarkCreationOptions::SuitableForBookmarkFile,
            None,
            None,
        )
        .expect("bookmark data");
    NSURL::writeBookmarkData_toURL_options_error(&data, &alias, 0).expect("write alias");
}

fn find_display<'a>(entries: &'a [FileSystemEntry], display: &Path) -> Option<&'a FileSystemEntry> {
    entries.iter().find_map(|entry| {
        if entry.display_path == display {
            Some(entry)
        } else {
            find_display(&entry.children, display)
        }
    })
}

fn snapshot(output: DirectoryScanOutput) -> FileListSnapshot {
    FileListSnapshot {
        root: output.root,
        children: Some(output.children),
        directory_states: output.directory_states,
        truncated: output.truncated,
        content_revision: Default::default(),
    }
}

#[test]
fn repeated_directory_references_all_keep_their_displayed_children() {
    let fixture = Fixture::new(link_directory);
    let output = fixture.scan();
    assert_eq!(output.root.status, RootStatus::Ready);
    fixture.assert_file_in_every_reference(&output.children, "Action.jsx");
}

#[cfg(target_os = "macos")]
#[test]
fn repeated_finder_aliases_and_the_real_directory_all_have_contents() {
    let fixture = Fixture::new(alias_directory);
    fixture.assert_file_in_every_reference(&fixture.scan().children, "Action.jsx");
}

#[test]
fn repeated_directories_share_one_metadata_record_but_keep_all_visible_rows() {
    let fixture = Fixture::new(link_directory);
    fs::write(fixture.target.join("Action.jsx"),
        "// SCRIPTMETA-BEGIN\n// Script-ID: org.example.alias\n// Version: 1.0\n// Name: Shared metadata\n// SCRIPTMETA-END\n"
    ).expect("metadata script");
    let mut engine = ScriptMetaKitEngine::new(ScriptMetaKitConfig::new("Test", "AliasMetadata"))
        .expect("engine");
    let result = engine
        .scan_root_paths(vec![fixture.root.clone()], ScanMode::FileListAndMetadata)
        .expect("scan");
    assert_eq!(
        result
            .catalog_snapshot
            .as_ref()
            .expect("catalog")
            .file_items
            .len(),
        1
    );
    let entries = result.file_list_snapshots[0]
        .children
        .as_deref()
        .expect("children");
    for reference in &fixture.references {
        let item = find_display(entries, &reference.join("Action.jsx")).expect("visible script");
        assert_eq!(
            item.scriptmeta_item
                .as_ref()
                .expect("metadata")
                .name
                .as_deref(),
            Some("Shared metadata")
        );
    }
}

#[test]
fn actual_ancestor_cycles_are_stopped_in_every_reference() {
    let fixture = Fixture::new(link_directory);
    link_directory(&fixture.root, &fixture.target.join("Back"));
    let output = fixture.scan();
    assert_eq!(output.root.status, RootStatus::Ready);
    assert!(!output.truncated);
    for reference in &fixture.references {
        let back = find_display(&output.children, &reference.join("Back")).expect("back reference");
        assert_eq!(back.resolution_status, PathResolutionStatus::Cycle);
        assert!(back.children.is_empty());
    }
}

#[test]
fn partial_refresh_updates_every_reference_with_its_own_display_paths() {
    let fixture = Fixture::new(link_directory);
    let previous = snapshot(fixture.scan());
    fs::write(fixture.target.join("Added.jsx"), "// added").expect("added script");
    let output = scan_file_list_root_with_dirty_directories(
        &RootId::from("root"),
        &fixture.root,
        &ScannerOptions::default(),
        &ExtensionPolicy::default(),
        Some(&previous),
        std::slice::from_ref(&fixture.target),
    );
    assert_eq!(output.root.status, RootStatus::Ready);
    fixture.assert_file_in_every_reference(&output.children, "Added.jsx");
}

#[test]
fn partial_refresh_keeps_ancestor_cycle_guards() {
    let fixture = Fixture::new(link_directory);
    link_directory(&fixture.root, &fixture.target.join("Back"));
    let previous = snapshot(fixture.scan());
    fs::write(fixture.target.join("Added.jsx"), "// added").expect("added script");
    let output = scan_file_list_root_with_dirty_directories(
        &RootId::from("root"),
        &fixture.root,
        &ScannerOptions::default(),
        &ExtensionPolicy::default(),
        Some(&previous),
        std::slice::from_ref(&fixture.target),
    );
    for reference in &fixture.references {
        let back = find_display(&output.children, &reference.join("Back")).expect("back reference");
        assert_eq!(back.resolution_status, PathResolutionStatus::Cycle);
        assert!(back.children.is_empty());
    }
}

#[test]
fn engine_dirty_refresh_updates_all_references_with_and_without_a_held_snapshot() {
    for holds_snapshot in [false, true] {
        let fixture = Fixture::new(link_directory);
        let mut engine = ScriptMetaKitEngine::new(ScriptMetaKitConfig::new("Test", "AliasRefresh"))
            .expect("engine");
        let first = engine
            .scan_root_paths(vec![fixture.root.clone()], ScanMode::FileListAndMetadata)
            .expect("initial scan");
        let _held = holds_snapshot.then_some(first);
        let added = fixture.target.join("Added.jsx");
        fs::write(&added, "// new file").expect("new file");
        engine
            .mark_changed_paths(RawChangeBatch {
                paths: vec![added.clone()],
                overflowed: false,
            })
            .expect("changed paths");
        let updated = engine
            .refresh_dirty_roots(RefreshRequest {
                mode: ScanMode::FileListAndMetadata,
            })
            .expect("refresh");
        let tree = updated.file_list_snapshots.first().expect("snapshot");
        fixture.assert_file_in_every_reference(
            tree.children.as_deref().expect("children"),
            "Added.jsx",
        );
        drop(updated);

        fs::remove_file(&added).expect("remove fixture file");
        engine
            .mark_changed_paths(RawChangeBatch {
                paths: vec![added],
                overflowed: false,
            })
            .expect("removed path");
        let updated = engine
            .refresh_dirty_roots(RefreshRequest {
                mode: ScanMode::FileListAndMetadata,
            })
            .expect("refresh removal");
        let tree = updated.file_list_snapshots.first().expect("snapshot");
        for reference in &fixture.references {
            assert!(
                find_display(
                    tree.children.as_deref().expect("children"),
                    &reference.join("Added.jsx")
                )
                .is_none()
            );
        }
    }
}

#[test]
fn partial_removal_does_not_stop_after_the_first_reference() {
    let mut fixture = Fixture::new(link_directory);
    // Put references in different branches so an early return would miss peers.
    for (index, reference) in fixture.references.iter_mut().enumerate().skip(1) {
        let parent = fixture.root.join(format!("Parent{index}"));
        fs::create_dir(&parent).expect("parent");
        let nested = parent.join("Link");
        fs::rename(&*reference, &nested).expect("move reference");
        *reference = nested;
    }
    let previous = snapshot(fixture.scan());
    fs::remove_dir_all(&fixture.target).expect("remove fixture target");
    let output = scan_file_list_root_with_dirty_directories(
        &RootId::from("root"),
        &fixture.root,
        &ScannerOptions::default(),
        &ExtensionPolicy::default(),
        Some(&previous),
        std::slice::from_ref(&fixture.target),
    );
    for reference in &fixture.references {
        assert!(
            find_display(&output.children, reference).is_none(),
            "stale reference {reference:?}"
        );
    }
}

#[test]
fn old_false_cycle_entries_are_reconciled_on_the_next_partial_refresh() {
    let fixture = Fixture::new(link_directory);
    let other = fixture.root.join("Other");
    fs::create_dir(&other).expect("other folder");
    fs::write(other.join("Other.jsx"), "// other").expect("other script");
    let mut previous = snapshot(fixture.scan());
    let reference = previous
        .children
        .as_mut()
        .expect("children")
        .iter_mut()
        .find(|entry| entry.display_path == fixture.references[1])
        .expect("reference");
    reference.children.clear();
    reference.resolution_status = PathResolutionStatus::Cycle;
    reference.resolution_message = Some("resolved path was already visited".into());
    let output = scan_file_list_root_with_dirty_directories(
        &RootId::from("root"),
        &fixture.root,
        &ScannerOptions::default(),
        &ExtensionPolicy::default(),
        Some(&previous),
        &[other],
    );
    fixture.assert_file_in_every_reference(&output.children, "Action.jsx");
}

#[test]
fn dirty_parent_refresh_includes_a_separate_reference_to_its_descendant() {
    let fixture = Fixture::new(link_directory);
    let nested = fixture.target.join("Nested");
    fs::create_dir(&nested).expect("nested directory");
    fs::write(nested.join("Child.jsx"), "// child").expect("child");
    let direct = fixture.root.join("DirectNested");
    link_directory(&nested, &direct);
    let previous = snapshot(fixture.scan());
    fs::write(nested.join("Added.jsx"), "// added").expect("added");
    let output = scan_file_list_root_with_dirty_directories(
        &RootId::from("root"),
        &fixture.root,
        &ScannerOptions::default(),
        &ExtensionPolicy::default(),
        Some(&previous),
        std::slice::from_ref(&fixture.target),
    );
    assert!(find_display(&output.children, &direct.join("Added.jsx")).is_some());
    for reference in &fixture.references {
        assert!(find_display(&output.children, &reference.join("Nested/Added.jsx")).is_some());
    }
}

#[test]
fn legacy_physical_children_under_an_alias_are_reconciled() {
    let fixture = Fixture::new(link_directory);
    let mut previous = snapshot(fixture.scan());
    let reference = previous
        .children
        .as_mut()
        .expect("children")
        .iter_mut()
        .find(|entry| entry.display_path == fixture.references[1])
        .expect("reference");
    reference.children[0].display_path = fixture.target.join("Action.jsx");
    let output = scan_file_list_root_with_dirty_directories(
        &RootId::from("root"),
        &fixture.root,
        &ScannerOptions::default(),
        &ExtensionPolicy::default(),
        Some(&previous),
        std::slice::from_ref(&fixture.target),
    );
    fixture.assert_file_in_every_reference(&output.children, "Action.jsx");
}

#[test]
fn repeated_references_still_obey_the_root_node_limit() {
    let fixture = Fixture::new(link_directory);
    let options = ScannerOptions {
        max_nodes_per_root: 4,
        ..Default::default()
    };
    let output = scan_file_list_root(
        &RootId::from("root"),
        &fixture.root,
        &options,
        &ExtensionPolicy::default(),
    );
    assert_eq!(output.root.status, RootStatus::Overflowed);
    assert!(output.truncated);
    assert!(output.root.item_count <= 4);
}

#[test]
fn old_persistent_false_cycles_are_fixed_before_a_restored_root_becomes_ready() {
    let fixture = Fixture::new(link_directory);
    let config = ScriptMetaKitConfig::new("Test", "AliasCache");
    let registration =
        RootRegistration::user_initiated("root", fixture.root.clone(), RootPurpose::FileList);
    let mut source = ScriptMetaKitEngine::new(config.clone()).expect("source engine");
    source
        .set_roots(vec![registration.clone()])
        .expect("source root");
    source
        .scan_roots(ScanRequest::all(ScanMode::FileListOnly))
        .expect("scan");
    let mut cache = source.export_cache(CacheScope::FileList).expect("cache");
    cache.schema.package_version = "1.3.0".into();
    let entries = cache.data["root"]["children"]
        .as_array_mut()
        .expect("stored children");
    let entry = entries
        .iter_mut()
        .find(|entry| entry["display_path"].as_str() == fixture.references[1].to_str())
        .expect("stored reference");
    entry["resolution_status"] = serde_json::json!("cycle");
    entry["children"] = serde_json::json!([]);

    let mut restored = ScriptMetaKitEngine::new(config).expect("restored engine");
    restored
        .set_roots(vec![registration])
        .expect("restored root");
    restored.load_cache(cache).expect("load legacy cache");
    assert!(
        restored
            .snapshot_ref(&RootId::from("root"))
            .expect("loaded")
            .root
            .is_dirty
    );
    restored
        .mark_changed_paths(RawChangeBatch {
            paths: vec![fixture.target.join("Action.jsx")],
            overflowed: false,
        })
        .expect("mark restored root for reconciliation");
    let refreshed = restored
        .refresh_dirty_roots(RefreshRequest {
            mode: ScanMode::FileListOnly,
        })
        .expect("reconcile");
    let tree = refreshed.file_list_snapshots.first().expect("snapshot");
    assert_eq!(tree.root.status, RootStatus::Ready);
    fixture
        .assert_file_in_every_reference(tree.children.as_deref().expect("children"), "Action.jsx");
}
