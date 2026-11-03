use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EngineConfigurationChange {
    pub changed: bool,
    pub metadata_invalidated: bool,
    pub refresh_required: bool,
    pub persistence_changed: bool,
    pub watcher_changed: bool,
}

impl ScriptMetaKitEngine {
    /// Validates a full configuration before changing any engine state.
    pub fn apply_configuration(
        &mut self,
        mut config: ScriptMetaKitConfig,
    ) -> ScriptMetaKitResult<EngineConfigurationChange> {
        config.supported_extensions =
            crate::scanner::ExtensionPolicy::new(config.supported_extensions.extensions());
        // Engine scans use the parser limit. Keep the legacy scanner copy in sync.
        config.scanner.max_prefix_bytes = config.parser.max_prefix_bytes;
        validate_config(&config)?;
        self.config = config;
        self.configuration_dirty = true;
        let change = self.synchronize_configuration();
        self.expire_idle_memory_cache();
        Ok(change)
    }

    pub fn validate_configuration(config: &ScriptMetaKitConfig) -> ScriptMetaKitResult<()> {
        validate_config(config)
    }

    pub(super) fn synchronize_configuration(&mut self) -> EngineConfigurationChange {
        if !self.configuration_dirty {
            return EngineConfigurationChange::default();
        }
        self.configuration_dirty = false;
        if self.config == self.applied_config {
            return EngineConfigurationChange::default();
        }
        let previous = std::mem::replace(&mut self.applied_config, self.config.clone());
        let metadata_invalidated = previous.app_id != self.config.app_id
            || previous.cache_namespace != self.config.cache_namespace
            || previous.read_context() != self.config.read_context()
            || previous.parser.preferred_languages != self.config.parser.preferred_languages;
        let persistence_changed = previous.cache.enabled != self.config.cache.enabled
            || previous.cache.persistent_cache != self.config.cache.persistent_cache;
        let watcher_changed = previous.watcher != self.config.watcher
            || previous.scanner != self.config.scanner
            || previous.supported_extensions != self.config.supported_extensions;
        let refresh_required = metadata_invalidated || previous.scanner != self.config.scanner;

        if metadata_invalidated {
            self.invalidate_cache(
                CacheScope::All,
                CacheInvalidationReason {
                    code: "configuration_changed".into(),
                    message: "metadata read configuration changed; reload source files".into(),
                },
            );
        } else if refresh_required {
            // Limits and preflight settings affect the next scan, but must not
            // discard the last good result if that scan is interrupted.
            self.mark_roots_for_full_refresh();
        }
        if previous.scanner.resolve_macos_alias != self.config.scanner.resolve_macos_alias
            || previous.scanner.follow_symlinks != self.config.scanner.follow_symlinks
        {
            self.resolved_watch_targets.clear();
            self.resolved_watch_sources.clear();
        }

        if !self.persistent_cache_enabled() {
            self.pending_file_list_persistence.clear();
            self.pending_catalog_persistence = None;
            self.pending_update_check_persistence = None;
        }
        if persistence_changed {
            self.persisted_file_list_revisions.clear();
            self.catalog_persistence_is_current = false;
        }
        if previous.cache.preserve_update_results && !self.config.cache.preserve_update_results {
            self.update_check_result = None;
            self.pending_update_check_persistence = None;
        }
        if !self.config.update_check.enabled || !self.config.update_check.cache_network_responses {
            self.http_validation_cache.clear();
        }
        if !self.memory_cache_enabled() {
            self.mark_resident_memory_cache_evicted();
            self.clear_memory_cache();
        } else {
            self.enforce_memory_node_limit();
        }
        self.rebuild_known_path_index();
        EngineConfigurationChange {
            changed: true,
            metadata_invalidated,
            refresh_required,
            persistence_changed,
            watcher_changed,
        }
    }

    pub(super) fn mark_roots_for_full_refresh(&mut self) {
        let root_ids: Vec<_> = self.roots.iter().map(|root| root.root_id.clone()).collect();
        for root_id in root_ids {
            let revision = self.issue_revision();
            if let Some(root) = self.root_snapshots.get_mut(&root_id) {
                root.state_revision = revision;
                root.is_dirty = true;
            }
            self.dirty_roots
                .entry(root_id)
                .or_default()
                .requires_full_rescan = true;
        }
    }
}
