//! A read-only consumer example: supplies ja -> en -> fr explicitly, never OS settings.
use scriptmetakit::{
    RootRegistration, ScanMode, ScanRequest, ScriptMetaKitConfig, ScriptMetaKitEngine,
};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let folder = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/fixtures/localized")
        });
    let mut config = ScriptMetaKitConfig::new("LocalizedMetadataExample", "test");
    config.watcher.enabled = false;
    config.update_check.enabled = false;
    config.cache.persistent_cache = false;
    let mut engine = ScriptMetaKitEngine::new(config)?;
    engine.set_preferred_languages(["ja", "en", "fr"])?;
    engine.set_roots(vec![RootRegistration::file_list_and_metadata(
        "example", folder,
    )])?;
    let result = engine.scan_roots(ScanRequest::all(ScanMode::MetadataOnly))?;
    let items = result
        .catalog_snapshot
        .as_ref()
        .map(|catalog| catalog.file_items.as_slice())
        .unwrap_or_default();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "preferred_languages": result.preferred_languages.as_slice(),
            "items": items,
        }))?
    );
    Ok(())
}
