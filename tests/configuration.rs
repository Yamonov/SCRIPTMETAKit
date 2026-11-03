use scriptmetakit::{
    CacheInvalidationReason, CacheScope, LanguagePreferences, RefreshRequest, RootRegistration,
    ScanMode, ScanRequest, ScriptMetaKitConfig, ScriptMetaKitEngine,
};
use std::{fs, sync::Arc};

fn fixture() -> (tempfile::TempDir, ScriptMetaKitEngine) {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("sample.jsx"), "/*\nSCRIPTMETA-BEGIN\nScript-ID=config-test\nMeta-Default-language=ja\nName=日本語\nName[en]=English\nSCRIPTMETA-END\n*/").unwrap();
    let mut config = ScriptMetaKitConfig::new("ConfigurationTests", "test");
    config.update_check.enabled = false;
    let mut engine = ScriptMetaKitEngine::new(config).unwrap();
    engine
        .set_roots(vec![RootRegistration::file_list_and_metadata(
            "root",
            directory.path(),
        )])
        .unwrap();
    (directory, engine)
}

fn request() -> ScanRequest {
    ScanRequest::all(ScanMode::FileListAndMetadata)
}

#[test]
fn rejected_configuration_leaves_live_configuration_and_cache_unchanged() {
    let (_dir, mut engine) = fixture();
    let first = engine.scan_roots(request()).unwrap();
    let before = engine.config().clone();
    let mut invalid = before.clone();
    invalid.parser.max_prefix_bytes = 0;
    invalid.parser.preferred_languages = LanguagePreferences::new(["en"]).unwrap();
    assert!(engine.apply_configuration(invalid).is_err());
    assert_eq!(*engine.config(), before);
    assert!(Arc::ptr_eq(
        first.catalog_snapshot.as_ref().unwrap(),
        engine
            .cached_scan_result(request())
            .catalog_snapshot
            .as_ref()
            .unwrap()
    ));
}

#[test]
fn direct_legacy_language_changes_are_reconciled_before_scan() {
    let (_dir, mut engine) = fixture();
    let first = engine.scan_roots(request()).unwrap();
    assert_eq!(
        first.catalog_snapshot.unwrap().all_items[0].name.as_deref(),
        Some("日本語")
    );
    engine.config_mut().parser.preferred_languages = LanguagePreferences::new(["en"]).unwrap();
    let second = engine.scan_roots(request()).unwrap();
    assert_eq!(
        second.catalog_snapshot.unwrap().all_items[0]
            .name
            .as_deref(),
        Some("English")
    );
}

#[test]
fn disabling_persistence_after_scan_releases_pending_metadata() {
    for disable_all in [false, true] {
        let (_dir, mut engine) = fixture();
        let first = engine.scan_roots(request()).unwrap();
        let weak = Arc::downgrade(&first.catalog_snapshot.as_ref().unwrap().file_items[0]);
        drop(first);
        if disable_all {
            engine.config_mut().cache.enabled = false;
        } else {
            engine.config_mut().cache.persistent_cache = false;
        }
        engine.config_mut().cache.idle_lifetime_millis = 0;
        drop(engine.cached_scan_result(request()));
        assert!(
            weak.upgrade().is_none(),
            "disabled persistence must release pending data"
        );
    }
}

#[test]
fn changing_metadata_configuration_marks_roots_for_refresh() {
    let (_dir, mut engine) = fixture();
    engine.scan_roots(request()).unwrap();
    let before_watch = engine.watch_plan();
    let mut config = engine.config().clone();
    config.parser.preferred_languages = LanguagePreferences::new(["en"]).unwrap();
    let change = engine.apply_configuration(config).unwrap();
    assert!(change.metadata_invalidated);
    assert_eq!(before_watch, engine.watch_plan());
    let refreshed = engine
        .refresh_dirty_roots(RefreshRequest {
            mode: ScanMode::FileListAndMetadata,
        })
        .unwrap();
    assert_eq!(
        refreshed.catalog_snapshot.unwrap().all_items[0]
            .name
            .as_deref(),
        Some("English")
    );
    assert!(refreshed.roots.iter().all(|root| !root.is_dirty));
}

#[test]
fn cache_scope_invalidation_keeps_watch_topology_and_can_be_refreshed() {
    for scope in [CacheScope::Catalog, CacheScope::FileList, CacheScope::All] {
        let (_dir, mut engine) = fixture();
        engine.scan_roots(request()).unwrap();
        let before_watch = engine.watch_plan();
        engine.invalidate_cache(
            scope,
            CacheInvalidationReason {
                code: "test".into(),
                message: "test".into(),
            },
        );
        assert_eq!(before_watch, engine.watch_plan());
        let result = engine
            .refresh_dirty_roots(RefreshRequest {
                mode: ScanMode::FileListAndMetadata,
            })
            .unwrap();
        assert_eq!(result.catalog_snapshot.unwrap().all_items.len(), 1);
        assert!(result.roots.iter().all(|root| !root.is_dirty));
    }
}

#[test]
fn cache_validation_includes_parser_scanner_and_extension_configuration() {
    let (_dir, mut engine) = fixture();
    engine.scan_roots(request()).unwrap();
    let payload = engine.export_cache(CacheScope::All).unwrap();
    payload.validate_for_config(engine.config()).unwrap();
    for variant in 0..4 {
        let mut changed = engine.config().clone();
        match variant {
            0 => changed.parser.max_prefix_bytes = 256,
            1 => changed.parser.require_closed_block = false,
            2 => changed.scanner.skip_hidden = !changed.scanner.skip_hidden,
            _ => changed.supported_extensions = scriptmetakit::ExtensionPolicy::new(["jsx"]),
        }
        assert!(payload.validate_for_config(&changed).is_err());
    }
    let mut old_projection = payload;
    old_projection.schema.metadata_projection_version = 3;
    old_projection.schema.read_context = None;
    assert!(old_projection.validate_for_config(engine.config()).is_err());
}
