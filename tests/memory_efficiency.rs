use scriptmetakit::{
    CachePolicy, CacheScope, RootRegistration, ScanMode, ScanRequest, ScriptMetaKitConfig,
    ScriptMetaKitEngine, save_cache_payload,
};
use std::{fs, path::Path, sync::Arc};

fn fixture(root: &Path, id: &str) {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("test.jsx"), format!(
        "/*\nSCRIPTMETA-BEGIN\nScript-ID={id}\nMeta-Default-language=ja\nName=Example\nTags=test,fixture\nDescription-BEGIN\nBODY_MARKER_{id}\n{}\n[en]Other\nDescription-END\nSCRIPTMETA-END\n*/",
        "Japanese metadata test body. ".repeat(160)
    )).unwrap();
}

fn config(persistent: bool, idle_millis: u64, max_nodes: usize) -> ScriptMetaKitConfig {
    let mut config = ScriptMetaKitConfig::new("MemoryEfficiencyTests", "test");
    config.watcher.enabled = false;
    config.update_check.enabled = false;
    config.cache.persistent_cache = persistent;
    config.cache.idle_lifetime_millis = idle_millis;
    config.cache.max_memory_nodes = max_nodes;
    config
}

fn request() -> ScanRequest {
    ScanRequest::all(ScanMode::FileListAndMetadata)
}

fn assert_eviction_releases_metadata(
    policy: CachePolicy,
    persistent: bool,
    idle: u64,
    nodes: usize,
) {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), "com.example.memory");
    let mut engine = ScriptMetaKitEngine::new(config(persistent, idle, nodes)).unwrap();
    let mut root = RootRegistration::file_list_and_metadata("root", directory.path());
    root.cache_policy = policy;
    engine.set_roots(vec![root]).unwrap();
    let result = engine.scan_roots(request()).unwrap();
    let item = Arc::downgrade(&result.catalog_snapshot.as_ref().unwrap().file_items[0]);
    drop(result);
    let cached = engine.cached_scan_result(request());
    assert!(
        cached
            .catalog_snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.file_items.is_empty())
    );
    drop(cached);
    assert!(
        item.upgrade().is_none(),
        "evicted metadata must not remain in a persistence queue"
    );
}

#[test]
fn memory_only_metadata_is_released_after_idle_eviction() {
    assert_eviction_releases_metadata(CachePolicy::MemoryOnly, true, 0, 60_000);
}

#[test]
fn memory_only_metadata_is_released_after_node_limit_eviction() {
    assert_eviction_releases_metadata(CachePolicy::MemoryOnly, true, 1_200_000, 0);
}

#[test]
fn disabling_persistence_releases_both_catalog_and_file_list_after_eviction() {
    assert_eviction_releases_metadata(CachePolicy::MemoryAndPersistent, false, 0, 60_000);
}

#[test]
fn mixed_cache_policies_only_keep_persistable_metadata_until_saved() {
    for (idle, nodes) in [(0, 60_000), (1_200_000, 0)] {
        let directory = tempfile::tempdir().unwrap();
        let memory_path = directory.path().join("memory");
        let disk_path = directory.path().join("disk");
        fixture(&memory_path, "com.example.memory");
        fixture(&disk_path, "com.example.disk");
        let mut memory_root = RootRegistration::file_list_and_metadata("memory", &memory_path);
        memory_root.cache_policy = CachePolicy::MemoryOnly;
        let disk_root = RootRegistration::file_list_and_metadata("disk", &disk_path);
        let mut engine = ScriptMetaKitEngine::new(config(true, idle, nodes)).unwrap();
        engine.set_roots(vec![memory_root, disk_root]).unwrap();
        let result = engine.scan_roots(request()).unwrap();
        let items = &result.catalog_snapshot.as_ref().unwrap().file_items;
        let memory = Arc::downgrade(
            items
                .iter()
                .find(|item| item.root_id.as_ref() == "memory")
                .unwrap(),
        );
        let disk = Arc::downgrade(
            items
                .iter()
                .find(|item| item.root_id.as_ref() == "disk")
                .unwrap(),
        );
        drop(result);
        drop(engine.cached_scan_result(request()));
        assert!(memory.upgrade().is_none());
        assert!(disk.upgrade().is_some());
        let payload = engine.export_cache(CacheScope::All).unwrap();
        assert!(
            !serde_json::to_string(&payload)
                .unwrap()
                .contains("com.example.memory")
        );
        save_cache_payload(directory.path().join("cache.json"), &payload).unwrap();
        drop(payload);
        engine.mark_cache_persisted(CacheScope::All);
        assert!(disk.upgrade().is_none());
    }
}

fn persistent_engine(root: &Path) -> ScriptMetaKitEngine {
    let mut engine = ScriptMetaKitEngine::new(config(true, 1_200_000, 60_000)).unwrap();
    engine
        .set_roots(vec![RootRegistration::file_list_and_metadata("root", root)])
        .unwrap();
    engine
}

