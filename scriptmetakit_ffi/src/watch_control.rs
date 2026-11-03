use super::*;

#[cfg(feature = "native-watch")]
pub(super) fn has_paused_watch_roots(engine: &ScriptMetaKitEngine) -> bool {
    engine
        .roots()
        .iter()
        .any(|root| engine.is_root_watching_paused(&root.root_id))
}

#[unsafe(no_mangle)]
/// Pauses event-driven reads for a nonempty array of registered root IDs.
/// Registered roots, their policies and cached content are retained.
///
/// # Safety
/// `engine` is live. `root_ids` contains `count` valid UTF-8 slices.
pub unsafe extern "C" fn smk_engine_pause_root_watching(
    engine: *mut SmkEngine,
    root_ids: *const SmkUtf8Slice,
    count: usize,
) -> SmkStatus {
    let mut notification = empty_pending_watch_notification();
    let status = ffi_guard(|| {
        let root_ids = root_ids_from_raw(root_ids, count)?;
        let mut state = engine_mut(engine)?;
        state.clear_error();
        if !root_ids.is_empty()
            && root_ids
                .iter()
                .all(|id| state.engine.is_root_watching_paused(id))
        {
            return Ok(());
        }
        notification =
            apply_root_watch_transition(&mut state, &root_ids, true).map_err(|message| {
                state.set_error(&message);
                (SmkStatus::EngineError, message)
            })?;
        notification = reconcile_root_watch_transition(&mut state).or(notification);
        Ok(())
    });
    emit_pending_watch_notification(notification);
    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

#[unsafe(no_mangle)]
/// Arms the configured monitoring before fully rereading the requested roots.
/// No network update checks are performed. Inspect returned root statuses:
/// incomplete reads leave every requested root paused for an explicit retry.
/// Existing watch streams remain open. Does not start a stopped global watcher.
///
/// # Safety
/// `engine` is live. `root_ids` contains `count` valid UTF-8 slices and count is
/// nonzero. `output` is writable; free its non-null value with `smk_scan_result_free`.
pub unsafe extern "C" fn smk_engine_resume_root_watching(
    engine: *mut SmkEngine,
    root_ids: *const SmkUtf8Slice,
    count: usize,
    output: *mut *mut SmkScanResult,
) -> SmkStatus {
    let mut notification = empty_pending_watch_notification();
    let status = ffi_guard(|| {
        let output = out_mut(output)?;
        *output = ptr::null_mut();
        let root_ids = root_ids_from_raw(root_ids, count)?;
        let mut state = engine_mut(engine)?;
        state.clear_error();
        notification =
            apply_root_watch_transition(&mut state, &root_ids, false).map_err(|message| {
                state.set_error(&message);
                (SmkStatus::EngineError, message)
            })?;
        let result = state.engine.resume_root_watching(&root_ids);
        #[cfg(feature = "native-watch")]
        if let Ok(result) = &result
            && state.watcher.is_some()
            && result
                .roots
                .iter()
                .all(|root| root.status == RootStatus::Ready)
        {
            // These reads occurred after monitoring was armed. A topology
            // change below clears this credit and requires reconciliation.
            state
                .watch_reconciled_root_ids
                .extend(root_ids.iter().cloned());
        }
        let next_notification = match synchronize_running_watch_plan(&mut state) {
            Ok(notification) => notification,
            Err(message) => {
                // Even if native recovery fails, logical routing must not read
                // the requested roots after a failed resume.
                let _ = state.engine.pause_root_watching(&root_ids);
                if let Ok(recovery) = synchronize_running_watch_plan(&mut state) {
                    notification = recovery.or(notification);
                }
                notification = reconcile_root_watch_transition(&mut state).or(notification);
                state.set_error(&message);
                return Err((SmkStatus::EngineError, message));
            }
        };
        notification = next_notification.or(notification);
        notification = reconcile_root_watch_transition(&mut state).or(notification);
        let result = result.map_err(|error| {
            let message = error.to_string();
            state.set_error(&message);
            (SmkStatus::EngineError, message)
        })?;
        *output = Box::into_raw(Box::new(SmkScanResult::from_scan_result(result, None)));
        Ok(())
    });
    emit_pending_watch_notification(notification);
    if status == SmkStatus::Panic {
        set_engine_error(engine, "panic crossed scriptmetakit_ffi boundary");
    }
    status
}

fn reconcile_root_watch_transition(state: &mut SmkEngineState) -> PendingWatchNotification {
    #[cfg(feature = "native-watch")]
    if state.watcher.is_some() {
        // Shared physical watches can keep the same topology when only one
        // logical root pauses. End any old progressive cycle and publish the
        // current coverage of the remaining roots through the existing stream.
        state.watch_reconcile_pending = true;
        state.watch_progressive_reconciliation_active = false;
        state.watch_progressive_pending_root_ids.clear();
        if let Some(plan) = &state.watch_plan {
            state
                .watch_reconciled_root_ids
                .retain(|id| plan.logical_roots.iter().any(|root| &root.root_id == id));
        }
        return watch_callback_notification(state);
    }
    let _ = state;
    None
}

fn apply_root_watch_transition(
    state: &mut SmkEngineState,
    root_ids: &[RootId],
    paused: bool,
) -> Result<PendingWatchNotification, String> {
    #[cfg(feature = "native-watch")]
    let prepared = if state.watcher.is_some() {
        let plan = state
            .engine
            .preview_root_watch_plan(root_ids, paused)
            .map_err(|error| error.to_string())?;
        let replacement = if state
            .watch_plan
            .as_ref()
            .is_none_or(|old| !native_watch_configuration_equal(old, &plan))
        {
            Some(native_watcher_for_plan(
                &plan,
                state.watch_callback,
                state.watch_context,
                true,
            )?)
        } else {
            None
        };
        Some((plan, replacement))
    } else {
        None
    };

    // Validation and native setup precede mutation. No catalog, path index or
    // candidate record is cloned merely to change the suspension set.
    if paused {
        state.engine.pause_root_watching(root_ids)
    } else {
        state.engine.prepare_root_watch_resume(root_ids)
    }
    .map_err(|error| error.to_string())?;

    #[cfg(feature = "native-watch")]
    if let Some((plan, replacement)) = prepared {
        if let Some(watcher) = replacement {
            replace_native_watcher(state, Some(watcher));
            state.watch_reconciled_root_ids.clear();
        } else if !state.watch_reconcile_pending && !state.watch_progressive_reconciliation_active {
            // Physical monitoring continued uninterrupted. Cached ready roots
            // need publication only; actual queued events are still processed.
            state
                .watch_reconciled_root_ids
                .extend(plan.logical_roots.iter().map(|root| root.root_id.clone()));
        }
        state.watch_plan = Some(plan);
    }
    Ok(empty_pending_watch_notification())
}

#[cfg(feature = "native-watch")]
pub(super) fn merge_watch_changes(left: &mut Option<RootChangeBatch>, mut right: RootChangeBatch) {
    let Some(left) = left else {
        *left = Some(right);
        return;
    };
    left.paths.append(&mut right.paths);
    left.paths.sort();
    left.paths.dedup();
    left.overflowed |= right.overflowed;
    left.events.append(&mut right.events);
    left.ignored_paths.append(&mut right.ignored_paths);
    left.rename_candidates.append(&mut right.rename_candidates);
    for mut change in right.affected_roots {
        if let Some(existing) = left
            .affected_roots
            .iter_mut()
            .find(|old| old.root_id == change.root_id)
        {
            existing
                .dirty_directories
                .append(&mut change.dirty_directories);
            existing.dirty_directories.sort();
            existing.dirty_directories.dedup();
            for target in change.rescan_targets {
                if !existing.rescan_targets.contains(&target) {
                    existing.rescan_targets.push(target);
                }
            }
            existing.metadata_may_have_changed |= change.metadata_may_have_changed;
            existing.requires_full_rescan |= change.requires_full_rescan;
        } else {
            left.affected_roots.push(change);
        }
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// `engine` is live, `root_id` is valid UTF-8, and `output` is writable.
pub unsafe extern "C" fn smk_engine_is_root_watching_paused(
    engine: *mut SmkEngine,
    root_id: SmkUtf8Slice,
    output: *mut u8,
) -> SmkStatus {
    ffi_guard(|| {
        let output = out_mut(output)?;
        let root_id = required_str_from_slice(root_id, "root ID")?;
        let state = engine_mut(engine)?;
        *output = bool_byte(state.engine.is_root_watching_paused(&RootId::from(root_id)));
        Ok(())
    })
}

#[cfg(all(test, feature = "native-watch"))]
mod tests {
    use super::*;
    struct Fixture {
        handle: *mut SmkEngine,
        directory: tempfile::TempDir,
    }
    impl Fixture {
        fn new(policy: Option<WatchReconciliationDelivery>) -> Self {
            let directory = tempfile::tempdir().unwrap();
            std::fs::write(
                directory.path().join("script.jsx"),
                "// SCRIPTMETA-BEGIN\n// Script-ID=reload\n// Name=One\n// SCRIPTMETA-END\n",
            )
            .unwrap();
            let mut handle = ptr::null_mut();
            // SAFETY: this fixture exclusively owns the engine and borrowed guards.
            unsafe {
                assert_eq!(smk_engine_create_default(&mut handle), SmkStatus::Ok);
                let mut state = engine_mut(handle).unwrap();
                state
                    .engine
                    .set_roots(vec![RootRegistration::file_list_and_metadata(
                        "root",
                        directory.path(),
                    )])
                    .unwrap();
                if let Some(policy) = policy {
                    start_watching_engine(&mut state, None, ptr::null_mut(), policy).unwrap();
                    poll_watcher_scan(&mut state, false).unwrap();
                }
            }
            Self { handle, directory }
        }
        fn resume(&self) {
            let id = SmkUtf8Slice {
                ptr: b"root".as_ptr(),
                len: 4,
            };
            let mut result = ptr::null_mut();
            // SAFETY: input and output storage outlive the calls; result is owned.
            unsafe {
                assert_eq!(
                    smk_engine_pause_root_watching(self.handle, &id, 1),
                    SmkStatus::Ok
                );
                assert_eq!(
                    smk_engine_resume_root_watching(self.handle, &id, 1, &mut result),
                    SmkStatus::Ok
                );
                smk_scan_result_free(result);
            }
        }
        fn change_preserving_file_metadata(&self) -> PathBuf {
            let path = self.directory.path().join("script.jsx");
            let time = std::fs::metadata(&path).unwrap().modified().unwrap();
            let text = std::fs::read_to_string(&path)
                .unwrap()
                .replace("Name=One", "Name=Two");
            std::fs::write(&path, text).unwrap();
            std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(time))
                .unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe {
                smk_engine_stop_watching(self.handle);
                smk_engine_free(self.handle);
            }
        }
    }

    #[test]
    fn resume_publication_does_not_repeat_its_source_scan() {
        for policy in [
            WatchReconciliationDelivery::Complete,
            WatchReconciliationDelivery::ProgressiveByRootPriority,
        ] {
            let fixture = Fixture::new(Some(policy));
            fixture.resume();
            let mut state = engine_mut(fixture.handle).unwrap();
            let before = state.engine.snapshot(&RootId::from("root")).unwrap();
            let (delivery, _) = poll_watcher_scan(&mut state, false).unwrap();
            assert!(
                delivery.is_some(),
                "resume must still publish to the watch stream"
            );
            let after = state.engine.snapshot(&RootId::from("root")).unwrap();
            assert!(
                Arc::ptr_eq(&before, &after),
                "publishing a completed resume reread the source tree"
            );
        }
    }

    #[test]
    fn changes_after_resume_are_read_even_with_identical_size_and_timestamp() {
        for policy in [
            WatchReconciliationDelivery::Complete,
            WatchReconciliationDelivery::ProgressiveByRootPriority,
        ] {
            let fixture = Fixture::new(Some(policy));
            fixture.resume();
            let path = fixture.change_preserving_file_metadata();
            let mut state = engine_mut(fixture.handle).unwrap();
            state
                .engine
                .mark_changed_paths(RawChangeBatch {
                    paths: vec![path],
                    overflowed: true,
                })
                .unwrap();
            poll_watcher_scan(&mut state, false).unwrap();
            assert_eq!(
                state.engine.catalog_snapshot().unwrap().file_items[0]
                    .name
                    .as_deref(),
                Some("Two")
            );
        }
    }

    #[test]
    fn cache_invalidation_after_resume_does_not_skip_the_next_read() {
        let fixture = Fixture::new(Some(WatchReconciliationDelivery::Complete));
        fixture.resume();
        fixture.change_preserving_file_metadata();
        let mut state = engine_mut(fixture.handle).unwrap();
        state.engine.invalidate_cache(
            CacheScope::All,
            scriptmetakit::CacheInvalidationReason {
                code: "test".into(),
                message: "test".into(),
            },
        );
        poll_watcher_scan(&mut state, false).unwrap();
        assert_eq!(
            state.engine.catalog_snapshot().unwrap().file_items[0]
                .name
                .as_deref(),
            Some("Two")
        );
    }

    #[test]
    fn a_stopped_watcher_retains_no_pending_publication_ids() {
        let fixture = Fixture::new(None);
        fixture.resume();
        let state = engine_mut(fixture.handle).unwrap();
        assert!(state.watch_reconciled_root_ids.is_empty());
    }

    #[test]
    fn replaced_registrations_do_not_retain_old_publication_ids() {
        let fixture = Fixture::new(Some(WatchReconciliationDelivery::Complete));
        fixture.resume();
        let mut state = engine_mut(fixture.handle).unwrap();
        assert!(
            state
                .watch_reconciled_root_ids
                .contains(&RootId::from("root"))
        );
        apply_engine_reconfiguration(&mut state, |engine| {
            engine
                .set_roots(vec![RootRegistration::file_list_and_metadata(
                    "replacement",
                    fixture.directory.path(),
                )])
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .unwrap();
        assert!(state.watch_reconciled_root_ids.is_empty());
    }

    #[test]
    fn replacing_a_watch_allows_its_notification_to_query_the_engine() {
        use std::{
            sync::{
                atomic::{AtomicBool, Ordering},
                mpsc,
            },
            time::{Duration, Instant},
        };
        let fixture = Fixture::new(None);
        let address = fixture.handle as usize;
        let (entered_tx, entered_rx) = mpsc::channel();
        let (gate_tx, gate_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let gate_rx = Mutex::new(gate_rx);
        let entered = AtomicBool::new(false);
        let mut state = engine_mut(fixture.handle).unwrap();
        let plan = state.engine.watch_plan();
        state.watcher = Some(
            NativeWatcher::start_with_notifier(
                &plan,
                Some(Arc::new(move || {
                    if entered.swap(true, Ordering::SeqCst) {
                        return;
                    }
                    entered_tx.send(()).unwrap();
                    let unlocked = gate_rx.lock().unwrap().recv().unwrap();
                    let mut paused = 0;
                    let ok = unlocked
                        && unsafe {
                            smk_engine_is_root_watching_paused(
                                address as *mut SmkEngine,
                                SmkUtf8Slice {
                                    ptr: b"root".as_ptr(),
                                    len: 4,
                                },
                                &mut paused,
                            )
                        } == SmkStatus::Ok;
                    finished_tx.send(ok && paused != 0).unwrap();
                })),
            )
            .unwrap(),
        );
        state.watch_plan = Some(plan);
        std::fs::write(fixture.directory.path().join("changed.jsx"), "// changed").unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let opener = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                // SAFETY: fixture stays alive until the helper and callback finish.
                if let Ok(guard) = unsafe { &*(address as *mut SmkEngine) }.state.try_lock() {
                    drop(guard);
                    gate_tx.send(true).unwrap();
                    return;
                }
                if Instant::now() >= deadline {
                    // Let the old implementation finish rather than hang the test.
                    gate_tx.send(false).unwrap();
                    return;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        apply_root_watch_transition(&mut state, &[RootId::from("root")], true).unwrap();
        drop(state);
        opener.join().unwrap();
        assert!(
            finished_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "watcher termination held the engine lock while waiting for its callback"
        );
    }
}
