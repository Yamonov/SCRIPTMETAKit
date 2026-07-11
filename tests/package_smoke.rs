#[test]
fn imports_scriptmetakit_crate() {
    assert_eq!(scriptmetakit::package_name(), "scriptmetakit");
}

#[test]
fn default_config_sets_scan_and_update_timeouts() {
    let config = scriptmetakit::ScriptMetaKitConfig::default();

    assert_eq!(config.scanner.scan_timeout_per_root_millis, Some(30_000));
    assert_eq!(config.scanner.max_nodes_per_root, 10_000);
    assert!(!config.scanner.decompile_compiled_osa_during_scan);
    assert!(config.scanner.root_preflight.reject_trash_roots);
    assert!(config.scanner.root_preflight.reject_restricted_roots);
    assert!(
        config
            .scanner
            .root_preflight
            .reject_low_script_density_large_roots
    );
    assert!(config.supported_extensions.contains_extension("scptd"));
    assert_eq!(config.update_check.request_timeout_millis, Some(15_000));
    assert_eq!(config.update_check.resource_timeout_millis, Some(15_000));
    assert_eq!(config.update_check.retry_initial_delay_millis, 500);
    assert_eq!(config.update_check.retry_backoff_multiplier, 3);
    assert_eq!(config.update_check.max_retry_delay_millis, 30_000);
}

#[test]
fn disabled_update_check_stops_before_resolver_creation() {
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "DisabledUpdateCheck");
    config.update_check.enabled = false;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");

    let error = pollster::block_on(
        engine.check_updates(scriptmetakit::UpdateCheckRequest { items: Vec::new() }),
    )
    .expect_err("disabled update check");

    assert!(error.to_string().contains("update checking is disabled"));
}

#[test]
fn parses_script_metadata_without_replacing_name() {
    let metadata = scriptmetakit::parse_script_metadata(
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.resize
// Version: 1.2.3
// Name: Human Title
// Author: Yamo
// Meta-URL: https://example.com/SCRIPTMETA.txt
// Description-BEGIN
// Resize selected object.
// Description-END
// SCRIPTMETA-END
"#,
    )
    .expect("metadata should parse");

    assert_eq!(metadata.script_id, "com.example.resize");
    assert_eq!(metadata.version.as_deref(), Some("1.2.3"));
    assert_eq!(metadata.name.as_deref(), Some("Human Title"));
    assert_eq!(
        metadata.description.as_deref(),
        Some("Resize selected object.")
    );
}

#[test]
fn parses_equals_style_script_metadata() {
    let metadata = scriptmetakit::parse_script_metadata(
        r#"
SCRIPTMETA-BEGIN
Script-ID=org.iwashi.Halftone_Generator
Version=1.1
Meta-URL=https://gist.github.com/Yamonov/6f00bd65e486513d82f773f858ac76cb
Name=Photoshopで疑似AMスクリーン生成
Description-BEGIN
Photoshop用のテストメタデータです。
Description-END
SCRIPTMETA-END
"#,
    )
    .expect("equals-style metadata should parse");

    assert_eq!(metadata.script_id, "org.iwashi.Halftone_Generator");
    assert_eq!(metadata.version.as_deref(), Some("1.1"));
    assert_eq!(
        metadata.meta_url.as_ref().map(|url| url.as_str()),
        Some("https://gist.github.com/Yamonov/6f00bd65e486513d82f773f858ac76cb")
    );
    assert_eq!(
        metadata.name.as_deref(),
        Some("Photoshopで疑似AMスクリーン生成")
    );
    assert_eq!(
        metadata.description.as_deref(),
        Some("Photoshop用のテストメタデータです。")
    );
}

#[test]
fn parses_distribution_metadata_for_matching_script_id() {
    let metadata = scriptmetakit::parse_distribution_metadata_for_script(
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID=org.example.first
Version=1.0.0
Script-ID=org.example.second
Version=2.5.0
SCRIPTMETA-DIST-END
"#,
        "org.example.second",
    )
    .expect("matching distribution metadata should parse");

    assert_eq!(metadata.script_id.as_deref(), Some("org.example.second"));
    assert_eq!(metadata.latest_version.as_deref(), Some("2.5.0"));
}

#[test]
fn missing_multi_script_distribution_entry_does_not_reuse_other_script_id() {
    let metadata = scriptmetakit::parse_distribution_metadata_for_script(
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID=org.example.first
Version=1.0.0
Script-ID=org.example.second
Version=2.5.0
SCRIPTMETA-DIST-END
"#,
        "org.example.third",
    )
    .expect("missing distribution metadata should return unresolved metadata");

    assert_eq!(metadata.script_id.as_deref(), None);
    assert_eq!(metadata.latest_version.as_deref(), None);
    assert_eq!(
        metadata.note.as_deref(),
        Some("distribution metadata for script id `org.example.third` was not found")
    );
}

#[test]
fn parses_inline_distribution_metadata_from_note_description() {
    let metadata = scriptmetakit::parse_distribution_metadata_for_script(
        r#"
SCRIPTMETA-DIST-BEGIN Script-ID=com.kojirasetakuma.ai.datamergekit Version=2.5.0 Latest-URL=https://note.com/nice_lotus120/n/n1a3ab689bed6 Latest-Page-URL=https://note.com/nice_lotus120/n/n1a3ab689bed6 Release-Date=2026-05-21 SCRIPTMETA-DIST-END
"#,
        "com.kojirasetakuma.ai.datamergekit",
    )
    .expect("inline distribution metadata should parse");

    assert_eq!(
        metadata.script_id.as_deref(),
        Some("com.kojirasetakuma.ai.datamergekit")
    );
    assert_eq!(metadata.latest_version.as_deref(), Some("2.5.0"));
    assert_eq!(
        metadata.latest_url.as_ref().map(url::Url::as_str),
        Some("https://note.com/nice_lotus120/n/n1a3ab689bed6")
    );
    assert_eq!(
        metadata.latest_page_url.as_ref().map(url::Url::as_str),
        Some("https://note.com/nice_lotus120/n/n1a3ab689bed6")
    );
}

#[test]
fn parses_html_distribution_metadata_from_note_body() {
    let metadata = scriptmetakit::parse_distribution_metadata_for_script(
        r#"
SCRIPTMETA-DIST-BEGIN<br>Script-ID=com.kojirasetakuma.ai.datamergekit<br>Version=2.5.0<br>Latest-URL=<a href="https://note.com/nice_lotus120/n/n1a3ab689bed6" target="_blank" rel="noopener nofollow">https://note.com/nice_lotus120/n/n1a3ab689bed6</a><br>Latest-Page-URL=<a href="https://note.com/nice_lotus120/n/n1a3ab689bed6" target="_blank" rel="noopener nofollow">https://note.com/nice_lotus120/n/n1a3ab689bed6</a><br>Release-Date=2026-05-21<br>SCRIPTMETA-DIST-END
"#,
        "com.kojirasetakuma.ai.datamergekit",
    )
    .expect("HTML distribution metadata should parse");

    assert_eq!(
        metadata.script_id.as_deref(),
        Some("com.kojirasetakuma.ai.datamergekit")
    );
    assert_eq!(metadata.latest_version.as_deref(), Some("2.5.0"));
    assert_eq!(
        metadata.latest_url.as_ref().map(url::Url::as_str),
        Some("https://note.com/nice_lotus120/n/n1a3ab689bed6")
    );
    assert_eq!(
        metadata.latest_page_url.as_ref().map(url::Url::as_str),
        Some("https://note.com/nice_lotus120/n/n1a3ab689bed6")
    );
}

#[test]
fn treats_html_closing_blocks_as_distribution_line_breaks() {
    let metadata = scriptmetakit::parse_distribution_metadata_for_script(
        r#"
SCRIPTMETA-DIST-BEGIN</br>Script-ID=com.example.html</p><p>Version=3.1.0</p><p>Latest-Page-URL=https://example.com/script</p>SCRIPTMETA-DIST-END
"#,
        "com.example.html",
    )
    .expect("HTML block separators should parse");

    assert_eq!(metadata.script_id.as_deref(), Some("com.example.html"));
    assert_eq!(metadata.latest_version.as_deref(), Some("3.1.0"));
    assert_eq!(
        metadata.latest_page_url.as_ref().map(url::Url::as_str),
        Some("https://example.com/script")
    );
}

#[test]
fn rejects_legacy_script_metadata_for_distribution_metadata() {
    let error = scriptmetakit::parse_distribution_metadata_for_script(
        r#"
SCRIPTMETA-BEGIN
Script-ID=org.example.legacy
Version=1.0.0
SCRIPTMETA-END
"#,
        "org.example.legacy",
    )
    .expect_err("legacy script metadata is not distribution metadata");

    assert!(
        error
            .to_string()
            .contains("missing SCRIPTMETA distribution block")
    );
}

#[test]
fn builds_watch_plan_for_visible_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root_a = temp.path().join("A");
    let root_b = temp.path().join("B");
    std::fs::create_dir_all(&root_a).expect("root a");
    std::fs::create_dir_all(&root_b).expect("root b");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    engine
        .set_roots(vec![
            scriptmetakit::RootRegistration {
                root_id: "a".into(),
                path: root_a,
                display_name: Some("A".into()),
                purpose: scriptmetakit::RootPurpose::FileListAndMetadata,
                watch_policy: scriptmetakit::WatchPolicy::VisibleOnly,
                cache_policy: scriptmetakit::CachePolicy::MemoryAndPersistent,
                refresh_policy: scriptmetakit::RefreshPolicy::OnVisible,
                priority: scriptmetakit::RootPriority::VisibleWhenSelected,
            },
            scriptmetakit::RootRegistration {
                root_id: "b".into(),
                path: root_b,
                display_name: Some("B".into()),
                purpose: scriptmetakit::RootPurpose::FileListAndMetadata,
                watch_policy: scriptmetakit::WatchPolicy::VisibleOnly,
                cache_policy: scriptmetakit::CachePolicy::MemoryAndPersistent,
                refresh_policy: scriptmetakit::RefreshPolicy::OnVisible,
                priority: scriptmetakit::RootPriority::VisibleWhenSelected,
            },
        ])
        .expect("roots");
    engine.set_visible_root(Some("b".into()));

    let plan = engine.watch_plan();
    assert_eq!(plan.physical_roots.len(), 1);
    assert_eq!(
        plan.physical_roots[0].covers_root_ids,
        vec![scriptmetakit::RootId::from("b")]
    );
}

#[test]
fn scans_metadata_and_file_list() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Example.jsx");
    std::fs::write(
        &script_path,
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.script
// Version: 2.0.0
// Name: Example Script
// SCRIPTMETA-END
"#,
    )
    .expect("script");
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    engine
        .set_roots(vec![scriptmetakit::RootRegistration {
            root_id: "scripts".into(),
            path: temp.path().to_path_buf(),
            display_name: Some("Scripts".into()),
            purpose: scriptmetakit::RootPurpose::FileListAndMetadata,
            watch_policy: scriptmetakit::WatchPolicy::AllRegistered,
            cache_policy: scriptmetakit::CachePolicy::MemoryAndPersistent,
            refresh_policy: scriptmetakit::RefreshPolicy::OnFileEvent,
            priority: scriptmetakit::RootPriority::UserInitiated,
        }])
        .expect("roots");

    let result = engine
        .scan_roots(scriptmetakit::ScanRequest::all(
            scriptmetakit::ScanMode::FileListAndMetadata,
        ))
        .expect("scan");

    assert_eq!(result.file_list_snapshots.len(), 1);
    assert_eq!(
        result
            .catalog_snapshot
            .as_ref()
            .expect("catalog")
            .all_items
            .len(),
        1
    );
    assert_eq!(
        result.catalog_snapshot.unwrap().all_items[0]
            .name
            .as_deref(),
        Some("Example Script")
    );
}

#[test]
fn config_parser_options_control_metadata_scan() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("ParserOptions.jsx");
    let source = format!(
        "// {}\n// SCRIPTMETA-BEGIN\n// Script-ID: com.example.parser-options\n",
        "padding".repeat(32)
    );
    std::fs::write(&script_path, source).expect("script");
    let root = scriptmetakit::RootRegistration {
        root_id: "scripts".into(),
        path: temp.path().to_path_buf(),
        display_name: Some("Scripts".into()),
        purpose: scriptmetakit::RootPurpose::MetadataCatalog,
        watch_policy: scriptmetakit::WatchPolicy::AllRegistered,
        cache_policy: scriptmetakit::CachePolicy::MemoryAndPersistent,
        refresh_policy: scriptmetakit::RefreshPolicy::OnFileEvent,
        priority: scriptmetakit::RootPriority::UserInitiated,
    };

    let mut strict_config = scriptmetakit::ScriptMetaKitConfig::new("Test", "ParserStrict");
    strict_config.scanner.max_prefix_bytes = 4096;
    strict_config.parser.max_prefix_bytes = 64;
    strict_config.parser.require_closed_block = false;
    let mut strict_engine =
        scriptmetakit::ScriptMetaKitEngine::new(strict_config).expect("strict engine");
    strict_engine.set_roots(vec![root.clone()]).expect("roots");
    let short_prefix = strict_engine
        .scan_roots(scriptmetakit::ScanRequest::all(
            scriptmetakit::ScanMode::MetadataOnly,
        ))
        .expect("short prefix scan");
    assert!(
        short_prefix
            .catalog_snapshot
            .expect("short prefix catalog")
            .all_items
            .is_empty()
    );

    let mut permissive_config = scriptmetakit::ScriptMetaKitConfig::new("Test", "ParserPermissive");
    permissive_config.scanner.max_prefix_bytes = 64;
    permissive_config.parser.max_prefix_bytes = 4096;
    permissive_config.parser.require_closed_block = false;
    let mut permissive_engine =
        scriptmetakit::ScriptMetaKitEngine::new(permissive_config).expect("permissive engine");
    permissive_engine.set_roots(vec![root]).expect("roots");
    let full_prefix = permissive_engine
        .scan_roots(scriptmetakit::ScanRequest::all(
            scriptmetakit::ScanMode::MetadataOnly,
        ))
        .expect("full prefix scan");
    let catalog = full_prefix.catalog_snapshot.expect("full prefix catalog");
    let items = &catalog.all_items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].script_id, "com.example.parser-options");
}

#[test]
fn memory_cache_can_be_disabled_without_hiding_scan_result() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("NoCache.jsx"),
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.no-cache
// Version: 1.0.0
// SCRIPTMETA-END
"#,
    )
    .expect("script");
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "Test");
    config.cache.memory_cache = false;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    engine
        .set_roots(vec![scriptmetakit::RootRegistration {
            root_id: "scripts".into(),
            path: temp.path().to_path_buf(),
            display_name: Some("Scripts".into()),
            purpose: scriptmetakit::RootPurpose::FileListAndMetadata,
            watch_policy: scriptmetakit::WatchPolicy::AllRegistered,
            cache_policy: scriptmetakit::CachePolicy::MemoryAndPersistent,
            refresh_policy: scriptmetakit::RefreshPolicy::OnFileEvent,
            priority: scriptmetakit::RootPriority::UserInitiated,
        }])
        .expect("roots");

    let result = engine
        .scan_roots(scriptmetakit::ScanRequest::all(
            scriptmetakit::ScanMode::FileListAndMetadata,
        ))
        .expect("scan");

    assert_eq!(result.file_list_snapshots.len(), 1);
    assert_eq!(
        result
            .catalog_snapshot
            .as_ref()
            .expect("returned catalog")
            .all_items
            .len(),
        1
    );
    assert!(
        engine
            .snapshot(&scriptmetakit::RootId::from("scripts"))
            .is_none()
    );
    assert!(engine.catalog_snapshot().is_none());
}

#[test]
fn memory_node_limit_evicts_one_root_without_clearing_every_cache() {
    let temp = tempfile::tempdir().expect("tempdir");
    let first = temp.path().join("First");
    let second = temp.path().join("Second");
    std::fs::create_dir_all(&first).expect("first root");
    std::fs::create_dir_all(&second).expect("second root");
    std::fs::write(
        first.join("First.jsx"),
        "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.first\n// SCRIPTMETA-END\n",
    )
    .expect("first script");
    std::fs::write(
        second.join("Second.jsx"),
        "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.second\n// SCRIPTMETA-END\n",
    )
    .expect("second script");

    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "RootEviction");
    config.cache.max_memory_nodes = 2;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    engine
        .set_roots(vec![
            scriptmetakit::RootRegistration::file_list_and_metadata("first", &first),
            scriptmetakit::RootRegistration::file_list_and_metadata("second", &second),
        ])
        .expect("roots");

    let scanned = engine
        .scan_roots(scriptmetakit::ScanRequest::all(
            scriptmetakit::ScanMode::FileListAndMetadata,
        ))
        .expect("scan");
    assert_eq!(scanned.file_list_snapshots.len(), 2);

    let cached = engine.cached_scan_result(scriptmetakit::ScanRequest::all(
        scriptmetakit::ScanMode::FileListAndMetadata,
    ));
    assert_eq!(cached.file_list_snapshots.len(), 1);
    assert_eq!(
        cached
            .catalog_snapshot
            .as_ref()
            .expect("catalog")
            .file_items
            .len(),
        1
    );
    assert_eq!(
        cached
            .roots
            .iter()
            .filter(|root| root.status == scriptmetakit::RootStatus::NotLoaded)
            .count(),
        1
    );
    assert!(
        engine.export_cache(scriptmetakit::CacheScope::All).is_err(),
        "a partial in-memory cache must not overwrite a complete persistent cache"
    );
}

