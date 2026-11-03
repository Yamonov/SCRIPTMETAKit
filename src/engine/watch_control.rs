use super::*;

impl ScriptMetaKitEngine {
    /// Prepares native monitoring without copying catalog/path-index state.
    #[doc(hidden)]
    pub fn preview_root_watch_plan(
        &self,
        root_ids: &[RootId],
        paused: bool,
    ) -> ScriptMetaKitResult<WatchPlan> {
        self.validate_watch_root_ids(root_ids)?;
        let mut suspended = self.paused_watch_root_ids.clone();
        for root_id in root_ids {
            if paused {
                suspended.insert(root_id.clone());
            } else {
                suspended.remove(root_id);
            }
        }
        Ok(self.watch_plan_with_paused_roots(&suspended))
    }

    /// Checks that a recent source read is still available for watch publication.
    #[doc(hidden)]
    pub fn retain_reconciled_watch_roots(&mut self, root_ids: &mut BTreeSet<RootId>) {
        self.expire_idle_memory_cache();
        root_ids.retain(|id| {
            let Some(root) = self.root_by_id(id) else {
                return false;
            };
            !self.paused_watch_root_ids.contains(id)
                && !self.dirty_roots.contains_key(id)
                && self
                    .root_snapshots
                    .get(id)
                    .is_some_and(|snapshot| snapshot.status == RootStatus::Ready)
                && (!root.purpose.includes_file_list()
                    || self
                        .file_list_snapshots
                        .get(id)
                        .is_some_and(|snapshot| snapshot.children.is_some() && !snapshot.truncated))
                && (!root.purpose.includes_metadata()
                    || self.catalog_snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot.roots.iter().any(|root| &root.root_id == id)
                    }))
        });
    }

    /// Refreshes a progressive watch batch while honoring its recorded invalidation.
    #[doc(hidden)]
    pub fn refresh_selected_roots(
        &mut self,
        request: ScanRequest,
    ) -> ScriptMetaKitResult<ScanResult> {
        self.expire_idle_memory_cache();
        let _operation_scope = self.cancellation.begin_scope();
        let scopes = request
            .root_ids
            .iter()
            .filter_map(|id| {
                self.dirty_roots
                    .get(id)
                    .map(|dirty| (id.clone(), dirty.clone()))
            })
            .collect();
        self.scan_roots_inner(request, Some(&scopes))
    }
    /// Suspends event-driven reads for these registered root IDs. Other roots
    /// keep their policies and cached snapshots. Repeated pauses are idempotent.
    /// Explicit source scans remain available while watching is paused.
    ///
    /// Empty input or an unknown ID fails before changing any root.
    pub fn pause_root_watching(&mut self, root_ids: &[RootId]) -> ScriptMetaKitResult<()> {
        self.validate_watch_root_ids(root_ids)?;
        self.paused_watch_root_ids.extend(root_ids.iter().cloned());
        for root_id in root_ids {
            self.dirty_roots.remove(root_id);
        }
        Ok(())
    }

    /// Fully reloads these roots, then resumes their configured watch policies.
    /// Metadata is read even when file size, modification time and identity match
    /// the cached record. No network update checks are performed.
    ///
    /// An incomplete, missing, unreadable or cancelled result leaves all requested
    /// roots paused; inspect the returned root statuses before using the content.
    /// Empty input or an unknown ID fails before changing any root.
    pub fn resume_root_watching(&mut self, root_ids: &[RootId]) -> ScriptMetaKitResult<ScanResult> {
        self.prepare_root_watch_resume(root_ids)?;
        self.expire_idle_memory_cache();
        let _operation_scope = self.cancellation.begin_scope();
        let scopes = root_ids
            .iter()
            .map(|root_id| {
                (
                    root_id.clone(),
                    DirtyRootState {
                        requires_full_rescan: true,
                        ..Default::default()
                    },
                )
            })
            .collect();
        let result = self.scan_roots_inner(
            ScanRequest {
                root_ids: root_ids.to_vec(),
                mode: ScanMode::FileListAndMetadata,
            },
            Some(&scopes),
        );
        if !matches!(&result, Ok(result) if result.roots.iter().all(|root| root.status == RootStatus::Ready))
        {
            self.pause_root_watching(root_ids)?;
        }
        result
    }

    /// Whether event-driven reads for this registered root are suspended.
    #[must_use]
    pub fn is_root_watching_paused(&self, root_id: &RootId) -> bool {
        self.paused_watch_root_ids.contains(root_id)
    }

    // The native bridge arms monitoring before the synchronous reload. This
    // closes the interval between the final source read and watcher startup.
    #[doc(hidden)]
    pub fn prepare_root_watch_resume(&mut self, root_ids: &[RootId]) -> ScriptMetaKitResult<()> {
        self.validate_watch_root_ids(root_ids)?;
        for root_id in root_ids {
            self.paused_watch_root_ids.remove(root_id);
        }
        Ok(())
    }

    fn validate_watch_root_ids(&self, root_ids: &[RootId]) -> ScriptMetaKitResult<()> {
        if root_ids.is_empty() {
            return Err(ScriptMetaKitError::InvalidConfig(
                "at least one registered root ID is required".into(),
            ));
        }
        if let Some(root_id) = root_ids
            .iter()
            .find(|root_id| self.root_by_id(root_id).is_none())
        {
            return Err(ScriptMetaKitError::InvalidConfig(format!(
                "unknown root ID: {root_id}"
            )));
        }
        Ok(())
    }
}