fn assert_shared_metadata(result: &scriptmetakit::ScanResult) {
    let catalog = result.catalog_snapshot.as_ref().unwrap();
    for item in &catalog.file_items {
        let entry = result
            .flattened_file_entries()
            .find(|entry| entry.display_path == item.file_path)
            .unwrap();
        assert!(Arc::ptr_eq(item, entry.scriptmeta_item.as_ref().unwrap()));
        let record = catalog
            .candidate_cache
            .records
            .iter()
            .find(|record| record.file_path == item.file_path)
            .unwrap();
        assert!(Arc::ptr_eq(item, record.item.as_ref().unwrap()));
    }
}

#[test]
fn complete_cache_restore_reuses_metadata_between_catalog_and_file_tree() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), "com.example.shared");
    let mut writer = persistent_engine(directory.path());
    let before = writer.scan_roots(request()).unwrap();
    let payload = writer.export_cache(CacheScope::All).unwrap();
    let mut reader = persistent_engine(directory.path());
    reader.load_cache(payload).unwrap();
    let after = reader.cached_scan_result(request());
    assert_eq!(
        before.catalog_snapshot.as_ref().unwrap().file_items,
        after.catalog_snapshot.as_ref().unwrap().file_items
    );
    assert_shared_metadata(&after);
}

#[test]
fn independently_loaded_cache_scopes_share_metadata_in_both_orders() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), "com.example.scopes");
    let mut writer = persistent_engine(directory.path());
    writer.scan_roots(request()).unwrap();
    for order in [
        [CacheScope::Catalog, CacheScope::FileList],
        [CacheScope::FileList, CacheScope::Catalog],
    ] {
        let mut reader = persistent_engine(directory.path());
        for scope in order {
            reader
                .load_cache(writer.export_cache(scope).unwrap())
                .unwrap();
        }
        assert_shared_metadata(&reader.cached_scan_result(request()));
    }
}

#[test]
fn cache_format_omits_derived_catalog_lists_and_still_reads_schema_three() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), "com.example.compact");
    let mut writer = persistent_engine(directory.path());
    let scanned = writer.scan_roots(request()).unwrap();
    let compact = writer.export_cache(CacheScope::All).unwrap();
    assert_eq!(compact.schema.schema_version, 4);
    let stored_catalog = &compact.data["catalog"]["catalog_snapshot"];
    assert!(stored_catalog.get("all_items").is_none());
    assert!(stored_catalog.get("file_items").is_none());
    assert_eq!(
        serde_json::to_string(&compact)
            .unwrap()
            .matches("BODY_MARKER_com.example.compact")
            .count(),
        2
    );
    let mut old = compact;
    old.schema.schema_version = 3;
    old.data["catalog"]["catalog_snapshot"] =
        serde_json::to_value(scanned.catalog_snapshot.as_ref().unwrap()).unwrap();
    let mut reader = persistent_engine(directory.path());
    reader.load_cache(old).unwrap();
    assert_shared_metadata(&reader.cached_scan_result(request()));
}

#[test]
fn cache_sharing_never_overwrites_different_metadata_or_file_state() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), "com.example.different");
    let mut writer = persistent_engine(directory.path());
    writer.scan_roots(request()).unwrap();
    let payload = writer.export_cache(CacheScope::All).unwrap();
    for change_metadata in [false, true] {
        let mut modified = payload.clone();
        let entry = &mut modified.data["file_list_snapshots"]["root"]["children"][0];
        if change_metadata {
            entry["scriptmeta_item"]["name"] = serde_json::json!("Older cached name");
        } else {
            entry["file_size"] = serde_json::json!(entry["file_size"].as_u64().unwrap() + 1);
        }
        let mut reader = persistent_engine(directory.path());
        reader.load_cache(modified).unwrap();
        let result = reader.cached_scan_result(request());
        let catalog_item = &result.catalog_snapshot.as_ref().unwrap().file_items[0];
        let tree_item = result
            .flattened_file_entries()
            .find_map(|entry| entry.scriptmeta_item.as_ref())
            .unwrap();
        assert!(!Arc::ptr_eq(catalog_item, tree_item));
        if change_metadata {
            assert_eq!(tree_item.name.as_deref(), Some("Older cached name"));
            assert_eq!(catalog_item.name.as_deref(), Some("Example"));
        }
    }
}

#[test]
fn cache_reconstruction_keeps_duplicate_script_ids_at_different_paths_separate() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), "com.example.duplicate");
    let original = fs::read_to_string(directory.path().join("test.jsx")).unwrap();
    fs::write(
        directory.path().join("copy.jsx"),
        original.replace("Name=Example", "Name=Copy"),
    )
    .unwrap();
    let mut writer = persistent_engine(directory.path());
    writer.scan_roots(request()).unwrap();
    let mut reader = persistent_engine(directory.path());
    reader
        .load_cache(writer.export_cache(CacheScope::All).unwrap())
        .unwrap();
    let result = reader.cached_scan_result(request());
    let catalog = result.catalog_snapshot.as_ref().unwrap();
    assert_eq!(catalog.file_items.len(), 2);
    assert_eq!(catalog.all_items.len(), 1);
    assert!(!Arc::ptr_eq(&catalog.file_items[0], &catalog.file_items[1]));
    assert_shared_metadata(&result);
}