#[test]
fn root_cache_policy_disabled_skips_engine_storage_only() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("DisabledCache.jsx"),
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.disabled-cache
// Version: 1.0.0
// SCRIPTMETA-END
"#,
    )
    .expect("script");
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    engine
        .set_roots(vec![scriptmetakit::RootRegistration {
            root_id: "scripts".into(),
            path: temp.path().to_path_buf(),
            display_name: Some("Scripts".into()),
            purpose: scriptmetakit::RootPurpose::FileListAndMetadata,
            watch_policy: scriptmetakit::WatchPolicy::AllRegistered,
            cache_policy: scriptmetakit::CachePolicy::Disabled,
            refresh_policy: scriptmetakit::RefreshPolicy::OnFileEvent,
            priority: scriptmetakit::RootPriority::UserInitiated,
        }])
        .expect("roots");

    let result = engine
        .scan_roots(scriptmetakit::ScanRequest::all(
            scriptmetakit::ScanMode::FileListAndMetadata,
        ))
        .expect("scan");

    assert_eq!(
        result
            .catalog_snapshot
            .as_ref()
            .expect("returned catalog")
            .all_items
            .len(),
        1
    );
    assert!(
        engine
            .snapshot(&scriptmetakit::RootId::from("scripts"))
            .is_none()
    );
    assert!(
        engine
            .catalog_snapshot()
            .as_ref()
            .is_none_or(|catalog| catalog.all_items.is_empty())
    );
}

#[test]
fn scans_multiple_root_paths_with_root_aware_result() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root_a = temp.path().join("A");
    let root_b = temp.path().join("B");
    std::fs::create_dir_all(&root_a).expect("root a");
    std::fs::create_dir_all(&root_b).expect("root b");
    std::fs::write(
        root_a.join("A.jsx"),
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.a
// Version: 1.0.0
// Name: Script A
// SCRIPTMETA-END
"#,
    )
    .expect("script a");
    std::fs::write(
        root_b.join("B.jsx"),
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.b
// Version: 1.0.0
// Name: Script B
// SCRIPTMETA-END
"#,
    )
    .expect("script b");

    let root_a_id = scriptmetakit::path_based_root_id(&root_a);
    let root_b_id = scriptmetakit::path_based_root_id(&root_b);
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");

    let result = engine
        .scan_root_paths(
            vec![root_a, root_b],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");

    assert_eq!(engine.roots().len(), 2);
    assert_eq!(result.roots.len(), 2);
    assert_eq!(result.file_list_snapshots.len(), 2);
    assert!(
        result
            .file_list_snapshots
            .iter()
            .any(|snapshot| snapshot.root.root_id == root_a_id)
    );
    assert!(
        result
            .file_list_snapshots
            .iter()
            .any(|snapshot| snapshot.root.root_id == root_b_id)
    );

    let catalog = result.catalog_snapshot.as_ref().expect("catalog");
    assert_eq!(catalog.roots.len(), 2);
    assert_eq!(catalog.all_items.len(), 2);
    let mut item_ids: Vec<_> = catalog
        .all_items
        .iter()
        .map(|item| (item.root_id.as_ref(), item.script_id.as_ref()))
        .collect();
    item_ids.sort();
    assert_eq!(
        item_ids,
        vec![
            (root_a_id.as_ref(), "com.example.a"),
            (root_b_id.as_ref(), "com.example.b")
        ]
    );
}

#[test]
fn keeps_file_items_for_overlapping_registered_roots() {
    let temp = tempfile::tempdir().expect("tempdir");
    let parent = temp.path().join("Parent");
    let child = parent.join("JSX");
    std::fs::create_dir_all(&child).expect("child dir");
    std::fs::write(
        child.join("Shared.jsx"),
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.overlap
// Version: 1.0.0
// Name: Overlap
// SCRIPTMETA-END
"#,
    )
    .expect("script");

    let parent_id = scriptmetakit::path_based_root_id(&parent);
    let child_id = scriptmetakit::path_based_root_id(&child);
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("HostApp", "HostApp.ScriptMetaKit");
    config.scanner.decompile_compiled_osa_during_scan = true;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");

    let result = engine
        .scan_root_paths(
            vec![parent, child],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");
    let catalog = result.catalog_snapshot.as_ref().expect("catalog");

    assert_eq!(catalog.all_items.len(), 1);
    assert_eq!(catalog.file_items.len(), 2);
    assert_eq!(
        catalog
            .file_items
            .iter()
            .filter(|item| item.root_id == parent_id)
            .count(),
        1
    );
    assert_eq!(
        catalog
            .file_items
            .iter()
            .filter(|item| item.root_id == child_id)
            .count(),
        1
    );
}

#[test]
fn checks_updates_for_duplicate_script_id_file_items_independently() {
    let first = tempfile::tempdir().expect("first tempdir");
    let second = tempfile::tempdir().expect("second tempdir");
    let dist = tempfile::tempdir().expect("dist tempdir");
    let dist_path = dist.path().join("SCRIPTMETA.txt");
    let dist_url = url::Url::from_file_path(&dist_path).expect("dist url");
    std::fs::write(
        &dist_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.duplicate
Latest-Version: 2.0.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("dist");

    let first_script = first.path().join("Duplicate.jsx");
    let second_script = second.path().join("Duplicate.jsx");
    for path in [&first_script, &second_script] {
        std::fs::write(
            path,
            format!(
                r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.duplicate
// Version: 2.0.0
// Meta-URL: {dist_url}
// Name: Duplicate
// SCRIPTMETA-END
"#
            ),
        )
        .expect("script");
    }

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    let scan_result = engine
        .scan_root_paths(
            vec![first.path().to_path_buf(), second.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");
    let catalog = scan_result.catalog_snapshot.as_ref().expect("catalog");
    assert_eq!(catalog.all_items.len(), 1);
    assert_eq!(catalog.file_items.len(), 2);

    let items = catalog.file_items.clone();
    let update_result =
        pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest { items }))
            .expect("update check");
    let first_item_id = first_script.to_string_lossy().into_owned();
    let second_item_id = second_script.to_string_lossy().into_owned();
    assert_eq!(
        update_result
            .statuses_by_item_id
            .get(&first_item_id)
            .copied(),
        Some(scriptmetakit::UpdateStatus::UpToDate)
    );
    assert_eq!(
        update_result
            .statuses_by_item_id
            .get(&second_item_id)
            .copied(),
        Some(scriptmetakit::UpdateStatus::UpToDate)
    );
    assert_eq!(update_result.statuses_by_item_id.len(), 2);
}

#[test]
fn scans_shared_script_extensions_for_any_host_app() {
    let temp = tempfile::tempdir().expect("tempdir");
    let extensions = [
        "js",
        "jsx",
        "jsxbin",
        "jsxinc",
        "scpt",
        "applescript",
        "jxa",
        "idjs",
        "psjs",
    ];
    for extension in extensions {
        std::fs::write(
            temp.path().join(format!("Example.{extension}")),
            format!(
                r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.{extension}
// Version: 1.0.0
// Name: {extension}
// SCRIPTMETA-END
"#
            ),
        )
        .expect("script");
    }

    let mut config = scriptmetakit::ScriptMetaKitConfig::new("HostApp", "HostApp.ScriptMetaKit");
    config.scanner.decompile_compiled_osa_during_scan = true;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");

    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");

    let snapshot = result
        .file_list_snapshots
        .first()
        .expect("file list snapshot");
    assert_eq!(
        count_files(snapshot.children.as_deref().unwrap_or_default()),
        extensions.len()
    );

    let catalog = result.catalog_snapshot.as_ref().expect("catalog");
    assert_eq!(catalog.all_items.len(), extensions.len());
}

#[cfg(target_os = "macos")]
#[test]
fn scans_compiled_scpt_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Example.scpt");
    let source = r#"(*
SCRIPTMETA-BEGIN
Script-ID: com.example.compiled.applescript
Version: v1. 2 .3
Meta-URL: 'example.com/SCRIPTMETA.txt'
Name: CCライブラリパネル表示・非表示
SCRIPTMETA-END
*)
display dialog "hello"
"#;
    if !compile_osa_source(source, None, &script_path) {
        return;
    }

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("HostApp", "HostApp.ScriptMetaKit"),
    )
    .expect("engine");
    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");
    let catalog = result.catalog_snapshot.as_ref().expect("catalog");
    assert_eq!(catalog.all_items.len(), 1);
    let item = &catalog.all_items[0];
    assert_eq!(item.script_id, "com.example.compiled.applescript");
    assert_eq!(item.version.as_deref(), Some("1.2.3"));
    assert_eq!(
        item.meta_url.as_ref().map(url::Url::as_str),
        Some("https://example.com/SCRIPTMETA.txt")
    );
    assert_eq!(item.name.as_deref(), Some("CCライブラリパネル表示・非表示"));
    assert_eq!(
        item.runtime_kind,
        Some(scriptmetakit::ScriptRuntimeKind::AppleScript)
    );
    assert!(item.can_edit_scriptmeta);
}

#[cfg(target_os = "macos")]
#[test]
fn scans_compiled_jxa_scpt_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("ExampleJXA.scpt");
    let source = r#"/*
SCRIPTMETA-BEGIN
Script-ID: com.example.compiled.jxa
Version: 2.0.0
SCRIPTMETA-END
*/
Application("Finder").name();
"#;
    if !compile_osa_source(source, Some("JavaScript"), &script_path) {
        return;
    }

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("HostApp", "HostApp.ScriptMetaKit"),
    )
    .expect("engine");
    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");
    let catalog = result.catalog_snapshot.as_ref().expect("catalog");
    assert_eq!(catalog.all_items.len(), 1);
    let item = &catalog.all_items[0];
    assert_eq!(item.script_id, "com.example.compiled.jxa");
    assert_eq!(
        item.runtime_kind,
        Some(scriptmetakit::ScriptRuntimeKind::JavaScriptForAutomation)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn compiled_scpt_scan_skips_decompile_by_default() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("NoMetadata.scpt");
    if !compile_osa_source("display dialog \"hello\"\n", None, &script_path) {
        return;
    }

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("HostApp", "HostApp.ScriptMetaKit"),
    )
    .expect("engine");
    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");

    let catalog = result.catalog_snapshot.as_ref().expect("catalog");
    assert!(catalog.all_items.is_empty());
    let record = catalog
        .candidate_cache
        .records
        .iter()
        .find(|record| record.file_path == script_path)
        .expect("compiled scpt record");
    assert_eq!(
        record.scriptmeta_edit_state,
        scriptmetakit::ScriptMetaEditState::Unknown
    );
    assert!(
        result
            .file_issues
            .iter()
            .all(|issue| issue.path != script_path)
    );
}

#[test]
fn scptd_is_listed_as_unsupported_script_package_without_metadata_scan() {
    let temp = tempfile::tempdir().expect("tempdir");
    let package_path = temp.path().join("Workflow.scptd");
    let nested_script = package_path.join("Contents/Resources/Scripts/main.scpt");
    std::fs::create_dir_all(nested_script.parent().expect("nested parent"))
        .expect("create package");
    std::fs::write(
        &nested_script,
        r#"(*
SCRIPTMETA-BEGIN
Script-ID: com.example.scptd.inner
Version: 1.0.0
SCRIPTMETA-END
*)
display dialog "hello"
"#,
    )
    .expect("nested script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("HostApp", "HostApp.ScriptMetaKit"),
    )
    .expect("engine");
    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");

    let entries = result.file_list_snapshots[0]
        .children
        .as_deref()
        .unwrap_or_default();
    let package_entry = find_file_entry(entries, &package_path).expect("scptd package entry");
    assert!(!package_entry.is_directory);
    assert!(package_entry.children.is_empty());
    assert_eq!(
        package_entry.runtime_kind,
        Some(scriptmetakit::ScriptRuntimeKind::AppleScript)
    );
    assert_eq!(
        package_entry.scriptmeta_edit_state,
        scriptmetakit::ScriptMetaEditState::Unsupported
    );
    assert!(!contains_file(entries, &nested_script));

    let catalog = result.catalog_snapshot.as_ref().expect("catalog");
    assert!(catalog.all_items.is_empty());
    assert!(catalog.file_items.is_empty());
    assert!(catalog.candidate_cache.records.is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn editkit_writes_compiled_scpt_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Editable.scpt");
    if !compile_osa_source("display dialog \"hello\"\n", None, &script_path) {
        return;
    }

    let backup_root = temp.path().join("backups");
    let draft = scriptmetakit::ScriptMetadataDraft {
        script_id: "com.example.edit.compiled".to_string(),
        version: Some("v3. 4 .5".into()),
        meta_url: Some(url::Url::parse("https://example.com/SCRIPTMETA.txt").expect("url")),
        ..scriptmetakit::ScriptMetadataDraft::default()
    };
    let result = scriptmetakit::write_script_metadata_to_file(
        &script_path,
        &draft,
        scriptmetakit::ScriptMetaWriteMode::InsertOrReplace,
        Some(&scriptmetakit::ScriptMetaBackupOptions {
            root_directory: backup_root,
        }),
    )
    .expect("write compiled metadata");
    assert_eq!(
        result.operation,
        scriptmetakit::ScriptMetaWriteOperation::Inserted
    );
    assert!(result.backup.is_some());

    let source = decompile_osa_source(&script_path).expect("decompile updated");
    let metadata = scriptmetakit::parse_script_metadata(&source).expect("metadata");
    assert_eq!(metadata.script_id, "com.example.edit.compiled");
    assert_eq!(metadata.version.as_deref(), Some("3.4.5"));

    let preview = scriptmetakit::read_script_metadata_edit_preview_from_file(&script_path, 4096)
        .expect("compiled preview");
    assert!(preview.preview_text.contains("display dialog \"hello\""));
    assert!(preview.preview_text.contains("SCRIPTMETA-BEGIN"));
    assert!(preview.preview_byte_count > 0);
    assert!(preview.comment_style.is_some());
    assert!(!preview.is_truncated);
    assert!(!preview.requires_full_read);
}

#[test]
fn editkit_preview_reads_only_requested_prefix() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Large.jsx");
    let source = format!(
        "{}\n/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.preview\nSCRIPTMETA-END\n*/\n",
        "alert('x');\n".repeat(512)
    );
    std::fs::write(&script_path, source.as_bytes()).expect("script");

    let preview = scriptmetakit::read_script_metadata_edit_preview_from_file(&script_path, 128)
        .expect("preview");
    assert_eq!(preview.preview_byte_count, 128);
    assert_eq!(preview.file_size, Some(source.len() as u64));
    assert!(preview.is_truncated);
    assert!(preview.requires_full_read);
    assert!(!preview.has_scriptmeta_marker_in_preview);
    assert_eq!(
        preview.comment_style,
        Some(scriptmetakit::ScriptMetaCommentStyle::JavaScriptBlock)
    );
    assert!(!preview.file_state_fingerprint.is_empty());
}

#[test]
fn excludes_shell_command_and_txt_files_by_default() {
    let temp = tempfile::tempdir().expect("tempdir");
    for extension in ["sh", "command", "txt"] {
        std::fs::write(
            temp.path().join(format!("Example.{extension}")),
            format!(
                r#"
# SCRIPTMETA-BEGIN
# Script-ID: com.example.{extension}
# Version: 1.0.0
# SCRIPTMETA-END
"#
            ),
        )
        .expect("script");
    }

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("HostApp", "HostApp.ScriptMetaKit"),
    )
    .expect("engine");

    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");

    let snapshot = result
        .file_list_snapshots
        .first()
        .expect("file list snapshot");
    assert_eq!(
        count_files(snapshot.children.as_deref().unwrap_or_default()),
        0
    );
    assert_eq!(
        result
            .catalog_snapshot
            .as_ref()
            .expect("catalog")
            .all_items
            .len(),
        0
    );
}

