use scriptmetakit_ffi as ffi;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    fs, ptr,
};

struct Allocator;
thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static BYTES: Cell<usize> = const { Cell::new(0) };
}
fn record(size: usize) {
    if TRACKING.try_with(Cell::get).unwrap_or(false) {
        let _ = BYTES.try_with(|bytes| bytes.set(bytes.get() + size));
    }
}
// SAFETY: allocation ownership and layout are passed unchanged to System.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn pause_allocation_bytes(files: usize) -> usize {
    let directory = tempfile::tempdir().unwrap();
    for index in 0..files {
        fs::write(
            directory.path().join(format!("{index:04}.jsx")),
            "// fixture\n",
        )
        .unwrap();
    }
    let path = directory.path().to_string_lossy().into_owned();
    let value = ffi::SmkUtf8Slice {
        ptr: path.as_ptr(),
        len: path.len(),
    };
    let mut handle = ptr::null_mut();
    let mut result = ptr::null_mut();
    // SAFETY: this function owns its engine/results; borrowed strings outlive all calls.
    unsafe {
        assert_eq!(
            ffi::smk_engine_create_default(&mut handle),
            ffi::SmkStatus::Ok
        );
        assert_eq!(
            ffi::smk_engine_scan_folders(handle, &value, 1, 0, &mut result),
            ffi::SmkStatus::Ok
        );
        let mut roots = ffi::SmkRootSnapshotSlice::default();
        assert_eq!(
            ffi::smk_scan_result_roots(result, &mut roots),
            ffi::SmkStatus::Ok
        );
        let root_id = (*roots.ptr).root_id;
        BYTES.with(|bytes| bytes.set(0));
        TRACKING.with(|tracking| tracking.set(true));
        let status = ffi::smk_engine_pause_root_watching(handle, &root_id, 1);
        TRACKING.with(|tracking| tracking.set(false));
        let bytes = BYTES.with(Cell::get);
        ffi::smk_scan_result_free(result);
        ffi::smk_engine_free(handle);
        assert_eq!(status, ffi::SmkStatus::Ok);
        bytes
    }
}

#[test]
fn pausing_does_not_copy_a_catalogs_per_file_indexes() {
    let small = pause_allocation_bytes(10);
    let large = pause_allocation_bytes(1_000);
    println!("pause allocations: 10 files={small} bytes, 1000 files={large} bytes");
    assert!(
        large < small + 16 * 1024,
        "pause allocations scale with cached scripts: {small} -> {large}"
    );
}
