use scriptmetakit_ffi as ffi;
use std::{fs, path::Path, ptr, slice};

fn input(text: &str) -> ffi::SmkUtf8Slice {
    ffi::SmkUtf8Slice {
        ptr: text.as_ptr(),
        len: text.len(),
    }
}

// SAFETY: each caller borrows this slice only while its owning native result is live.
unsafe fn text(value: ffi::SmkUtf8Slice) -> String {
    if value.len == 0 {
        return String::new();
    }
    String::from_utf8(unsafe { slice::from_raw_parts(value.ptr, value.len) }.to_vec()).unwrap()
}

fn write_script(path: &Path, id: &str, version: &str) {
    fs::write(path, format!("// SCRIPTMETA-BEGIN\n// Script-ID={id}\n// Version={version}\n// Name=Example\n// SCRIPTMETA-END\n")).unwrap();
}

struct Engine(*mut ffi::SmkEngine);
impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: this fixture owns the live engine handle exclusively.
        unsafe { ffi::smk_engine_free(self.0) };
    }
}

fn fixture() -> (tempfile::TempDir, Engine) {
    let directory = tempfile::tempdir().unwrap();
    let paths = ["a", "b"].map(|id| {
        let path = directory.path().join(id);
        fs::create_dir(&path).unwrap();
        write_script(&path.join("script.jsx"), id, "1.0");
        path.to_string_lossy().into_owned()
    });
    let roots = [0, 1].map(|index| ffi::SmkRootRegistration {
        root_id: input(["a", "b"][index]),
        path: input(&paths[index]),
        purpose: 3,
        watch_policy: 2,
        cache_policy: 3,
        refresh_policy: 2,
        ..Default::default()
    });
    let mut engine = ptr::null_mut();
    // SAFETY: all input slices outlive each synchronous native call; outputs are writable.
    unsafe {
        assert_eq!(
            ffi::smk_engine_create_default(&mut engine),
            ffi::SmkStatus::Ok
        );
        let engine = Engine(engine);
        assert_eq!(
            ffi::smk_engine_set_roots(engine.0, roots.as_ptr(), roots.len()),
            ffi::SmkStatus::Ok
        );
        let mut result = ptr::null_mut();
        assert_eq!(
            ffi::smk_engine_scan_roots(engine.0, ptr::null(), 0, 2, 0, &mut result),
            ffi::SmkStatus::Ok
        );
        ffi::smk_scan_result_free(result);
        (directory, engine)
    }
}

fn paused(engine: &Engine, id: &str) -> bool {
    let mut output = 0;
    // SAFETY: the fixture owns the engine and output is writable.
    unsafe {
        assert_eq!(
            ffi::smk_engine_is_root_watching_paused(engine.0, input(id), &mut output),
            ffi::SmkStatus::Ok
        );
    }
    output != 0
}

unsafe fn version(result: *mut ffi::SmkScanResult, id: &str) -> Option<String> {
    let mut items = ffi::SmkScriptItemSlice::default();
    // SAFETY: result is borrowed by the caller and remains live throughout this function.
    unsafe {
        assert_eq!(
            ffi::smk_scan_result_items(result, &mut items),
            ffi::SmkStatus::Ok
        );
        if items.len == 0 {
            return None;
        }
        slice::from_raw_parts(items.ptr, items.len)
            .iter()
            .find(|item| text(item.script_id) == id)
            .map(|item| text(item.version))
    }
}

#[test]
fn c_abi_resume_forces_reload_and_validation_is_atomic() {
    let (directory, engine) = fixture();
    let root = input("a");
    let unknown = [root, input("unknown")];
    // SAFETY: fixture owns the engine; inputs and outputs remain live for every call.
    unsafe {
        assert_ne!(
            ffi::smk_engine_pause_root_watching(engine.0, unknown.as_ptr(), unknown.len()),
            ffi::SmkStatus::Ok
        );
        assert!(!paused(&engine, "a"));
        assert_ne!(
            ffi::smk_engine_pause_root_watching(engine.0, ptr::null(), 0),
            ffi::SmkStatus::Ok
        );
        assert_eq!(
            ffi::smk_engine_pause_root_watching(engine.0, &root, 1),
            ffi::SmkStatus::Ok
        );
        assert!(paused(&engine, "a"));
        let path = directory.path().join("a/script.jsx");
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        write_script(&path, "a", "2.0");
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let mut result = ptr::null_mut();
        assert_eq!(
            ffi::smk_engine_resume_root_watching(engine.0, &root, 1, &mut result),
            ffi::SmkStatus::Ok
        );
        assert_eq!(version(result, "a").as_deref(), Some("2.0"));
        assert!(!paused(&engine, "a"));
        ffi::smk_scan_result_free(result);
    }
}