#[test]
fn ignores_bookmark_like_unsupported_files_without_alias_resolution() {
    let temp = tempfile::tempdir().expect("tempdir");
    let unsupported = temp.path().join("LooksLikeAlias.txt");
    std::fs::write(&unsupported, b"book0000marknot a script").expect("unsupported file");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("HostApp", "HostApp.ScriptMetaKit"),
    )
    .expect("engine");
    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");

    assert!(
        !result
            .flattened_file_entries()
            .any(|entry| entry.display_path == unsupported)
    );
    let catalog = result.catalog_snapshot.as_ref().expect("catalog");
    assert!(catalog.file_items.is_empty());
    assert!(catalog.candidate_cache.records.is_empty());
}

#[test]
fn detects_jxa_shebang_for_js_and_applescript_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("Plain.js"),
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.plain-js
// Version: 1.0.0
// SCRIPTMETA-END
"#,
    )
    .expect("plain js");
    std::fs::write(
        temp.path().join("Automation.js"),
        r#"#!/usr/bin/osascript -l JavaScript
// SCRIPTMETA-BEGIN
// Script-ID: com.example.jxa-js
// Version: 1.0.0
// SCRIPTMETA-END
"#,
    )
    .expect("jxa js");
    std::fs::write(
        temp.path().join("Automation.applescript"),
        r#"#!/usr/bin/osascript -l JavaScript
// SCRIPTMETA-BEGIN
// Script-ID: com.example.jxa-applescript
// Version: 1.0.0
// SCRIPTMETA-END
"#,
    )
    .expect("jxa applescript");
    std::fs::write(
        temp.path().join("EnvAutomation.js"),
        r#"#!/usr/bin/env osascript -l JavaScript
// SCRIPTMETA-BEGIN
// Script-ID: com.example.env-js
// Version: 1.0.0
// SCRIPTMETA-END
"#,
    )
    .expect("env js");
    std::fs::write(
        temp.path().join("EnvSplitAutomation.js"),
        r#"#!/usr/bin/env -S osascript -l JavaScript
// SCRIPTMETA-BEGIN
// Script-ID: com.example.env-split-js
// Version: 1.0.0
// SCRIPTMETA-END
"#,
    )
    .expect("env -S js");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("HostApp", "HostApp.ScriptMetaKit"),
    )
    .expect("engine");

    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");
    let catalog = result.catalog_snapshot.as_ref().expect("catalog");
    let runtime_by_script_id: std::collections::BTreeMap<_, _> = catalog
        .all_items
        .iter()
        .map(|item| (item.script_id.as_ref(), item.runtime_kind))
        .collect();

    assert_eq!(
        runtime_by_script_id.get("com.example.plain-js").copied(),
        Some(Some(scriptmetakit::ScriptRuntimeKind::AdobeJavaScript))
    );
    assert_eq!(
        runtime_by_script_id.get("com.example.jxa-js").copied(),
        Some(Some(
            scriptmetakit::ScriptRuntimeKind::JavaScriptForAutomation
        ))
    );
    assert_eq!(
        runtime_by_script_id
            .get("com.example.jxa-applescript")
            .copied(),
        Some(Some(
            scriptmetakit::ScriptRuntimeKind::JavaScriptForAutomation
        ))
    );
    assert_eq!(
        runtime_by_script_id.get("com.example.env-js").copied(),
        Some(Some(
            scriptmetakit::ScriptRuntimeKind::JavaScriptForAutomation
        ))
    );
    assert_eq!(
        runtime_by_script_id
            .get("com.example.env-split-js")
            .copied(),
        Some(Some(
            scriptmetakit::ScriptRuntimeKind::JavaScriptForAutomation
        ))
    );

    let snapshot = result
        .file_list_snapshots
        .first()
        .expect("file list snapshot");
    let entries = snapshot.children.as_deref().unwrap_or_default();
    assert!(
        contains_runtime_kind(
            entries,
            "Automation.js",
            scriptmetakit::ScriptRuntimeKind::JavaScriptForAutomation
        ),
        "file list should expose JXA runtime for shebang-based .js"
    );
}

#[test]
fn reports_scriptmeta_edit_capabilities_for_file_list_and_items() {
    let temp = tempfile::tempdir().expect("tempdir");
    let editable_path = temp.path().join("Editable.jsx");
    let appendable_path = temp.path().join("Appendable.jsx");
    let obfuscated_path = temp.path().join("Obfuscated.jsx");
    let readonly_path = temp.path().join("ReadOnly.jsx");

    std::fs::write(
        &editable_path,
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.editable
// Version: 1.0.0
// Edit-Password-SHA256: salt:abcdef
// SCRIPTMETA-END
"#,
    )
    .expect("editable script");
    std::fs::write(&appendable_path, "alert('append');").expect("appendable script");
    std::fs::write(&obfuscated_path, "@JSXBIN@ES@2.0@script").expect("jsxbin script");
    std::fs::write(
        &readonly_path,
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.readonly
// Version: 1.0.0
// SCRIPTMETA-END
"#,
    )
    .expect("readonly script");
    let original_permissions = std::fs::metadata(&readonly_path)
        .expect("readonly metadata")
        .permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(&readonly_path, permissions).expect("set readonly");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("HostApp", "HostApp.ScriptMetaKit"),
    )
    .expect("engine");

    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");

    std::fs::set_permissions(&readonly_path, original_permissions).expect("restore permissions");

    let entries = result.file_list_snapshots[0]
        .children
        .as_deref()
        .unwrap_or_default();
    let editable_entry = find_file_entry(entries, &editable_path).expect("editable entry");
    assert!(editable_entry.has_scriptmeta);
    assert!(editable_entry.has_scriptmeta_edit_password);
    assert!(editable_entry.can_edit_scriptmeta);
    assert!(!editable_entry.can_append_scriptmeta);
    assert_eq!(
        editable_entry.scriptmeta_edit_state,
        scriptmetakit::ScriptMetaEditState::Editable
    );

    let appendable_entry = find_file_entry(entries, &appendable_path).expect("appendable entry");
    assert!(!appendable_entry.has_scriptmeta);
    assert!(!appendable_entry.can_edit_scriptmeta);
    assert!(appendable_entry.can_append_scriptmeta);
    assert_eq!(
        appendable_entry.scriptmeta_edit_state,
        scriptmetakit::ScriptMetaEditState::Appendable
    );

    let obfuscated_entry = find_file_entry(entries, &obfuscated_path).expect("obfuscated entry");
    assert!(!obfuscated_entry.can_edit_scriptmeta);
    assert!(!obfuscated_entry.can_append_scriptmeta);
    assert_eq!(
        obfuscated_entry.scriptmeta_edit_state,
        scriptmetakit::ScriptMetaEditState::Obfuscated
    );

    let readonly_entry = find_file_entry(entries, &readonly_path).expect("readonly entry");
    assert!(readonly_entry.has_scriptmeta);
    assert!(readonly_entry.is_read_only);
    assert!(!readonly_entry.can_edit_scriptmeta);
    assert_eq!(
        readonly_entry.scriptmeta_edit_state,
        scriptmetakit::ScriptMetaEditState::ReadOnly
    );

    let catalog = result.catalog_snapshot.as_ref().expect("catalog");
    let editable_item = catalog
        .file_items
        .iter()
        .find(|item| item.script_id == "com.example.editable")
        .expect("editable item");
    assert!(editable_item.can_edit_scriptmeta);
    assert!(editable_item.has_scriptmeta_edit_password);
}

#[test]
fn omits_directories_without_displayable_script_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let empty_dir = temp.path().join("Empty");
    let asset_dir = temp.path().join("Assets");
    let script_dir = temp.path().join("Scripts");
    std::fs::create_dir_all(&empty_dir).expect("empty dir");
    std::fs::create_dir_all(&asset_dir).expect("asset dir");
    std::fs::create_dir_all(&script_dir).expect("script dir");
    std::fs::write(asset_dir.join("note.md"), "# note").expect("asset");
    std::fs::write(script_dir.join("WithoutMeta.jsx"), "alert('ok');").expect("script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("HostApp", "HostApp.ScriptMetaKit"),
    )
    .expect("engine");

    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");

    let children = result.file_list_snapshots[0]
        .children
        .as_deref()
        .unwrap_or_default();
    assert!(!contains_directory(children, &empty_dir));
    assert!(!contains_directory(children, &asset_dir));
    assert!(contains_directory(children, &script_dir));
}

#[test]
fn scans_scripts_in_third_level_directory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let nested_dir = temp.path().join("Brush").join("OpacityFlowExposure");
    std::fs::create_dir_all(&nested_dir).expect("nested dir");
    let script_path = nested_dir.join("OFE_O3.jsx");
    std::fs::write(
        &script_path,
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.deep
// Version: 1.0.0
// Name: Deep Script
// SCRIPTMETA-END
"#,
    )
    .expect("script");
    let expected_script_path = script_path.canonicalize().expect("canonical script path");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    engine
        .set_roots(vec![scriptmetakit::RootRegistration {
            root_id: "scripts".into(),
            path: temp.path().to_path_buf(),
            display_name: Some("Scripts".into()),
            purpose: scriptmetakit::RootPurpose::FileListAndMetadata,
            watch_policy: scriptmetakit::WatchPolicy::AllRegistered,
            cache_policy: scriptmetakit::CachePolicy::MemoryAndPersistent,
            refresh_policy: scriptmetakit::RefreshPolicy::OnFileEvent,
            priority: scriptmetakit::RootPriority::UserInitiated,
        }])
        .expect("roots");

    let result = engine
        .scan_roots(scriptmetakit::ScanRequest::all(
            scriptmetakit::ScanMode::FileListAndMetadata,
        ))
        .expect("scan");

    let snapshot = result
        .file_list_snapshots
        .first()
        .expect("file list snapshot");
    assert!(
        contains_file(
            snapshot.children.as_deref().unwrap_or_default(),
            &expected_script_path
        ),
        "third-level script should be present in file list"
    );
    assert_eq!(snapshot.root.status, scriptmetakit::RootStatus::Ready);
    assert!(!snapshot.truncated);

    let catalog = result.catalog_snapshot.as_ref().expect("catalog");
    assert_eq!(catalog.all_items.len(), 1);
    assert_eq!(catalog.all_items[0].script_id, "com.example.deep");
}

#[test]
fn scans_equals_style_script_metadata_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("HalftoneGenerator.jsx");
    std::fs::write(
        &script_path,
        r#"
SCRIPTMETA-BEGIN
Script-ID=org.iwashi.Halftone_Generator
Version=1.1
Name=Photoshopで疑似AMスクリーン生成
SCRIPTMETA-END
"#,
    )
    .expect("script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    engine
        .set_roots(vec![scriptmetakit::RootRegistration {
            root_id: "scripts".into(),
            path: temp.path().to_path_buf(),
            display_name: Some("Photoshop".into()),
            purpose: scriptmetakit::RootPurpose::FileListAndMetadata,
            watch_policy: scriptmetakit::WatchPolicy::AllRegistered,
            cache_policy: scriptmetakit::CachePolicy::MemoryAndPersistent,
            refresh_policy: scriptmetakit::RefreshPolicy::OnFileEvent,
            priority: scriptmetakit::RootPriority::UserInitiated,
        }])
        .expect("roots");

    let result = engine
        .scan_roots(scriptmetakit::ScanRequest::all(
            scriptmetakit::ScanMode::FileListAndMetadata,
        ))
        .expect("scan");

    let catalog = result.catalog_snapshot.as_ref().expect("catalog");
    assert_eq!(catalog.all_items.len(), 1);
    assert_eq!(
        catalog.all_items[0].script_id,
        "org.iwashi.Halftone_Generator"
    );
    assert_eq!(
        catalog.all_items[0].name.as_deref(),
        Some("Photoshopで疑似AMスクリーン生成")
    );
}

#[test]
fn reports_file_list_changes_after_rescan() {
    let temp = tempfile::tempdir().expect("tempdir");
    let removed_path = temp.path().join("Removed.jsx");
    let modified_path = temp.path().join("Modified.jsx");
    let added_path = temp.path().join("Added.jsx");
    std::fs::write(&removed_path, "alert('removed');").expect("removed");
    std::fs::write(&modified_path, "alert('before');").expect("modified before");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("initial scan");

    std::fs::remove_file(&removed_path).expect("remove");
    std::fs::write(&modified_path, "alert('after after');").expect("modified after");
    std::fs::write(&added_path, "alert('added');").expect("added");

    let result = engine
        .scan_roots(scriptmetakit::ScanRequest::all(
            scriptmetakit::ScanMode::FileListAndMetadata,
        ))
        .expect("rescan");
    let summary = result.change_summary.expect("change summary");

    assert_eq!(summary.added_count, 1);
    assert_eq!(summary.removed_count, 1);
    assert_eq!(summary.modified_count, 1);

    let changes: std::collections::BTreeMap<_, _> = summary
        .changes
        .iter()
        .map(|change| {
            (
                change
                    .resolved_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(""),
                change.kind,
            )
        })
        .collect();
    assert_eq!(
        changes.get("Added.jsx").copied(),
        Some(scriptmetakit::FileEntryChangeKind::Added)
    );
    assert_eq!(
        changes.get("Removed.jsx").copied(),
        Some(scriptmetakit::FileEntryChangeKind::Removed)
    );
    assert_eq!(
        changes.get("Modified.jsx").copied(),
        Some(scriptmetakit::FileEntryChangeKind::Modified)
    );
}

#[test]
fn ignores_hidden_and_unsupported_file_events_before_dirtying_roots() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Example.jsx");
    std::fs::write(&script_path, "alert('script');").expect("script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("initial scan");

    let root_id = scriptmetakit::path_based_root_id(temp.path());
    let hidden_path = temp.path().join(".DS_Store");
    let unsupported_path = temp.path().join("notes.txt");
    std::fs::write(&hidden_path, "hidden").expect("hidden file");
    std::fs::write(&unsupported_path, "notes").expect("unsupported file");

    let events = engine
        .mark_changed_paths(scriptmetakit::RawChangeBatch {
            paths: vec![hidden_path, unsupported_path],
            overflowed: false,
        })
        .expect("mark changed");

    let batch = events
        .into_iter()
        .find_map(|event| match event {
            scriptmetakit::ScriptMetaKitEvent::ChangeDetected { batch } => Some(batch),
            _ => None,
        })
        .expect("ignored change batch");
    assert_eq!(batch.ignored_paths.len(), 2);
    assert!(batch.affected_roots.is_empty());
    assert!(!engine.snapshot(&root_id).expect("snapshot").root.is_dirty);

    let refreshed = engine
        .refresh_dirty_roots(scriptmetakit::RefreshRequest {
            mode: scriptmetakit::ScanMode::FileListAndMetadata,
        })
        .expect("refresh dirty roots");
    assert!(refreshed.change_summary.is_none());
}

#[test]
fn removes_empty_directories_after_script_file_disappears() {
    let temp = tempfile::tempdir().expect("tempdir");
    let directory = temp.path().join("Tools");
    std::fs::create_dir_all(&directory).expect("script directory");
    let script_path = directory.join("Tool.jsx");
    std::fs::write(&script_path, "alert('tool');").expect("script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    let initial = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("initial scan");
    let initial_snapshot = initial
        .file_list_snapshots
        .first()
        .expect("initial file list");
    assert!(contains_directory(
        initial_snapshot.children.as_deref().unwrap_or_default(),
        &directory
    ));

    std::fs::remove_file(&script_path).expect("remove script");
    engine
        .mark_changed_paths(scriptmetakit::RawChangeBatch {
            paths: vec![script_path],
            overflowed: false,
        })
        .expect("mark changed");

    let refreshed = engine
        .refresh_dirty_roots(scriptmetakit::RefreshRequest {
            mode: scriptmetakit::ScanMode::FileListAndMetadata,
        })
        .expect("refresh dirty roots");
    let snapshot = refreshed
        .file_list_snapshots
        .first()
        .expect("refreshed file list");
    assert!(!contains_directory(
        snapshot.children.as_deref().unwrap_or_default(),
        &directory
    ));
}

#[test]
fn directory_rename_updates_visible_file_list() {
    let temp = tempfile::tempdir().expect("tempdir");
    let old_directory = temp.path().join("Old");
    let new_directory = temp.path().join("New");
    std::fs::create_dir_all(&old_directory).expect("old directory");
    let old_script_path = old_directory.join("Tool.jsx");
    std::fs::write(&old_script_path, "alert('tool');").expect("script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("initial scan");

    std::fs::rename(&old_directory, &new_directory).expect("rename directory");
    let new_script_path = new_directory
        .join("Tool.jsx")
        .canonicalize()
        .expect("canonical new script");
    engine
        .mark_changed_paths(scriptmetakit::RawChangeBatch {
            paths: vec![old_directory, new_directory.clone()],
            overflowed: false,
        })
        .expect("mark changed");

    let refreshed = engine
        .refresh_dirty_roots(scriptmetakit::RefreshRequest {
            mode: scriptmetakit::ScanMode::FileListAndMetadata,
        })
        .expect("refresh dirty roots");
    let snapshot = refreshed
        .file_list_snapshots
        .first()
        .expect("refreshed file list");
    let children = snapshot.children.as_deref().unwrap_or_default();

    assert!(contains_directory(children, &new_directory));
    assert!(contains_file(children, &new_script_path));
    assert!(!children.iter().any(|entry| {
        entry
            .display_path
            .file_name()
            .and_then(|name| name.to_str())
            == Some("Old")
    }));
}

#[cfg(target_os = "macos")]
#[test]
fn scans_macos_alias_directory_with_display_path_preserved() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = tempfile::tempdir().expect("target tempdir");
    let target_script = target.path().join("AliasTarget.jsx");
    std::fs::write(&target_script, "alert('alias target');").expect("target script");
    let alias_path = temp.path().join("LinkedScripts");
    create_macos_alias(target.path(), &alias_path, true);

    let result = scriptmetakit::scanner::scan_file_list_root(
        &scriptmetakit::RootId::from("root"),
        temp.path(),
        &scriptmetakit::ScannerOptions::default(),
        &scriptmetakit::ExtensionPolicy::default(),
    );
    let children = result.children;
    let alias_entry = children
        .iter()
        .find(|entry| entry.display_path == alias_path)
        .expect("alias directory entry");
    assert_eq!(alias_entry.path_kind, scriptmetakit::PathKind::MacosAlias);
    assert_eq!(
        alias_entry.resolution_status,
        scriptmetakit::PathResolutionStatus::Resolved
    );
    assert_eq!(
        alias_entry.resolved_path,
        target.path().canonicalize().expect("target")
    );
    assert!(contains_file(
        &alias_entry.children,
        &target_script.canonicalize().expect("target script")
    ));
    assert!(alias_entry.children.iter().any(|entry| {
        entry.display_path == alias_path.join("AliasTarget.jsx")
            && entry.resolved_path == target_script.canonicalize().expect("target script")
    }));
}

#[cfg(target_os = "macos")]
#[test]
fn scans_macos_alias_script_without_alias_extension() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target_script = temp.path().join("Actual.jsx");
    std::fs::write(&target_script, "alert('actual');").expect("target script");
    let alias_script = temp.path().join("ScriptAlias");
    create_macos_alias(&target_script, &alias_script, false);

    let result = scriptmetakit::scanner::scan_file_list_root(
        &scriptmetakit::RootId::from("root"),
        temp.path(),
        &scriptmetakit::ScannerOptions::default(),
        &scriptmetakit::ExtensionPolicy::default(),
    );
    let alias_entry = result
        .children
        .iter()
        .find(|entry| entry.display_path == alias_script)
        .expect("alias script entry");
    assert!(!alias_entry.is_directory);
    assert_eq!(alias_entry.path_kind, scriptmetakit::PathKind::MacosAlias);
    assert_eq!(
        alias_entry.resolution_status,
        scriptmetakit::PathResolutionStatus::Resolved
    );
    assert_eq!(
        alias_entry.resolved_path,
        target_script.canonicalize().expect("target script")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn skips_macos_alias_script_when_resolution_is_disabled() {
    let root = tempfile::tempdir().expect("root tempdir");
    let target = tempfile::tempdir().expect("target tempdir");
    let target_script = target.path().join("Actual.jsx");
    std::fs::write(
        &target_script,
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.alias-disabled
// Version: 1.0.0
// SCRIPTMETA-END
"#,
    )
    .expect("target script");
    let alias_script = root.path().join("ScriptAlias");
    create_macos_alias(&target_script, &alias_script, false);

    let scanner_options = scriptmetakit::ScannerOptions {
        resolve_macos_alias: false,
        ..Default::default()
    };
    let file_list = scriptmetakit::scanner::scan_file_list_root(
        &scriptmetakit::RootId::from("root"),
        root.path(),
        &scanner_options,
        &scriptmetakit::ExtensionPolicy::default(),
    );
    assert!(
        file_list
            .children
            .iter()
            .all(|entry| entry.display_path != alias_script)
    );

    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "Test");
    config.scanner.resolve_macos_alias = false;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    let result = engine
        .scan_root_paths(
            vec![root.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");
    assert_eq!(
        result
            .catalog_snapshot
            .as_ref()
            .expect("catalog")
            .all_items
            .len(),
        0
    );
}

#[cfg(unix)]
#[test]
fn follow_symlinks_false_never_scans_root_directory_or_file_targets() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let external = tempfile::tempdir().expect("external");
    std::fs::write(
        external.path().join("External.jsx"),
        "/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.external\nSCRIPTMETA-END\n*/\n",
    )
    .expect("external script");

    let linked_root = temp.path().join("LinkedRoot");
    symlink(external.path(), &linked_root).expect("root symlink");

    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "NoSymlinks");
    config.scanner.follow_symlinks = false;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config.clone()).expect("engine");
    let root_result = engine
        .scan_root_paths(
            vec![linked_root],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("root scan");
    assert_eq!(
        root_result.roots[0].status,
        scriptmetakit::RootStatus::Unreadable
    );
    assert!(
        root_result
            .catalog_snapshot
            .as_ref()
            .is_none_or(|catalog| catalog.all_items.is_empty())
    );

    let registered_root = temp.path().join("Registered");
    std::fs::create_dir(&registered_root).expect("registered root");
    symlink(external.path(), registered_root.join("LinkedDirectory")).expect("directory symlink");
    symlink(
        external.path().join("External.jsx"),
        registered_root.join("LinkedFile.jsx"),
    )
    .expect("file symlink");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    let nested_result = engine
        .scan_root_paths(
            vec![registered_root],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("nested scan");
    assert!(
        nested_result
            .catalog_snapshot
            .as_ref()
            .is_none_or(|catalog| catalog.all_items.is_empty())
    );
    let entries = nested_result.flattened_file_entries();
    assert_eq!(
        entries
            .filter(|entry| entry.path_kind == scriptmetakit::PathKind::Symlink)
            .count(),
        2
    );
}

#[cfg(target_os = "macos")]
#[test]
fn reports_macos_alias_directory_cycle_without_descending() {
    let temp = tempfile::tempdir().expect("tempdir");
    let alias_path = temp.path().join("Loop");
    create_macos_alias(temp.path(), &alias_path, true);

    let result = scriptmetakit::scanner::scan_file_list_root(
        &scriptmetakit::RootId::from("root"),
        temp.path(),
        &scriptmetakit::ScannerOptions::default(),
        &scriptmetakit::ExtensionPolicy::default(),
    );
    let alias_entry = result
        .children
        .iter()
        .find(|entry| entry.display_path == alias_path)
        .expect("cycle alias entry");
    assert!(alias_entry.is_directory);
    assert_eq!(alias_entry.path_kind, scriptmetakit::PathKind::MacosAlias);
    assert_eq!(
        alias_entry.resolution_status,
        scriptmetakit::PathResolutionStatus::Cycle
    );
    assert!(alias_entry.children.is_empty());
}

#[test]
fn preflight_rejects_low_script_density_root_before_full_file_list_scan() {
    let temp = tempfile::tempdir().expect("tempdir");
    for index in 0..12 {
        std::fs::write(
            temp.path().join(format!("Document{index}.txt")),
            "plain text",
        )
        .expect("text file");
    }

    let options = low_script_density_preflight_options();
    let result = scriptmetakit::scanner::scan_file_list_root(
        &scriptmetakit::RootId::from("root"),
        temp.path(),
        &options,
        &scriptmetakit::ExtensionPolicy::default(),
    );

    assert_eq!(result.root.status, scriptmetakit::RootStatus::Overflowed);
    assert_eq!(
        result.root.error.as_ref().map(|error| error.code.as_str()),
        Some("too_large_for_script_folder")
    );
    assert!(result.children.is_empty());
}

#[test]
fn preflight_allows_script_dense_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    for index in 0..12 {
        std::fs::write(
            temp.path().join(format!("Script{index}.jsx")),
            "alert('ok');",
        )
        .expect("script file");
    }

    let options = low_script_density_preflight_options();
    let result = scriptmetakit::scanner::scan_file_list_root(
        &scriptmetakit::RootId::from("root"),
        temp.path(),
        &options,
        &scriptmetakit::ExtensionPolicy::default(),
    );

    assert_eq!(result.root.status, scriptmetakit::RootStatus::Ready);
    assert_eq!(result.children.len(), 12);
}

#[test]
fn metadata_scan_reports_low_script_density_preflight_issue() {
    let temp = tempfile::tempdir().expect("tempdir");
    for index in 0..12 {
        std::fs::write(
            temp.path().join(format!("Document{index}.txt")),
            "plain text",
        )
        .expect("text file");
    }

    let options = low_script_density_preflight_options();
    let root = scriptmetakit::RootRegistration::user_initiated(
        "root",
        temp.path(),
        scriptmetakit::RootPurpose::FileListAndMetadata,
    );
    let result = scriptmetakit::scanner::scan_metadata_roots(
        [&root],
        &options,
        &scriptmetakit::ExtensionPolicy::default(),
        None,
    );

    assert_eq!(
        result.roots[0].status,
        scriptmetakit::RootStatus::Overflowed
    );
    assert_eq!(
        result.roots[0]
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("too_large_for_script_folder")
    );
    assert!(result.candidate_cache.records.is_empty());
}

#[test]
fn metadata_scan_reports_max_nodes_limit() {
    let temp = tempfile::tempdir().expect("tempdir");
    for index in 0..5 {
        std::fs::write(
            temp.path().join(format!("Script{index}.jsx")),
            format!(
                r#"
// SCRIPTMETA-BEGIN
// Script-ID=com.example.limit.{index}
// SCRIPTMETA-END
"#
            ),
        )
        .expect("script file");
    }

    let mut options = scriptmetakit::ScannerOptions {
        max_nodes_per_root: 3,
        ..Default::default()
    };
    options.root_preflight.reject_low_script_density_large_roots = false;
    let root = scriptmetakit::RootRegistration::user_initiated(
        "root",
        temp.path(),
        scriptmetakit::RootPurpose::FileListAndMetadata,
    );
    let result = scriptmetakit::scanner::scan_metadata_roots(
        [&root],
        &options,
        &scriptmetakit::ExtensionPolicy::default(),
        None,
    );

    assert_eq!(
        result.roots[0].status,
        scriptmetakit::RootStatus::Overflowed
    );
    assert_eq!(
        result.roots[0]
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("max_nodes_exceeded")
    );
    assert_eq!(result.candidate_cache.records.len(), 3);
}

#[test]
fn metadata_scan_reports_max_depth_limit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let nested = temp.path().join("Nested");
    std::fs::create_dir(&nested).expect("nested directory");
    std::fs::write(
        nested.join("Deep.jsx"),
        r#"
// SCRIPTMETA-BEGIN
// Script-ID=com.example.limit.depth
// SCRIPTMETA-END
"#,
    )
    .expect("script file");

    let mut options = scriptmetakit::ScannerOptions {
        max_depth: 0,
        ..Default::default()
    };
    options.root_preflight.reject_low_script_density_large_roots = false;
    let root = scriptmetakit::RootRegistration::user_initiated(
        "root",
        temp.path(),
        scriptmetakit::RootPurpose::FileListAndMetadata,
    );
    let result = scriptmetakit::scanner::scan_metadata_roots(
        [&root],
        &options,
        &scriptmetakit::ExtensionPolicy::default(),
        None,
    );

    assert_eq!(
        result.roots[0].status,
        scriptmetakit::RootStatus::Overflowed
    );
    assert_eq!(
        result.roots[0]
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("max_depth_exceeded")
    );
    assert!(result.candidate_cache.records.is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn rejects_restricted_registered_root_before_scanning() {
    let result = scriptmetakit::scanner::scan_file_list_root(
        &scriptmetakit::RootId::from("root"),
        std::path::Path::new("/"),
        &scriptmetakit::ScannerOptions::default(),
        &scriptmetakit::ExtensionPolicy::default(),
    );

    assert_eq!(result.root.status, scriptmetakit::RootStatus::Unreadable);
    assert_eq!(
        result.root.error.as_ref().map(|error| error.code.as_str()),
        Some("restricted_root")
    );
}

fn low_script_density_preflight_options() -> scriptmetakit::ScannerOptions {
    let mut options = scriptmetakit::ScannerOptions::default();
    options.root_preflight.max_scanned_items = 8;
    options.root_preflight.max_duration_millis = 0;
    options.root_preflight.min_scanned_file_count_for_large_root = 8;
    options.root_preflight.min_script_ratio_denominator = 2;
    options
}

#[test]
fn dirty_refresh_returns_complete_registered_state() {
    let first = tempfile::tempdir().expect("first tempdir");
    let second = tempfile::tempdir().expect("second tempdir");
    let first_script = first.path().join("First.jsx");
    let second_script = second.path().join("Second.jsx");
    std::fs::write(
        &first_script,
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.first
// Version: 1.0.0
// SCRIPTMETA-END
alert('first');
"#,
    )
    .expect("first script");
    std::fs::write(
        &second_script,
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.second
// Version: 1.0.0
// SCRIPTMETA-END
alert('second');
"#,
    )
    .expect("second script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    engine
        .scan_root_paths(
            vec![first.path().to_path_buf(), second.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("initial scan");

    std::fs::write(
        &first_script,
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.first
// Version: 1.0.1
// SCRIPTMETA-END
alert('first updated');
"#,
    )
    .expect("first script updated");

    engine
        .mark_changed_paths(scriptmetakit::RawChangeBatch {
            paths: vec![first_script],
            overflowed: false,
        })
        .expect("mark changed");
    let result = engine
        .refresh_dirty_roots(scriptmetakit::RefreshRequest {
            mode: scriptmetakit::ScanMode::FileListAndMetadata,
        })
        .expect("refresh dirty roots");

    assert_eq!(result.roots.len(), 2);
    assert_eq!(result.file_list_snapshots.len(), 2);
    let catalog = result.catalog_snapshot.as_ref().expect("catalog");
    let script_ids: std::collections::BTreeSet<_> = catalog
        .all_items
        .iter()
        .map(|item| item.script_id.as_ref())
        .collect();
    assert!(script_ids.contains("com.example.first"));
    assert!(script_ids.contains("com.example.second"));
    assert_eq!(
        catalog
            .all_items
            .iter()
            .find(|item| item.script_id == "com.example.first")
            .and_then(|item| item.version.as_deref()),
        Some("1.0.1")
    );
}

#[test]
fn interrupted_dirty_refresh_keeps_old_catalog_and_retries_without_a_new_event() {
    let temp = tempfile::tempdir().expect("tempdir");
    let scripts = temp.path().join("Scripts");
    std::fs::create_dir(&scripts).expect("scripts directory");
    let script = scripts.join("Example.jsx");
    std::fs::write(
        &script,
        "// SCRIPTMETA-BEGIN\n// Script-ID: com.example.retry\n// Version: 1.0.0\n// SCRIPTMETA-END\n",
    )
    .expect("initial script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "DirtyRetry"),
    )
    .expect("engine");
    engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("initial scan");

    std::fs::write(
        &script,
        "// SCRIPTMETA-BEGIN\n// Script-ID: com.example.retry\n// Version: 1.0.1\n// SCRIPTMETA-END\n",
    )
    .expect("updated script");
    engine
        .mark_changed_paths(scriptmetakit::RawChangeBatch {
            paths: vec![script],
            overflowed: false,
        })
        .expect("mark changed");
    engine.config_mut().scanner.max_nodes_per_root = 0;

    let interrupted = engine
        .refresh_dirty_roots(scriptmetakit::RefreshRequest {
            mode: scriptmetakit::ScanMode::FileListAndMetadata,
        })
        .expect("interrupted refresh");
    assert_eq!(
        interrupted.operation.status,
        scriptmetakit::OperationStatus::Partial
    );
    assert!(
        interrupted
            .roots
            .iter()
            .any(|root| { root.status == scriptmetakit::RootStatus::Overflowed && root.is_dirty })
    );
    assert_eq!(
        interrupted
            .catalog_snapshot
            .as_ref()
            .and_then(|catalog| catalog.all_items.first())
            .and_then(|item| item.version.as_deref()),
        Some("1.0.0")
    );

    engine.config_mut().scanner.max_nodes_per_root = 10_000;
    let retried = engine
        .refresh_dirty_roots(scriptmetakit::RefreshRequest {
            mode: scriptmetakit::ScanMode::FileListAndMetadata,
        })
        .expect("retry");
    assert_eq!(
        retried.operation.status,
        scriptmetakit::OperationStatus::Finished
    );
    assert!(retried.roots.iter().all(|root| !root.is_dirty));
    assert_eq!(
        retried
            .catalog_snapshot
            .as_ref()
            .and_then(|catalog| catalog.all_items.first())
            .and_then(|item| item.version.as_deref()),
        Some("1.0.1")
    );
}

#[test]
fn dirty_refresh_falls_back_when_cached_tree_misses_new_script_directory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let empty_directory = temp.path().join("Empty");
    std::fs::create_dir_all(&empty_directory).expect("empty directory");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "StaleCache"),
    )
    .expect("engine");
    let initial = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("initial scan");
    assert!(
        !initial
            .flattened_file_entries()
            .any(|entry| entry.display_path == empty_directory)
    );

    let script_path = empty_directory.join("New.jsx");
    std::fs::write(
        &script_path,
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.stale-cache.new
// Version: 1.0.0
// SCRIPTMETA-END
alert('new');
"#,
    )
    .expect("new script");
    let expected_script_path = script_path.canonicalize().expect("canonical script");

    engine
        .mark_changed_paths(scriptmetakit::RawChangeBatch {
            paths: vec![script_path],
            overflowed: false,
        })
        .expect("mark changed");
    let refreshed = engine
        .refresh_dirty_roots(scriptmetakit::RefreshRequest {
            mode: scriptmetakit::ScanMode::FileListAndMetadata,
        })
        .expect("refresh");

    assert!(
        refreshed
            .flattened_file_entries()
            .any(|entry| entry.resolved_path == expected_script_path)
    );
    let catalog = refreshed.catalog_snapshot.as_ref().expect("catalog");
    assert!(
        catalog
            .file_items
            .iter()
            .any(|item| item.script_id == "com.example.stale-cache.new")
    );
    let added = refreshed
        .change_summary
        .as_ref()
        .expect("change summary")
        .changes
        .iter()
        .any(|change| {
            change.kind == scriptmetakit::FileEntryChangeKind::Added
                && change.resolved_path == expected_script_path
        });
    assert!(added);
}

#[test]
fn dirty_refresh_preserves_update_results_for_unchanged_items_only() {
    let first = tempfile::tempdir().expect("first tempdir");
    let second = tempfile::tempdir().expect("second tempdir");
    let dist = tempfile::tempdir().expect("dist tempdir");
    let first_dist = dist.path().join("first.txt");
    let second_dist = dist.path().join("second.txt");
    let first_dist_url = url::Url::from_file_path(&first_dist).expect("first dist url");
    let second_dist_url = url::Url::from_file_path(&second_dist).expect("second dist url");
    std::fs::write(
        &first_dist,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.first
Latest-Version: 2.0.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("first dist");
    std::fs::write(
        &second_dist,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.second
Latest-Version: 2.0.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("second dist");

    let first_script = first.path().join("First.jsx");
    let second_script = second.path().join("Second.jsx");
    std::fs::write(
        &first_script,
        format!(
            r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.first
// Version: 1.0.0
// Meta-URL: {first_dist_url}
// SCRIPTMETA-END
alert('first');
"#
        ),
    )
    .expect("first script");
    std::fs::write(
        &second_script,
        format!(
            r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.second
// Version: 1.0.0
// Meta-URL: {second_dist_url}
// SCRIPTMETA-END
alert('second');
"#
        ),
    )
    .expect("second script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    let scan_result = engine
        .scan_root_paths(
            vec![first.path().to_path_buf(), second.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("initial scan");
    let items = scan_result
        .catalog_snapshot
        .as_ref()
        .expect("catalog")
        .all_items
        .clone();
    let first_item_id = first_script.to_string_lossy().into_owned();
    let second_item_id = second_script.to_string_lossy().into_owned();
    pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest { items }))
        .expect("update check");

    std::fs::write(
        &first_script,
        format!(
            r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.first
// Version: 1.1.0
// Meta-URL: {first_dist_url}
// SCRIPTMETA-END
alert('first updated');
"#
        ),
    )
    .expect("first script updated");
    engine
        .mark_changed_paths(scriptmetakit::RawChangeBatch {
            paths: vec![first_script],
            overflowed: false,
        })
        .expect("mark changed");
    let result = engine
        .refresh_dirty_roots(scriptmetakit::RefreshRequest {
            mode: scriptmetakit::ScanMode::FileListAndMetadata,
        })
        .expect("refresh dirty roots");
    let update_result = result
        .update_check_result
        .as_ref()
        .expect("preserved update result");

    assert!(
        !update_result
            .statuses_by_item_id
            .contains_key(&first_item_id)
    );
    assert_eq!(
        update_result
            .statuses_by_item_id
            .get(&second_item_id)
            .copied(),
        Some(scriptmetakit::UpdateStatus::UpdateAvailable)
    );
}

fn contains_file(entries: &[scriptmetakit::FileSystemEntry], path: &std::path::Path) -> bool {
    entries.iter().any(|entry| {
        (!entry.is_directory && entry.resolved_path == path) || contains_file(&entry.children, path)
    })
}

fn find_file_entry<'a>(
    entries: &'a [scriptmetakit::FileSystemEntry],
    path: &std::path::Path,
) -> Option<&'a scriptmetakit::FileSystemEntry> {
    let resolved_path = path.canonicalize().expect("canonical file path");
    entries.iter().find_map(|entry| {
        if !entry.is_directory && entry.resolved_path == resolved_path {
            Some(entry)
        } else {
            find_file_entry(&entry.children, path)
        }
    })
}

fn contains_directory(entries: &[scriptmetakit::FileSystemEntry], path: &std::path::Path) -> bool {
    let resolved_path = path.canonicalize().expect("canonical directory path");
    entries.iter().any(|entry| {
        (entry.is_directory && entry.resolved_path == resolved_path)
            || contains_directory(&entry.children, path)
    })
}

fn contains_runtime_kind(
    entries: &[scriptmetakit::FileSystemEntry],
    file_name: &str,
    runtime_kind: scriptmetakit::ScriptRuntimeKind,
) -> bool {
    entries.iter().any(|entry| {
        (!entry.is_directory
            && entry
                .display_path
                .file_name()
                .and_then(|name| name.to_str())
                == Some(file_name)
            && entry.runtime_kind == Some(runtime_kind))
            || contains_runtime_kind(&entry.children, file_name, runtime_kind)
    })
}

fn count_files(entries: &[scriptmetakit::FileSystemEntry]) -> usize {
    entries
        .iter()
        .map(|entry| {
            if entry.is_directory {
                count_files(&entry.children)
            } else {
                1
            }
        })
        .sum()
}

#[cfg(target_os = "macos")]
fn create_macos_alias(target: &std::path::Path, alias_path: &std::path::Path, is_directory: bool) {
    use objc2_foundation::{NSURL, NSURLBookmarkCreationOptions};

    let target_url = if is_directory {
        NSURL::from_directory_path(target)
    } else {
        NSURL::from_file_path(target)
    }
    .expect("target NSURL");
    let alias_url = NSURL::from_file_path(alias_path).expect("alias NSURL");
    let bookmark_data = target_url
        .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            NSURLBookmarkCreationOptions::SuitableForBookmarkFile,
            None,
            None,
        )
        .expect("bookmark data");
    NSURL::writeBookmarkData_toURL_options_error(&bookmark_data, &alias_url, 0)
        .expect("write alias");
}

#[test]
fn update_check_does_not_fork_catalog_snapshot_while_scan_result_is_held() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script = temp.path().join("Held.jsx");
    std::fs::write(
        &script,
        r#"
// SCRIPTMETA-BEGIN
// Script-ID: com.example.held
// Version: 1.0.0
// SCRIPTMETA-END
alert('held');
"#,
    )
    .expect("script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    let scan_result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");
    let held_catalog = scan_result
        .catalog_snapshot
        .as_ref()
        .expect("catalog")
        .clone();
    let items = held_catalog.all_items.clone();

    pollster::block_on(engine.check_updates_for_items(&items)).expect("update check");
    let engine_catalog = engine.catalog_snapshot().expect("engine catalog");

    assert!(std::sync::Arc::ptr_eq(&held_catalog, &engine_catalog));
}

#[test]
fn saves_and_loads_cache_payload() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache_path = temp.path().join("cache").join("catalog.json");
    let payload = scriptmetakit::CachePayload::new(
        scriptmetakit::CacheScope::Catalog,
        serde_json::json!({
            "roots": [],
            "all_items": []
        }),
    );

    scriptmetakit::save_cache_payload(&cache_path, &payload).expect("save cache");
    let loaded = scriptmetakit::load_cache_payload(&cache_path).expect("load cache");

    assert_eq!(loaded.scope, scriptmetakit::CacheScope::Catalog);
    assert_eq!(loaded.schema.schema_version, payload.schema.schema_version);
    assert_eq!(loaded.data, payload.data);
}