#[cfg(feature = "native-watch")]
#[test]
fn native_watching_continues_for_other_roots_and_survives_all_roots_paused() {
    use std::time::{Duration, Instant};
    let (directory, engine) = fixture();
    let root = input("a");
    // SAFETY: fixture owns the engine and frees all native scan results before returning.
    unsafe {
        assert_eq!(ffi::smk_engine_start_watching(engine.0), ffi::SmkStatus::Ok);
        assert_eq!(
            ffi::smk_engine_pause_root_watching(engine.0, &root, 1),
            ffi::SmkStatus::Ok
        );
        write_script(&directory.path().join("a/script.jsx"), "a", "2.0");
        write_script(&directory.path().join("b/script.jsx"), "b", "2.0");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observed_b = false;
        while Instant::now() < deadline {
            let mut changed = 0;
            let mut result = ptr::null_mut();
            assert_eq!(
                ffi::smk_engine_poll_watcher_scan(engine.0, &mut changed, &mut result),
                ffi::SmkStatus::Ok
            );
            if !result.is_null() {
                let a = version(result, "a");
                assert!(a.is_none() || a.as_deref() == Some("1.0"));
                observed_b = version(result, "b").as_deref() == Some("2.0");
                ffi::smk_scan_result_free(result);
            }
            if observed_b {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(observed_b, "unpaused root did not update");
        let b = input("b");
        assert_eq!(
            ffi::smk_engine_pause_root_watching(engine.0, &b, 1),
            ffi::SmkStatus::Ok
        );
        let mut result = ptr::null_mut();
        assert_eq!(
            ffi::smk_engine_resume_root_watching(engine.0, &root, 1, &mut result),
            ffi::SmkStatus::Ok
        );
        assert_eq!(version(result, "a").as_deref(), Some("2.0"));
        assert!(!paused(&engine, "a"));
        assert!(paused(&engine, "b"));
        ffi::smk_scan_result_free(result);
        assert_eq!(ffi::smk_engine_stop_watching(engine.0), ffi::SmkStatus::Ok);
    }
}

#[cfg(feature = "native-watch")]
#[test]
fn pausing_pending_logical_root_completes_reconciliation_on_a_shared_physical_watch() {
    let (directory, engine) = fixture();
    let path = directory.path().join("a").to_string_lossy().into_owned();
    let roots = ["a", "b"].map(|id| ffi::SmkRootRegistration {
        root_id: input(id),
        path: input(&path),
        purpose: 3,
        watch_policy: 2,
        cache_policy: 3,
        refresh_policy: 2,
        priority: if id == "a" { 1 } else { 2 },
        ..Default::default()
    });
    // SAFETY: all inputs are borrowed for synchronous calls; each native result
    // is freed before the next call and the fixture owns the engine.
    unsafe {
        assert_eq!(
            ffi::smk_engine_set_roots(engine.0, roots.as_ptr(), roots.len()),
            ffi::SmkStatus::Ok
        );
        assert_eq!(
            ffi::smk_engine_start_watching_with_callback_v2(engine.0, None, ptr::null_mut(), 1),
            ffi::SmkStatus::Ok
        );
        let mut changed = 0;
        let mut result = ptr::null_mut();
        assert_eq!(
            ffi::smk_engine_poll_watcher_scan_dirty_only(engine.0, &mut changed, &mut result),
            ffi::SmkStatus::Ok
        );
        let mut info = ffi::SmkWatchDeliveryInfo::default();
        assert_eq!(
            ffi::smk_scan_result_watch_delivery_info(result, &mut info),
            ffi::SmkStatus::Ok
        );
        assert_eq!(info.covers_all_watched_roots, 0);
        ffi::smk_scan_result_free(result);
        let paused_id = input("b");
        assert_eq!(
            ffi::smk_engine_pause_root_watching(engine.0, &paused_id, 1),
            ffi::SmkStatus::Ok
        );
        result = ptr::null_mut();
        assert_eq!(
            ffi::smk_engine_poll_watcher_scan_dirty_only(engine.0, &mut changed, &mut result),
            ffi::SmkStatus::Ok
        );
        assert!(!result.is_null());
        assert_eq!(
            ffi::smk_scan_result_watch_delivery_info(result, &mut info),
            ffi::SmkStatus::Ok
        );
        assert_eq!(info.is_reconciliation, 1);
        assert_eq!(info.covers_all_watched_roots, 1);
        let mut root_values = ffi::SmkRootSnapshotSlice::default();
        assert_eq!(
            ffi::smk_scan_result_roots(result, &mut root_values),
            ffi::SmkStatus::Ok
        );
        assert_eq!(root_values.len, 1);
        assert_eq!(text((*root_values.ptr).root_id), "a");
        ffi::smk_scan_result_free(result);
    }
}