#[test]
fn persistent_catalog_cache_rejects_mismatched_roots() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root_a = temp.path().join("A");
    let root_b = temp.path().join("B");
    std::fs::create_dir_all(&root_a).expect("root a");
    std::fs::create_dir_all(&root_b).expect("root b");
    std::fs::write(
        root_a.join("Tool.jsx"),
        "/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.cache.root\nSCRIPTMETA-END\n*/\n",
    )
    .expect("script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "CacheRoot"),
    )
    .expect("engine");
    engine
        .scan_root_paths(vec![root_a], scriptmetakit::ScanMode::FileListAndMetadata)
        .expect("scan");
    let payload = engine
        .export_cache(scriptmetakit::CacheScope::Catalog)
        .expect("export cache");

    let mut next_engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "CacheRoot"),
    )
    .expect("next engine");
    next_engine
        .set_root_paths(
            vec![root_b],
            scriptmetakit::RootPurpose::FileListAndMetadata,
        )
        .expect("set roots");

    let error = next_engine
        .load_cache(payload)
        .expect_err("mismatched roots");
    assert!(error.to_string().contains("root signature"));
}

#[test]
fn reads_script_metadata_edit_draft_with_unknown_lines() {
    let source = "/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.edit.read\nVersion=v1. 2\nUnknown-Key=keep\nDescription-BEGIN\nLine 1\nLine 2\nDescription-END\nSCRIPTMETA-END\n*/\nalert('x');\n";

    let result = scriptmetakit::read_script_metadata_draft_from_text(
        source,
        std::path::Path::new("Tool.jsx"),
    )
    .expect("read draft");

    assert!(result.has_existing_block);
    assert_eq!(result.draft.script_id, "com.example.edit.read");
    assert_eq!(result.draft.version.as_deref(), Some("1.2"));
    assert_eq!(result.draft.description.as_deref(), Some("Line 1\nLine 2"));
    assert_eq!(result.unknown_lines, ["Unknown-Key=keep"]);
    assert!(
        result
            .existing_lines
            .iter()
            .any(|line| line == "Unknown-Key=keep")
    );
    assert_eq!(
        result.comment_style,
        scriptmetakit::ScriptMetaCommentStyle::JavaScriptBlock
    );
    assert!(!result.source_fingerprint.is_empty());
}

#[test]
fn reads_script_metadata_inside_jsdoc_header() {
    let source = "/**\n * Illustrator Ruby GUI Script\n * 使い方:\n * 1. テキストフレームを選択\n \n\nSCRIPTMETA-BEGIN\nScript-ID=local.yamo.Illustrator_Ruby_GUI_2y0r4yzmq57t\nName=Illustratorでルビを振る\nDescription=https://github.com/akiyo-as/illustrator_ruby_GUI\nSCRIPTMETA-END\n\n*/\nalert('x');\n";

    let metadata = scriptmetakit::parse_script_metadata(source).expect("metadata");
    assert_eq!(
        metadata.script_id,
        "local.yamo.Illustrator_Ruby_GUI_2y0r4yzmq57t"
    );
    assert_eq!(metadata.name.as_deref(), Some("Illustratorでルビを振る"));
    assert_eq!(
        metadata.description.as_deref(),
        Some("https://github.com/akiyo-as/illustrator_ruby_GUI")
    );

    let draft = scriptmetakit::read_script_metadata_draft_from_text(
        source,
        std::path::Path::new("Illustrator Ruby GUI.jsx"),
    )
    .expect("read draft");
    assert!(draft.has_existing_block);
    assert_eq!(
        draft.draft.script_id,
        "local.yamo.Illustrator_Ruby_GUI_2y0r4yzmq57t"
    );
}

#[test]
fn scans_script_metadata_inside_jsdoc_header() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Illustrator Ruby GUI.jsx");
    std::fs::write(
        &script_path,
        "/**\n * Illustrator Ruby GUI Script\n * 使い方:\n \n\nSCRIPTMETA-BEGIN\nScript-ID=local.yamo.Illustrator_Ruby_GUI_2y0r4yzmq57t\nName=Illustratorでルビを振る\nDescription=https://github.com/akiyo-as/illustrator_ruby_GUI\nSCRIPTMETA-END\n\n*/\nalert('x');\n",
    )
    .expect("write script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "JSDoc"),
    )
    .expect("engine");
    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");
    let catalog = result.catalog_snapshot.expect("catalog");
    let items = &catalog.all_items;
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].script_id,
        "local.yamo.Illustrator_Ruby_GUI_2y0r4yzmq57t"
    );
    assert_eq!(items[0].name.as_deref(), Some("Illustratorでルビを振る"));
}

#[test]
fn file_entries_and_changes_include_identity() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Tool.jsx");
    std::fs::write(&script_path, "alert('tool');").expect("script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Identity"),
    )
    .expect("engine");
    let initial = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("initial scan");
    let entry = initial
        .flattened_file_entries()
        .find(|entry| !entry.is_directory)
        .expect("file entry");
    assert!(entry.identity.is_some());

    std::fs::write(&script_path, "alert('changed');").expect("modify");
    engine
        .mark_changed_paths(scriptmetakit::RawChangeBatch {
            paths: vec![script_path],
            overflowed: false,
        })
        .expect("mark changed");
    let refreshed = engine
        .refresh_dirty_roots(scriptmetakit::RefreshRequest {
            mode: scriptmetakit::ScanMode::FileListAndMetadata,
        })
        .expect("refresh");
    let change = refreshed
        .change_summary
        .as_ref()
        .and_then(|summary| summary.changes.first())
        .expect("change");
    assert!(change.identity.is_some());
}

#[test]
fn structured_watch_events_report_change_kinds_and_ignored_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Tool.jsx");
    std::fs::write(&script_path, "alert('tool');").expect("script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "WatchEvents"),
    )
    .expect("engine");
    engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("initial scan");

    std::fs::write(&script_path, "alert('changed');").expect("modify");
    let events = engine
        .mark_changed_paths(scriptmetakit::RawChangeBatch {
            paths: vec![script_path],
            overflowed: false,
        })
        .expect("mark modified");
    let batch = events
        .into_iter()
        .find_map(|event| match event {
            scriptmetakit::ScriptMetaKitEvent::ChangeDetected { batch } => Some(batch),
            _ => None,
        })
        .expect("change batch");
    assert_eq!(
        batch.events[0].kind,
        scriptmetakit::WatchPathEventKind::Modified
    );
    assert_eq!(
        batch.affected_roots[0].rescan_targets[0].path,
        temp.path().canonicalize().expect("canonical temp root")
    );

    let ignored = engine
        .mark_changed_paths(scriptmetakit::RawChangeBatch {
            paths: vec![temp.path().join(".DS_Store")],
            overflowed: false,
        })
        .expect("mark ignored");
    let ignored_batch = ignored
        .into_iter()
        .find_map(|event| match event {
            scriptmetakit::ScriptMetaKitEvent::ChangeDetected { batch } => Some(batch),
            _ => None,
        })
        .expect("ignored batch");
    assert_eq!(
        ignored_batch.ignored_paths[0].reason,
        scriptmetakit::WatchIgnoreReason::HiddenPath
    );

    let old_path = temp.path().join("Old.jsx");
    let new_path = temp.path().join("New.jsx");
    std::fs::write(&old_path, "alert('old');").expect("old");
    engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("rescan with old");
    std::fs::remove_file(&old_path).expect("remove old");
    std::fs::write(&new_path, "alert('new');").expect("new");
    let rename_events = engine
        .mark_changed_paths(scriptmetakit::RawChangeBatch {
            paths: vec![old_path, new_path],
            overflowed: false,
        })
        .expect("mark rename-like batch");
    let rename_batch = rename_events
        .into_iter()
        .find_map(|event| match event {
            scriptmetakit::ScriptMetaKitEvent::ChangeDetected { batch } => Some(batch),
            _ => None,
        })
        .expect("rename batch");
    assert!(!rename_batch.rename_candidates.is_empty());
}

#[test]
fn checks_file_url_update_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dist_path = temp.path().join("SCRIPTMETA.txt");
    let dist_url = url::Url::from_file_path(&dist_path).expect("file url");
    std::fs::write(
        &dist_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.script
Latest-Version: 3.0.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("dist");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![scriptmetakit::ScriptMetaItem {
            root_id: "scripts".into(),
            file_path: temp.path().join("Example.jsx"),
            identity_path: temp.path().join("Example.jsx"),
            runtime_kind: None,
            shebang: None,
            script_id: "com.example.script".to_string(),
            version: Some("2.0.0".into()),
            description: None,
            target_app: None,
            min_target_version: None,
            meta_url: Some(dist_url),
            name: Some("Example".into()),
            author: None,
            release_date: None,
            edit_password_sha256: None,
            has_scriptmeta: true,
            has_scriptmeta_edit_password: false,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: false,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: scriptmetakit::ScriptMetaEditState::Unknown,
        }
        .into()],
    }))
    .expect("update check");

    assert_eq!(
        result.statuses_by_item_id.values().next().copied(),
        Some(scriptmetakit::UpdateStatus::UpdateAvailable)
    );
}

#[test]
fn single_item_update_check_merges_into_cached_result() {
    let temp = tempfile::tempdir().expect("tempdir");
    let first_dist_path = temp.path().join("first-SCRIPTMETA.txt");
    let second_dist_path = temp.path().join("second-SCRIPTMETA.txt");
    let first_dist_url = url::Url::from_file_path(&first_dist_path).expect("first file url");
    let second_dist_url = url::Url::from_file_path(&second_dist_path).expect("second file url");
    std::fs::write(
        &first_dist_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.single.first
Latest-Version: 2.0.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("first dist");
    std::fs::write(
        &second_dist_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.single.second
Latest-Version: 2.0.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("second dist");

    let first_path = temp.path().join("First.jsx");
    let second_path = temp.path().join("Second.jsx");
    let first_item: scriptmetakit::ScriptMetaItemRef = scriptmetakit::ScriptMetaItem {
        root_id: "scripts".into(),
        file_path: first_path.clone(),
        identity_path: first_path.clone(),
        runtime_kind: None,
        shebang: None,
        script_id: "com.example.single.first".to_string(),
        version: Some("1.0.0".into()),
        description: None,
        target_app: None,
        min_target_version: None,
        meta_url: Some(first_dist_url),
        name: Some("First".into()),
        author: None,
        release_date: None,
        edit_password_sha256: None,
        has_scriptmeta: true,
        has_scriptmeta_edit_password: false,
        is_file_locked: false,
        is_read_only: false,
        can_edit_scriptmeta: false,
        can_append_scriptmeta: false,
        scriptmeta_edit_state: scriptmetakit::ScriptMetaEditState::Unknown,
    }
    .into();
    let second_item: scriptmetakit::ScriptMetaItemRef = scriptmetakit::ScriptMetaItem {
        root_id: "scripts".into(),
        file_path: second_path.clone(),
        identity_path: second_path.clone(),
        runtime_kind: None,
        shebang: None,
        script_id: "com.example.single.second".to_string(),
        version: Some("1.0.0".into()),
        description: None,
        target_app: None,
        min_target_version: None,
        meta_url: Some(second_dist_url),
        name: Some("Second".into()),
        author: None,
        release_date: None,
        edit_password_sha256: None,
        has_scriptmeta: true,
        has_scriptmeta_edit_password: false,
        is_file_locked: false,
        is_read_only: false,
        can_edit_scriptmeta: false,
        can_append_scriptmeta: false,
        scriptmeta_edit_state: scriptmetakit::ScriptMetaEditState::Unknown,
    }
    .into();
    let first_item_id = first_path.to_string_lossy().into_owned();
    let second_item_id = second_path.to_string_lossy().into_owned();
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    pollster::block_on(engine.check_updates_for_items(&[first_item.clone(), second_item]))
        .expect("initial update check");

    std::fs::write(
        &first_dist_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.single.first
Latest-Version: 1.0.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("first dist updated");
    let single_result =
        pollster::block_on(engine.check_update_for_item(first_item)).expect("single update check");

    assert_eq!(single_result.statuses_by_item_id.len(), 1);
    assert_eq!(
        single_result
            .statuses_by_item_id
            .get(&first_item_id)
            .copied(),
        Some(scriptmetakit::UpdateStatus::UpToDate)
    );

    let cached_result = engine.update_check_result().expect("cached result");
    assert_eq!(
        cached_result
            .statuses_by_item_id
            .get(&first_item_id)
            .copied(),
        Some(scriptmetakit::UpdateStatus::UpToDate)
    );
    assert_eq!(
        cached_result
            .statuses_by_item_id
            .get(&second_item_id)
            .copied(),
        Some(scriptmetakit::UpdateStatus::UpdateAvailable)
    );
}

#[test]
fn checks_file_url_update_metadata_after_large_prefix() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dist_path = temp.path().join("large-page.txt");
    let dist_url = url::Url::from_file_path(&dist_path).expect("file url");
    let mut source = String::with_capacity(300_000);
    source.extend(std::iter::repeat_n('x', 280_000));
    source.push_str(
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.large.page
Latest-Version: 7.0.0
SCRIPTMETA-DIST-END
"#,
    );
    std::fs::write(&dist_path, source).expect("dist");

    let script_path = temp.path().join("Large.jsx");
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![scriptmetakit::ScriptMetaItem {
            root_id: "scripts".into(),
            file_path: script_path.clone(),
            identity_path: script_path,
            runtime_kind: None,
            shebang: None,
            script_id: "com.example.large.page".to_string(),
            version: Some("6.0.0".into()),
            description: None,
            target_app: None,
            min_target_version: None,
            meta_url: Some(dist_url),
            name: Some("Large".into()),
            author: None,
            release_date: None,
            edit_password_sha256: None,
            has_scriptmeta: true,
            has_scriptmeta_edit_password: false,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: false,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: scriptmetakit::ScriptMetaEditState::Unknown,
        }
        .into()],
    }))
    .expect("update check");

    assert_eq!(
        result.statuses_by_item_id.values().next().copied(),
        Some(scriptmetakit::UpdateStatus::UpdateAvailable)
    );
}

#[test]
fn returns_structured_update_failure_data() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dist_path = temp.path().join("SCRIPTMETA.txt");
    let dist_url = url::Url::from_file_path(&dist_path).expect("file url");
    std::fs::write(
        &dist_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.script
SCRIPTMETA-DIST-END
"#,
    )
    .expect("dist");

    let script_path = temp.path().join("Example.jsx");
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![scriptmetakit::ScriptMetaItem {
            root_id: "scripts".into(),
            file_path: script_path.clone(),
            identity_path: script_path.clone(),
            runtime_kind: None,
            shebang: None,
            script_id: "com.example.script".to_string(),
            version: Some("2.0.0".into()),
            description: None,
            target_app: None,
            min_target_version: None,
            meta_url: Some(dist_url.clone()),
            name: Some("Example".into()),
            author: None,
            release_date: None,
            edit_password_sha256: None,
            has_scriptmeta: true,
            has_scriptmeta_edit_password: false,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: false,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: scriptmetakit::ScriptMetaEditState::Unknown,
        }
        .into()],
    }))
    .expect("update check");

    let item_id = script_path.to_string_lossy().into_owned();
    let failure = result
        .failures_by_item_id
        .get(&item_id)
        .expect("failure data");
    assert_eq!(failure.code, "unresolved_distribution");
    assert_eq!(failure.message, "Latest-Version was not found");
    assert_eq!(failure.script_id, "com.example.script");
    assert_eq!(failure.current_version.as_deref(), Some("2.0.0"));
    assert_eq!(failure.meta_url.as_ref(), Some(&dist_url));
    assert_eq!(failure.source_url.as_ref(), Some(&dist_url));
    assert_eq!(
        result.statuses_by_item_id.get(&item_id).copied(),
        Some(scriptmetakit::UpdateStatus::Failed)
    );
    assert_eq!(
        result.errors_by_item_id.get(&item_id).map(String::as_str),
        Some("Latest-Version was not found")
    );
}

#[test]
fn scan_result_reports_operation_summary_and_file_issues() {
    let temp = tempfile::tempdir().expect("tempdir");
    let obfuscated_path = temp.path().join("Obfuscated.jsxbin");
    let readonly_path = temp.path().join("ReadOnly.jsx");
    std::fs::write(&obfuscated_path, "@JSXBIN@ES@2.0@script").expect("jsxbin script");
    std::fs::write(&readonly_path, "alert('readonly');").expect("readonly script");
    let original_permissions = std::fs::metadata(&readonly_path)
        .expect("readonly metadata")
        .permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(&readonly_path, permissions).expect("set readonly");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Issues"),
    )
    .expect("engine");
    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");

    std::fs::set_permissions(&readonly_path, original_permissions).expect("restore permissions");

    assert_eq!(
        result.operation.status,
        scriptmetakit::OperationStatus::Finished
    );
    assert_eq!(result.operation.total_units, 1);
    let entries = result.file_list_snapshots[0]
        .children
        .as_deref()
        .unwrap_or_default();
    let obfuscated_entry = find_file_entry(entries, &obfuscated_path).expect("obfuscated entry");
    assert_eq!(
        obfuscated_entry.scriptmeta_edit_state,
        scriptmetakit::ScriptMetaEditState::Obfuscated
    );
    assert!(
        result
            .file_issues
            .iter()
            .any(|issue| issue.path == readonly_path && issue.code == "read_only")
    );
}

#[cfg(unix)]
#[test]
fn scan_result_reports_permission_denied_file_issue() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let unreadable_path = temp.path().join("Unreadable.jsx");
    std::fs::write(
        &unreadable_path,
        "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.unreadable\n// SCRIPTMETA-END\n",
    )
    .expect("unreadable script");
    let original_permissions = std::fs::metadata(&unreadable_path)
        .expect("metadata")
        .permissions();
    let mut blocked_permissions = original_permissions.clone();
    blocked_permissions.set_mode(0o000);
    std::fs::set_permissions(&unreadable_path, blocked_permissions).expect("chmod 000");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "PermissionIssue"),
    )
    .expect("engine");
    let result = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("scan");

    std::fs::set_permissions(&unreadable_path, original_permissions).expect("restore permissions");

    assert!(
        result
            .file_issues
            .iter()
            .any(|issue| issue.path == unreadable_path && issue.code == "permission_denied"),
        "expected permission_denied issue, got {:?}",
        result.file_issues
    );
}

#[cfg(unix)]
#[test]
fn dirty_file_list_refresh_preserves_previous_subtree_after_nested_read_error() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let blocked_directory = temp.path().join("Blocked");
    std::fs::create_dir(&blocked_directory).expect("blocked directory");
    let script_path = blocked_directory.join("Example.jsx");
    std::fs::write(
        &script_path,
        "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.blocked\n// SCRIPTMETA-END\n",
    )
    .expect("script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "NestedReadError"),
    )
    .expect("engine");
    let initial = engine
        .scan_root_paths(
            vec![temp.path().to_path_buf()],
            scriptmetakit::ScanMode::FileListAndMetadata,
        )
        .expect("initial scan");
    assert!(
        initial
            .flattened_file_entries()
            .any(|entry| entry.display_path == script_path)
    );

    let original_permissions = std::fs::metadata(&blocked_directory)
        .expect("metadata")
        .permissions();
    let mut blocked_permissions = original_permissions.clone();
    blocked_permissions.set_mode(0o000);
    std::fs::set_permissions(&blocked_directory, blocked_permissions).expect("chmod 000");

    engine
        .mark_changed_paths(scriptmetakit::RawChangeBatch {
            paths: vec![blocked_directory.clone()],
            overflowed: false,
        })
        .expect("mark changed");
    let refreshed = engine
        .refresh_dirty_roots(scriptmetakit::RefreshRequest {
            mode: scriptmetakit::ScanMode::FileListAndMetadata,
        })
        .expect("refresh");

    std::fs::set_permissions(&blocked_directory, original_permissions)
        .expect("restore permissions");

    assert_eq!(
        refreshed.operation.status,
        scriptmetakit::OperationStatus::Partial
    );
    assert!(
        refreshed
            .roots
            .iter()
            .any(|root| { root.status == scriptmetakit::RootStatus::Unreadable && root.is_dirty }),
        "unexpected roots: {:?}",
        refreshed.roots
    );
    assert!(
        refreshed
            .flattened_file_entries()
            .any(|entry| entry.display_path == script_path),
        "the previous subtree must remain visible after a transient read failure"
    );
}

#[cfg(unix)]
#[test]
fn full_metadata_rescan_preserves_last_good_catalog_after_permission_error() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let container = temp.path().join("Container");
    let root = container.join("Scripts");
    std::fs::create_dir_all(&root).expect("root");
    let script = root.join("Example.jsx");
    std::fs::write(
        &script,
        "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.stale\n// SCRIPTMETA-END\n",
    )
    .expect("script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "TransactionalMetadata"),
    )
    .expect("engine");
    engine
        .set_roots(vec![
            scriptmetakit::RootRegistration::file_list_and_metadata("root", &root),
        ])
        .expect("roots");
    let initial = engine
        .scan_roots(scriptmetakit::ScanRequest::all(
            scriptmetakit::ScanMode::MetadataOnly,
        ))
        .expect("initial scan");
    assert_eq!(
        initial.catalog_snapshot.expect("catalog").file_items.len(),
        1
    );

    let original_permissions = std::fs::metadata(&container)
        .expect("container metadata")
        .permissions();
    let mut blocked_permissions = original_permissions.clone();
    blocked_permissions.set_mode(0o000);
    std::fs::set_permissions(&container, blocked_permissions).expect("block container");

    let refreshed = engine
        .scan_roots(scriptmetakit::ScanRequest::all(
            scriptmetakit::ScanMode::MetadataOnly,
        ))
        .expect("failed scan still returns stale result");

    std::fs::set_permissions(&container, original_permissions).expect("restore permissions");
    let catalog = refreshed.catalog_snapshot.expect("stale catalog");
    assert_eq!(catalog.file_items.len(), 1);
    assert_eq!(catalog.file_items[0].script_id, "com.example.stale");
    assert!(
        refreshed
            .roots
            .iter()
            .any(|root| root.status == scriptmetakit::RootStatus::Unreadable && root.is_dirty),
        "unexpected roots: {:?}",
        refreshed.roots
    );
}

#[test]
fn update_check_cancellation_returns_partial_result() {
    let temp = tempfile::tempdir().expect("tempdir");
    let first_dist = temp.path().join("first.txt");
    let second_dist = temp.path().join("second.txt");
    std::fs::write(
        &first_dist,
        "SCRIPTMETA-DIST-BEGIN\nScript-ID=com.example.cancel.first\nLatest-Version=2.0.0\nSCRIPTMETA-DIST-END\n",
    )
    .expect("first dist");
    std::fs::write(
        &second_dist,
        "SCRIPTMETA-DIST-BEGIN\nScript-ID=com.example.cancel.second\nLatest-Version=2.0.0\nSCRIPTMETA-DIST-END\n",
    )
    .expect("second dist");

    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "CancelUpdate");
    config.update_check.max_concurrent_meta_url_checks = 1;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    let cancellation = engine.cancellation_token();
    let first_path = temp.path().join("First.jsx");
    let second_path = temp.path().join("Second.jsx");
    let result = pollster::block_on(engine.check_updates_with_progress(
        scriptmetakit::UpdateCheckRequest {
            items: vec![
                script_item_for_update(
                    "com.example.cancel.first",
                    "1.0.0",
                    &first_path,
                    url::Url::from_file_path(&first_dist).expect("first url"),
                ),
                script_item_for_update(
                    "com.example.cancel.second",
                    "1.0.0",
                    &second_path,
                    url::Url::from_file_path(&second_dist).expect("second url"),
                ),
            ],
        },
        |progress| {
            if progress.phase == scriptmetakit::UpdateCheckProgressPhase::FinishedItem {
                cancellation.cancel();
            }
        },
    ))
    .expect("update check");

    assert_eq!(
        result.operation.status,
        scriptmetakit::OperationStatus::Cancelled
    );
    assert_eq!(result.operation.total_units, 2);
    assert_eq!(
        result
            .statuses_by_item_id
            .get(&first_path.to_string_lossy().into_owned())
            .copied(),
        Some(scriptmetakit::UpdateStatus::UpdateAvailable)
    );
    assert_eq!(
        result
            .statuses_by_item_id
            .get(&second_path.to_string_lossy().into_owned())
            .copied(),
        Some(scriptmetakit::UpdateStatus::Cancelled)
    );
    let failure = result
        .failures_by_item_id
        .get(&second_path.to_string_lossy().into_owned())
        .expect("cancelled failure");
    assert_eq!(failure.code, "operation_cancelled");
}

#[test]
fn cancellation_before_composite_scan_prevents_follow_up_update_check() {
    let temp = tempfile::tempdir().expect("tempdir");
    let distribution = temp.path().join("distribution.txt");
    std::fs::write(
        &distribution,
        "SCRIPTMETA-DIST-BEGIN\nScript-ID=com.example.composite.cancel\nLatest-Version=2.0.0\nSCRIPTMETA-DIST-END\n",
    )
    .expect("distribution");
    let script = temp.path().join("Example.jsx");
    let distribution_url = url::Url::from_file_path(&distribution).expect("distribution URL");
    std::fs::write(
        &script,
        format!(
            "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.composite.cancel\n// Version=1.0.0\n// Meta-URL={}\n// SCRIPTMETA-END\n",
            distribution_url
        ),
    )
    .expect("script");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "CompositeCancellation"),
    )
    .expect("engine");
    engine
        .set_roots(vec![
            scriptmetakit::RootRegistration::file_list_and_metadata("root", temp.path()),
        ])
        .expect("roots");
    let cancellation = engine.cancellation_token();
    let reservation = cancellation.reserve_next_operation();
    cancellation.cancel_current_or_reserved();

    let (scan_result, update_result) = engine
        .scan_roots_and_check_updates(
            scriptmetakit::ScanRequest {
                root_ids: vec![],
                mode: scriptmetakit::ScanMode::FileListAndMetadata,
            },
            true,
        )
        .expect("composite scan");
    drop(reservation);

    assert_eq!(
        scan_result.operation.status,
        scriptmetakit::OperationStatus::Cancelled
    );
    assert!(update_result.is_none());
}

#[test]
fn checks_matching_entry_in_multi_script_distribution_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dist_path = temp.path().join("SCRIPTMETA.txt");
    let dist_url = url::Url::from_file_path(&dist_path).expect("file url");
    std::fs::write(
        &dist_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID=org.example.first
Version=1.0.0
Script-ID=org.example.second
Version=2.5.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("dist");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![scriptmetakit::ScriptMetaItem {
            root_id: "scripts".into(),
            file_path: temp.path().join("Second.jsx"),
            identity_path: temp.path().join("Second.jsx"),
            runtime_kind: None,
            shebang: None,
            script_id: "org.example.second".to_string(),
            version: Some("2.0.0".into()),
            description: None,
            target_app: None,
            min_target_version: None,
            meta_url: Some(dist_url),
            name: Some("Second".into()),
            author: None,
            release_date: None,
            edit_password_sha256: None,
            has_scriptmeta: true,
            has_scriptmeta_edit_password: false,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: false,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: scriptmetakit::ScriptMetaEditState::Unknown,
        }
        .into()],
    }))
    .expect("update check");

    let resolution = result
        .resolutions_by_item_id
        .values()
        .next()
        .expect("resolution");
    assert_eq!(resolution.latest_version.as_deref(), Some("2.5.0"));
    assert_eq!(
        result.statuses_by_item_id.values().next().copied(),
        Some(scriptmetakit::UpdateStatus::UpdateAvailable)
    );
}

#[test]
fn reports_missing_entry_in_multi_script_distribution_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dist_path = temp.path().join("SCRIPTMETA.txt");
    let dist_url = url::Url::from_file_path(&dist_path).expect("file url");
    std::fs::write(
        &dist_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID=org.example.first
Version=1.0.0
Script-ID=org.example.second
Version=2.5.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("dist");

    let script_path = temp.path().join("Missing.jsx");
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![scriptmetakit::ScriptMetaItem {
            root_id: "scripts".into(),
            file_path: script_path.clone(),
            identity_path: script_path.clone(),
            runtime_kind: None,
            shebang: None,
            script_id: "org.example.missing".to_string(),
            version: Some("2.0.0".into()),
            description: None,
            target_app: None,
            min_target_version: None,
            meta_url: Some(dist_url.clone()),
            name: Some("Missing".into()),
            author: None,
            release_date: None,
            edit_password_sha256: None,
            has_scriptmeta: true,
            has_scriptmeta_edit_password: false,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: false,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: scriptmetakit::ScriptMetaEditState::Unknown,
        }
        .into()],
    }))
    .expect("update check");

    let item_id = script_path.to_string_lossy().into_owned();
    let expected_message =
        "distribution metadata for script id `org.example.missing` was not found";
    let failure = result
        .failures_by_item_id
        .get(&item_id)
        .expect("failure data");
    assert_eq!(failure.code, "unresolved_distribution");
    assert_eq!(failure.message, expected_message);
    assert_eq!(failure.script_id, "org.example.missing");
    assert_eq!(
        result.statuses_by_item_id.get(&item_id).copied(),
        Some(scriptmetakit::UpdateStatus::Failed)
    );
    assert_eq!(
        result.errors_by_item_id.get(&item_id).map(String::as_str),
        Some(expected_message)
    );
    let resolution = result
        .resolutions_by_item_id
        .get(&item_id)
        .expect("resolution");
    assert_eq!(resolution.latest_version.as_deref(), None);
    assert_eq!(resolution.note.as_deref(), Some(expected_message));
    assert_eq!(resolution.final_page_url, dist_url);
}

#[test]
fn follows_latest_url_chain_for_file_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let first_path = temp.path().join("first.txt");
    let second_path = temp.path().join("second.txt");
    let second_url = url::Url::from_file_path(&second_path).expect("second file url");
    std::fs::write(
        &first_path,
        format!(
            r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.script
Latest-URL: {second_url}
SCRIPTMETA-DIST-END
"#
        ),
    )
    .expect("first");
    std::fs::write(
        &second_path,
        r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.script
Latest-Version: 4.0.0
SCRIPTMETA-DIST-END
"#,
    )
    .expect("second");

    let first_url = url::Url::from_file_path(&first_path).expect("first file url");
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![scriptmetakit::ScriptMetaItem {
            root_id: "scripts".into(),
            file_path: temp.path().join("Example.jsx"),
            identity_path: temp.path().join("Example.jsx"),
            runtime_kind: None,
            shebang: None,
            script_id: "com.example.script".to_string(),
            version: Some("3.0.0".into()),
            description: None,
            target_app: None,
            min_target_version: None,
            meta_url: Some(first_url),
            name: Some("Example".into()),
            author: None,
            release_date: None,
            edit_password_sha256: None,
            has_scriptmeta: true,
            has_scriptmeta_edit_password: false,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: false,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: scriptmetakit::ScriptMetaEditState::Unknown,
        }
        .into()],
    }))
    .expect("update check");

    let resolution = result
        .resolutions_by_item_id
        .values()
        .next()
        .expect("resolution");
    assert_eq!(resolution.latest_version.as_deref(), Some("4.0.0"));
    assert_eq!(resolution.redirect_count, Some(1));
    assert_eq!(resolution.latest_url_history, vec![second_url]);
}

#[test]
fn latest_url_cycle_returns_structured_unresolved_result() {
    let temp = tempfile::tempdir().expect("tempdir");
    let first_path = temp.path().join("first.txt");
    let second_path = temp.path().join("second.txt");
    let first_url = url::Url::from_file_path(&first_path).expect("first file url");
    let second_url = url::Url::from_file_path(&second_path).expect("second file url");
    std::fs::write(
        &first_path,
        format!(
            r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.cycle
Latest-URL: {second_url}
SCRIPTMETA-DIST-END
"#
        ),
    )
    .expect("first");
    std::fs::write(
        &second_path,
        format!(
            r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.cycle
Latest-URL: {first_url}
SCRIPTMETA-DIST-END
"#
        ),
    )
    .expect("second");

    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![script_item_for_update(
            "com.example.cycle",
            "1.0.0",
            &temp.path().join("Cycle.jsx"),
            first_url.clone(),
        )],
    }))
    .expect("update check");

    assert_eq!(
        result.statuses_by_item_id.values().next().copied(),
        Some(scriptmetakit::UpdateStatus::Failed)
    );
    let resolution = result
        .resolutions_by_item_id
        .values()
        .next()
        .expect("resolution");
    assert!(resolution.is_unresolved);
    assert_eq!(
        resolution.note.as_deref(),
        Some("circular Latest-URL reference was ignored")
    );
    assert_eq!(resolution.latest_url_history, vec![second_url, first_url]);
}

#[cfg(target_os = "macos")]
fn compile_osa_source(source: &str, language: Option<&str>, output_path: &std::path::Path) -> bool {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    let mut command = Command::new("osacompile");
    if let Some(language) = language {
        command.arg("-l").arg(language);
    }
    command.arg("-o").arg(output_path).stdin(Stdio::piped());
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take()
        && stdin.write_all(source.as_bytes()).is_err()
    {
        let _ = child.kill();
        return false;
    }
    child.wait().is_ok_and(|status| status.success())
}

#[cfg(target_os = "macos")]
fn decompile_osa_source(path: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("osadecompile")
        .arg(path)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn script_item_for_update(
    script_id: &str,
    version: &str,
    script_path: &std::path::Path,
    meta_url: url::Url,
) -> scriptmetakit::ScriptMetaItemRef {
    scriptmetakit::ScriptMetaItem {
        root_id: "scripts".into(),
        file_path: script_path.to_path_buf(),
        identity_path: script_path.to_path_buf(),
        runtime_kind: None,
        shebang: None,
        script_id: script_id.to_string(),
        version: Some(version.to_string()),
        description: None,
        target_app: None,
        min_target_version: None,
        meta_url: Some(meta_url),
        name: Some(script_id.to_string()),
        author: None,
        release_date: None,
        edit_password_sha256: None,
        has_scriptmeta: true,
        has_scriptmeta_edit_password: false,
        is_file_locked: false,
        is_read_only: false,
        can_edit_scriptmeta: false,
        can_append_scriptmeta: false,
        scriptmeta_edit_state: scriptmetakit::ScriptMetaEditState::Unknown,
    }
    .into()
}

#[cfg(feature = "blocking-http")]
fn http_test_guard() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};

    static HTTP_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    HTTP_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(feature = "blocking-http")]
#[test]
fn checks_http_update_metadata() {
    let _http_test_guard = http_test_guard();
    let response_body = r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.http
Latest-Version: 5.0.0
SCRIPTMETA-DIST-END
"#;
    let url = spawn_http_once(response_body);
    let temp = tempfile::tempdir().expect("tempdir");
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "Test"),
    )
    .expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![scriptmetakit::ScriptMetaItem {
            root_id: "scripts".into(),
            file_path: temp.path().join("Http.jsx"),
            identity_path: temp.path().join("Http.jsx"),
            runtime_kind: None,
            shebang: None,
            script_id: "com.example.http".to_string(),
            version: Some("4.0.0".into()),
            description: None,
            target_app: None,
            min_target_version: None,
            meta_url: Some(url),
            name: Some("HTTP".into()),
            author: None,
            release_date: None,
            edit_password_sha256: None,
            has_scriptmeta: true,
            has_scriptmeta_edit_password: false,
            is_file_locked: false,
            is_read_only: false,
            can_edit_scriptmeta: false,
            can_append_scriptmeta: false,
            scriptmeta_edit_state: scriptmetakit::ScriptMetaEditState::Unknown,
        }
        .into()],
    }))
    .expect("update check");

    assert_eq!(
        result.statuses_by_item_id.values().next().copied(),
        Some(scriptmetakit::UpdateStatus::UpdateAvailable)
    );
}

#[cfg(feature = "blocking-http")]
#[test]
fn retries_http_update_metadata_after_transient_failure() {
    let _http_test_guard = http_test_guard();
    let response_body = r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.http.retry
Latest-Version: 5.0.0
SCRIPTMETA-DIST-END
"#;
    let url = spawn_http_fail_then_success(response_body);
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Retry.jsx");
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "Retry");
    config.update_check.retry_attempts = 1;
    config.update_check.retry_initial_delay_millis = 1;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![script_item_for_update(
            "com.example.http.retry",
            "4.0.0",
            &script_path,
            url,
        )],
    }))
    .expect("update check");

    assert_eq!(
        result.statuses_by_item_id.values().next().copied(),
        Some(scriptmetakit::UpdateStatus::UpdateAvailable)
    );
    assert_eq!(
        result.operation.status,
        scriptmetakit::OperationStatus::Finished
    );
}

#[cfg(feature = "blocking-http")]
#[test]
fn batch_update_reuses_one_http_source_for_items_with_the_same_meta_url() {
    let _http_test_guard = http_test_guard();
    let response_body = r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.shared.first
Latest-Version: 2.0.0
Script-ID: com.example.shared.second
Latest-Version: 3.0.0
SCRIPTMETA-DIST-END
"#;
    let url = spawn_http_once(response_body);
    let temp = tempfile::tempdir().expect("tempdir");
    let first_path = temp.path().join("First.jsx");
    let second_path = temp.path().join("Second.jsx");
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(
        scriptmetakit::ScriptMetaKitConfig::new("Test", "SharedUpdateSource"),
    )
    .expect("engine");

    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![
            script_item_for_update(
                "com.example.shared.first",
                "1.0.0",
                &first_path,
                url.clone(),
            ),
            script_item_for_update("com.example.shared.second", "1.0.0", &second_path, url),
        ],
    }))
    .expect("update check");

    assert!(
        result
            .statuses_by_item_id
            .values()
            .all(|status| { *status == scriptmetakit::UpdateStatus::UpdateAvailable })
    );
}

#[cfg(feature = "blocking-http")]
#[test]
fn batch_update_shares_terminal_http_failure_and_retry_budget_by_meta_url() {
    let _http_test_guard = http_test_guard();
    use std::sync::atomic::Ordering;

    let (url, requests, stop, server) = spawn_counted_http_error();
    let temp = tempfile::tempdir().expect("tempdir");
    let first_path = temp.path().join("FirstFailure.jsx");
    let second_path = temp.path().join("SecondFailure.jsx");
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "SharedFailure");
    config.update_check.retry_attempts = 2;
    config.update_check.retry_initial_delay_millis = 1;
    config.update_check.retry_backoff_multiplier = 1;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");

    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![
            script_item_for_update(
                "com.example.shared.failure.first",
                "1.0.0",
                &first_path,
                url.clone(),
            ),
            script_item_for_update(
                "com.example.shared.failure.second",
                "1.0.0",
                &second_path,
                url,
            ),
        ],
    }))
    .expect("failed update check still returns a result");

    stop.store(true, Ordering::Release);
    server.join().expect("HTTP server");
    assert_eq!(requests.load(Ordering::Acquire), 3);
    assert_eq!(result.failures_by_item_id.len(), 2);
    assert!(
        result
            .statuses_by_item_id
            .values()
            .all(|status| *status == scriptmetakit::UpdateStatus::Failed)
    );
}

#[cfg(feature = "blocking-http")]
#[test]
fn terminal_http_404_is_not_retried() {
    let _http_test_guard = http_test_guard();
    use std::sync::atomic::Ordering;

    let (url, requests, stop, server) = spawn_counted_http_status("404 Not Found");
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("NotFound.jsx");
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "Terminal404");
    config.update_check.retry_attempts = 2;
    config.update_check.retry_initial_delay_millis = 1;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");

    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![script_item_for_update(
            "com.example.terminal.404",
            "1.0.0",
            &script_path,
            url,
        )],
    }))
    .expect("failed update check still returns a result");

    stop.store(true, Ordering::Release);
    server.join().expect("HTTP server");
    assert_eq!(requests.load(Ordering::Acquire), 1);
    assert_eq!(result.failures_by_item_id.len(), 1);
}

#[cfg(feature = "blocking-http")]
#[test]
fn transient_http_timeout_uses_only_the_configured_request_budget() {
    let _http_test_guard = http_test_guard();
    use std::sync::atomic::Ordering;

    let (url, requests, stop, server) = spawn_counted_stalled_http();
    let temp = tempfile::tempdir().expect("tempdir");
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "TimeoutBudget");
    config.update_check.retry_attempts = 2;
    config.update_check.retry_initial_delay_millis = 1;
    config.update_check.retry_backoff_multiplier = 1;
    config.update_check.request_timeout_millis = Some(30);
    config.update_check.resource_timeout_millis = Some(30);
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![script_item_for_update(
            "com.example.timeout.budget",
            "1.0.0",
            &temp.path().join("Timeout.jsx"),
            url,
        )],
    }))
    .expect("failed update check still returns a result");

    stop.store(true, Ordering::Release);
    server.join().expect("HTTP server");
    assert_eq!(requests.load(Ordering::Acquire), 3);
    assert_eq!(result.failures_by_item_id.len(), 1);
}

#[cfg(unix)]
#[test]
fn permission_denied_file_meta_url_is_terminal_without_retry() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("SCRIPTMETA.txt");
    std::fs::write(
        &source,
        "SCRIPTMETA-DIST-BEGIN\nScript-ID: com.example.permission\nLatest-Version: 2.0.0\nSCRIPTMETA-DIST-END\n",
    )
    .expect("source");
    let original_permissions = std::fs::metadata(&source).expect("metadata").permissions();
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o000))
        .expect("block source");
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "PermissionTerminal");
    config.update_check.retry_attempts = 2;
    config.update_check.retry_initial_delay_millis = 1;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    let mut retries = 0;
    let result = pollster::block_on(engine.check_updates_with_progress(
        scriptmetakit::UpdateCheckRequest {
            items: vec![script_item_for_update(
                "com.example.permission",
                "1.0.0",
                &temp.path().join("Permission.jsx"),
                url::Url::from_file_path(&source).expect("file URL"),
            )],
        },
        |progress| {
            if progress.phase == scriptmetakit::UpdateCheckProgressPhase::Retrying {
                retries += 1;
            }
        },
    ))
    .expect("failed update check still returns a result");
    std::fs::set_permissions(&source, original_permissions).expect("restore source");

    assert_eq!(retries, 0);
    assert_eq!(result.failures_by_item_id.len(), 1);
}

#[cfg(feature = "blocking-http")]
#[test]
fn retry_after_zero_is_respected_within_the_shared_retry_budget() {
    let _http_test_guard = http_test_guard();
    use std::sync::atomic::Ordering;

    let (url, requests, stop, server) =
        spawn_counted_http_response("429 Too Many Requests", "Retry-After: 0\r\n", "");
    let temp = tempfile::tempdir().expect("tempdir");
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "RetryAfter");
    config.update_check.retry_attempts = 2;
    config.update_check.retry_initial_delay_millis = 1;
    config.update_check.retry_backoff_multiplier = 1;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![script_item_for_update(
            "com.example.retry.after",
            "1.0.0",
            &temp.path().join("RetryAfter.jsx"),
            url,
        )],
    }))
    .expect("failed update check still returns a result");

    stop.store(true, Ordering::Release);
    server.join().expect("HTTP server");
    assert_eq!(requests.load(Ordering::Acquire), 3);
    assert_eq!(result.failures_by_item_id.len(), 1);
}

#[cfg(feature = "blocking-http")]
#[test]
fn retry_after_above_the_configured_limit_does_not_repeat_the_request() {
    let _http_test_guard = http_test_guard();
    use std::sync::atomic::Ordering;

    let (url, requests, stop, server) =
        spawn_counted_http_response("503 Service Unavailable", "Retry-After: 60\r\n", "");
    let temp = tempfile::tempdir().expect("tempdir");
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "RetryAfterLimit");
    config.update_check.retry_attempts = 2;
    config.update_check.retry_initial_delay_millis = 1;
    config.update_check.max_retry_delay_millis = 50;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![script_item_for_update(
            "com.example.retry.after.limit",
            "1.0.0",
            &temp.path().join("RetryAfterLimit.jsx"),
            url,
        )],
    }))
    .expect("failed update check still returns a result");

    stop.store(true, Ordering::Release);
    server.join().expect("HTTP server");
    assert_eq!(requests.load(Ordering::Acquire), 1);
    assert_eq!(result.failures_by_item_id.len(), 1);
}

#[cfg(feature = "blocking-http")]
#[test]
fn downstream_latest_url_requests_are_singleflighted() {
    let _http_test_guard = http_test_guard();
    use std::sync::atomic::Ordering;

    let body = r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.converged.first
Latest-Version: 2.0.0
Script-ID: com.example.converged.second
Latest-Version: 3.0.0
SCRIPTMETA-DIST-END
"#;
    let (downstream_url, requests, stop, server) =
        spawn_counted_http_response("200 OK", "Content-Type: text/plain\r\n", body);
    let temp = tempfile::tempdir().expect("tempdir");
    let first_source = temp.path().join("First-SCRIPTMETA.txt");
    let second_source = temp.path().join("Second-SCRIPTMETA.txt");
    std::fs::write(
        &first_source,
        format!(
            "SCRIPTMETA-DIST-BEGIN\nScript-ID: com.example.converged.first\nLatest-URL: {downstream_url}\nSCRIPTMETA-DIST-END\n"
        ),
    )
    .expect("first source");
    std::fs::write(
        &second_source,
        format!(
            "SCRIPTMETA-DIST-BEGIN\nScript-ID: com.example.converged.second\nLatest-URL: {downstream_url}\nSCRIPTMETA-DIST-END\n"
        ),
    )
    .expect("second source");
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "DownstreamSingleflight");
    config.update_check.max_concurrent_meta_url_checks = 2;
    config.update_check.retry_attempts = 0;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    let result = pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
        items: vec![
            script_item_for_update(
                "com.example.converged.first",
                "1.0.0",
                &temp.path().join("First.jsx"),
                url::Url::from_file_path(first_source).expect("first URL"),
            ),
            script_item_for_update(
                "com.example.converged.second",
                "1.0.0",
                &temp.path().join("Second.jsx"),
                url::Url::from_file_path(second_source).expect("second URL"),
            ),
        ],
    }))
    .expect("update check");

    stop.store(true, Ordering::Release);
    server.join().expect("HTTP server");
    assert_eq!(requests.load(Ordering::Acquire), 1);
    assert_eq!(
        result.statuses_by_item_id.len(),
        2,
        "unexpected singleflight result: {result:#?}"
    );
    assert!(
        result
            .statuses_by_item_id
            .values()
            .all(|status| *status == scriptmetakit::UpdateStatus::UpdateAvailable),
        "unexpected singleflight result: {result:#?}"
    );
}

#[cfg(feature = "blocking-http")]
#[test]
fn repeated_update_check_uses_http_validator_and_cached_body() {
    let _http_test_guard = http_test_guard();
    use std::sync::atomic::Ordering;

    let body = r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.conditional
Latest-Version: 2.0.0
SCRIPTMETA-DIST-END
"#;
    for validator_header in [
        "ETag: \"metadata-v1\"\r\n",
        "Last-Modified: Wed, 21 Oct 2015 07:28:00 GMT\r\n",
    ] {
        let (url, requests, body_responses, stop, server) =
            spawn_conditional_http_response(validator_header, body);
        let temp = tempfile::tempdir().expect("tempdir");
        let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "ConditionalHTTP");
        config.update_check.retry_attempts = 0;
        let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");

        for _ in 0..2 {
            let result =
                pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
                    items: vec![script_item_for_update(
                        "com.example.conditional",
                        "1.0.0",
                        &temp.path().join("Conditional.jsx"),
                        url.clone(),
                    )],
                }))
                .expect("conditional update check");
            assert!(
                result
                    .statuses_by_item_id
                    .values()
                    .all(|status| *status == scriptmetakit::UpdateStatus::UpdateAvailable),
                "unexpected conditional HTTP result: {result:#?}"
            );
        }

        stop.store(true, Ordering::Release);
        server.join().expect("HTTP server");
        assert_eq!(requests.load(Ordering::Acquire), 2);
        assert_eq!(
            body_responses.load(Ordering::Acquire),
            1,
            "validator was not reused: {validator_header:?}"
        );
    }
}

#[cfg(feature = "blocking-http")]
#[test]
fn update_retry_backoff_is_cancelled_without_an_extra_request() {
    let _http_test_guard = http_test_guard();
    use std::{sync::atomic::Ordering, time::Duration};

    let (url, requests, stop, server) = spawn_counted_http_error();
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("CancelBackoff.jsx");
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "CancelBackoff");
    config.update_check.retry_attempts = 2;
    config.update_check.retry_initial_delay_millis = 10_000;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    let cancellation = engine.cancellation_token();
    let started = std::time::Instant::now();

    let result = pollster::block_on(engine.check_updates_with_progress(
        scriptmetakit::UpdateCheckRequest {
            items: vec![script_item_for_update(
                "com.example.cancel.backoff",
                "1.0.0",
                &script_path,
                url,
            )],
        },
        |progress| {
            if progress.phase == scriptmetakit::UpdateCheckProgressPhase::Retrying {
                cancellation.cancel();
            }
        },
    ))
    .expect("cancelled update check returns a result");

    stop.store(true, Ordering::Release);
    server.join().expect("HTTP server");
    assert_eq!(requests.load(Ordering::Acquire), 1);
    assert_eq!(
        result.operation.status,
        scriptmetakit::OperationStatus::Cancelled
    );
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn batch_update_shares_terminal_file_failure_and_retry_progress_by_meta_url() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing_source = temp.path().join("Missing-SCRIPTMETA.txt");
    let source_url = url::Url::from_file_path(&missing_source).expect("file URL");
    let first_path = temp.path().join("FirstMissing.jsx");
    let second_path = temp.path().join("SecondMissing.jsx");
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "SharedFileFailure");
    config.update_check.retry_attempts = 2;
    config.update_check.retry_initial_delay_millis = 1;
    config.update_check.retry_backoff_multiplier = 1;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    let mut retry_progress_count = 0usize;

    let result = pollster::block_on(engine.check_updates_with_progress(
        scriptmetakit::UpdateCheckRequest {
            items: vec![
                script_item_for_update(
                    "com.example.shared.file.first",
                    "1.0.0",
                    &first_path,
                    source_url.clone(),
                ),
                script_item_for_update(
                    "com.example.shared.file.second",
                    "1.0.0",
                    &second_path,
                    source_url,
                ),
            ],
        },
        |progress| {
            if progress.phase == scriptmetakit::UpdateCheckProgressPhase::Retrying {
                retry_progress_count += 1;
            }
        },
    ))
    .expect("failed update check still returns a result");

    assert_eq!(retry_progress_count, 2);
    assert_eq!(result.failures_by_item_id.len(), 2);
}

#[cfg(feature = "blocking-http")]
#[test]
fn cancellation_interrupts_an_active_http_request() {
    let _http_test_guard = http_test_guard();
    use std::{
        thread,
        time::{Duration, Instant},
    };

    let (url, request_started) = spawn_stalled_http();
    let temp = tempfile::tempdir().expect("tempdir");
    let script_path = temp.path().join("Cancelled.jsx");
    let mut config = scriptmetakit::ScriptMetaKitConfig::new("Test", "CancelledHTTP");
    config.update_check.request_timeout_millis = Some(10_000);
    config.update_check.resource_timeout_millis = Some(10_000);
    config.update_check.retry_attempts = 0;
    let mut engine = scriptmetakit::ScriptMetaKitEngine::new(config).expect("engine");
    let cancellation = engine.cancellation_token();

    let update_thread = thread::spawn(move || {
        pollster::block_on(engine.check_updates(scriptmetakit::UpdateCheckRequest {
            items: vec![script_item_for_update(
                "com.example.http.cancelled",
                "1.0.0",
                &script_path,
                url,
            )],
        }))
    });
    request_started
        .recv_timeout(Duration::from_secs(2))
        .expect("HTTP request started");

    let cancelled_at = Instant::now();
    cancellation.cancel();
    let result = update_thread
        .join()
        .expect("update thread")
        .expect("cancelled update result");

    assert!(
        cancelled_at.elapsed() < Duration::from_secs(1),
        "active HTTP cancellation took {:?}",
        cancelled_at.elapsed()
    );
    assert_eq!(
        result.operation.status,
        scriptmetakit::OperationStatus::Cancelled
    );
}

#[cfg(feature = "blocking-http")]
fn read_http_request_headers(stream: &mut std::net::TcpStream) -> Vec<u8> {
    use std::io::Read;

    const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    while request.len() < MAX_REQUEST_HEADER_BYTES {
        let read = stream.read(&mut chunk).unwrap_or_default();
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    request
}

#[cfg(feature = "blocking-http")]
fn has_complete_http_request_headers(request: &[u8]) -> bool {
    request.windows(4).any(|window| window == b"\r\n\r\n")
}

#[cfg(feature = "blocking-http")]
fn spawn_http_once(body: &'static str) -> url::Url {
    use std::{io::Write, net::TcpListener, thread};

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let address = listener.local_addr().expect("local addr");
    thread::spawn(move || {
        loop {
            let (mut stream, _) = listener.accept().expect("accept");
            let request = read_http_request_headers(&mut stream);
            if !has_complete_http_request_headers(&request) {
                continue;
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
            break;
        }
    });
    url::Url::parse(&format!("http://{address}/SCRIPTMETA.txt")).expect("server url")
}

#[cfg(feature = "blocking-http")]
fn spawn_http_fail_then_success(body: &'static str) -> url::Url {
    use std::{io::Write, net::TcpListener, thread};

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let address = listener.local_addr().expect("local addr");
    thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            drop(stream);
        }
        loop {
            let (mut stream, _) = listener.accept().expect("accept retry");
            let request = read_http_request_headers(&mut stream);
            if !has_complete_http_request_headers(&request) {
                continue;
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
            break;
        }
    });
    url::Url::parse(&format!("http://{address}/SCRIPTMETA.txt")).expect("server url")
}

#[cfg(feature = "blocking-http")]
fn spawn_counted_http_error() -> (
    url::Url,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    spawn_counted_http_status("503 Service Unavailable")
}

#[cfg(feature = "blocking-http")]
fn spawn_counted_http_status(
    status: &'static str,
) -> (
    url::Url,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    spawn_counted_http_response(status, "", "")
}

#[cfg(feature = "blocking-http")]
fn spawn_counted_http_response(
    status: &'static str,
    headers: &'static str,
    body: &'static str,
) -> (
    url::Url,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    use std::{
        io::Write,
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        thread,
        time::Duration,
    };

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    listener
        .set_nonblocking(true)
        .expect("nonblocking test server");
    let address = listener.local_addr().expect("local addr");
    let requests = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let server_requests = Arc::clone(&requests);
    let server_stop = Arc::clone(&stop);
    let server = thread::spawn(move || {
        while !server_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let request = read_http_request_headers(&mut stream);
                    if !has_complete_http_request_headers(&request) {
                        continue;
                    }
                    server_requests.fetch_add(1, Ordering::AcqRel);
                    let response = format!(
                        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("write error response");
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("accept error: {error}"),
            }
        }
    });
    (
        url::Url::parse(&format!("http://{address}/SCRIPTMETA.txt")).expect("server url"),
        requests,
        stop,
        server,
    )
}

#[cfg(feature = "blocking-http")]
fn spawn_conditional_http_response(
    validator_header: &'static str,
    body: &'static str,
) -> (
    url::Url,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    use std::{
        io::Write,
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        thread,
        time::Duration,
    };

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind conditional test server");
    listener
        .set_nonblocking(true)
        .expect("nonblocking conditional test server");
    let address = listener.local_addr().expect("local addr");
    let requests = Arc::new(AtomicUsize::new(0));
    let body_responses = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let server_requests = Arc::clone(&requests);
    let server_body_responses = Arc::clone(&body_responses);
    let server_stop = Arc::clone(&stop);
    let server = thread::spawn(move || {
        while !server_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let request = read_http_request_headers(&mut stream);
                    if !has_complete_http_request_headers(&request) {
                        continue;
                    }
                    let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
                    server_requests.fetch_add(1, Ordering::AcqRel);
                    let conditional = request.contains("if-none-match: \"metadata-v1\"")
                        || request.contains("if-modified-since:");
                    let response = if conditional {
                        format!("HTTP/1.0 304 Not Modified\r\n{validator_header}\r\n")
                    } else {
                        server_body_responses.fetch_add(1, Ordering::AcqRel);
                        format!(
                            "HTTP/1.1 200 OK\r\n{validator_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    };
                    stream
                        .write_all(response.as_bytes())
                        .expect("write conditional response");
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("accept error: {error}"),
            }
        }
    });
    (
        url::Url::parse(&format!("http://{address}/SCRIPTMETA.txt")).expect("server url"),
        requests,
        body_responses,
        stop,
        server,
    )
}

#[cfg(feature = "blocking-http")]
fn spawn_stalled_http() -> (url::Url, std::sync::mpsc::Receiver<()>) {
    use std::{io::Read, net::TcpListener, sync::mpsc, thread, time::Duration};

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stalled test server");
    let address = listener.local_addr().expect("local addr");
    let (started_sender, started_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept stalled request");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let _ = started_sender.send(());
        thread::sleep(Duration::from_secs(3));
    });
    (
        url::Url::parse(&format!("http://{address}/SCRIPTMETA.txt")).expect("server url"),
        started_receiver,
    )
}

#[cfg(feature = "blocking-http")]
fn spawn_counted_stalled_http() -> (
    url::Url,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    use std::{
        io::Read,
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        thread,
        time::Duration,
    };

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stalled test server");
    listener.set_nonblocking(true).expect("nonblocking");
    let address = listener.local_addr().expect("address");
    let requests = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let server_requests = Arc::clone(&requests);
    let server_stop = Arc::clone(&stop);
    let server = thread::spawn(move || {
        while !server_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    server_requests.fetch_add(1, Ordering::AcqRel);
                    thread::spawn(move || {
                        let mut request = [0_u8; 1024];
                        let _ = stream.read(&mut request);
                        thread::sleep(Duration::from_millis(200));
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("accept error: {error}"),
            }
        }
    });
    (
        url::Url::parse(&format!("http://{address}/SCRIPTMETA.txt")).expect("URL"),
        requests,
        stop,
        server,
    )
}
