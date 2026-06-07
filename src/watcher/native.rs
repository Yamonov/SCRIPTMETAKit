#![allow(unsafe_code)]

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    core::{ScriptMetaKitError, ScriptMetaKitResult},
    scanner::ExtensionPolicy,
    watcher::{RawChangeBatch, WatchPlan},
};

pub struct NativeWatcher {
    platform: Option<platform::PlatformWatcher>,
    receiver: Receiver<RawChangeBatch>,
    worker: Option<JoinHandle<()>>,
}

impl NativeWatcher {
    pub fn start(plan: &WatchPlan) -> ScriptMetaKitResult<Self> {
        Self::start_with_notifier(plan, None)
    }

    pub fn start_with_notifier(
        plan: &WatchPlan,
        notifier: Option<Arc<dyn Fn() + Send + Sync + 'static>>,
    ) -> ScriptMetaKitResult<Self> {
        let (event_sender, event_receiver) = mpsc::channel();
        let platform = platform::PlatformWatcher::start(plan, event_sender)?;

        let debounce_delay = Duration::from_millis(plan.debounce_delay_millis);
        let max_delivery_delay = (plan.max_delivery_delay_millis > 0)
            .then(|| Duration::from_millis(plan.max_delivery_delay_millis));
        let max_pending_paths = plan.max_pending_paths;
        let overflow_paths: Vec<_> = plan
            .physical_roots
            .iter()
            .map(|root| root.path.clone())
            .collect();
        let watch_roots = overflow_paths.clone();
        let supported_extensions = plan.supported_extensions.clone();
        let skip_hidden_paths = plan.skip_hidden_paths;
        let skip_package_paths = plan.skip_package_paths;

        let (batch_sender, batch_receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut pending_paths = Vec::new();
            let mut pending_overflowed = false;
            let mut first_event_at = None;
            let mut last_event_at = None;

            loop {
                let receive_result = match next_flush_timeout(
                    first_event_at,
                    last_event_at,
                    debounce_delay,
                    max_delivery_delay,
                ) {
                    Some(timeout) => event_receiver.recv_timeout(timeout),
                    None => event_receiver
                        .recv()
                        .map_err(|_| RecvTimeoutError::Disconnected),
                };

                match receive_result {
                    Ok(event) => {
                        let (pending_changed, flush_now) = append_event_to_pending(
                            event,
                            &mut pending_paths,
                            &mut pending_overflowed,
                            NativeEventFilter {
                                overflow_paths: &overflow_paths,
                                max_pending_paths,
                                watch_roots: &watch_roots,
                                supported_extensions: &supported_extensions,
                                skip_hidden_paths,
                                skip_package_paths,
                            },
                        );
                        if !pending_changed {
                            continue;
                        }

                        let now = Instant::now();
                        if first_event_at.is_none() {
                            first_event_at = Some(now);
                        }
                        last_event_at = Some(now);

                        if (flush_now
                            || should_flush_after_event(
                                first_event_at,
                                debounce_delay,
                                max_delivery_delay,
                            ))
                            && !flush_pending_batch(
                                &batch_sender,
                                notifier.as_ref(),
                                &mut pending_paths,
                                &mut pending_overflowed,
                                &mut first_event_at,
                                &mut last_event_at,
                            )
                        {
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        if !flush_pending_batch(
                            &batch_sender,
                            notifier.as_ref(),
                            &mut pending_paths,
                            &mut pending_overflowed,
                            &mut first_event_at,
                            &mut last_event_at,
                        ) {
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        let _ = flush_pending_batch(
                            &batch_sender,
                            notifier.as_ref(),
                            &mut pending_paths,
                            &mut pending_overflowed,
                            &mut first_event_at,
                            &mut last_event_at,
                        );
                        break;
                    }
                }
            }
        });

        Ok(Self {
            platform: Some(platform),
            receiver: batch_receiver,
            worker: Some(worker),
        })
    }

    pub fn try_recv(&self) -> Option<RawChangeBatch> {
        self.receiver.try_recv().ok()
    }
}

impl Drop for NativeWatcher {
    fn drop(&mut self) {
        self.platform.take();
        if let Some(worker) = self.worker.take()
            && worker.thread().id() != thread::current().id()
        {
            let _ = worker.join();
        }
    }
}

#[derive(Debug)]
enum NativeFsEvent {
    Changed {
        path: PathBuf,
        may_change_directory_tree: bool,
        identifies_folder: bool,
    },
    Overflow,
}

fn append_event_to_pending(
    event: NativeFsEvent,
    pending_paths: &mut Vec<PathBuf>,
    pending_overflowed: &mut bool,
    filter: NativeEventFilter<'_>,
) -> (bool, bool) {
    match event {
        NativeFsEvent::Changed {
            path,
            may_change_directory_tree,
            identifies_folder,
        } => {
            if !should_keep_event_path(
                &path,
                may_change_directory_tree,
                identifies_folder,
                filter.watch_roots,
                filter.supported_extensions,
                filter.skip_hidden_paths,
                filter.skip_package_paths,
            ) {
                return (false, false);
            }

            pending_paths.push(path);
            pending_paths.sort();
            pending_paths.dedup();

            if pending_exceeded_max(pending_paths, filter.max_pending_paths) {
                mark_pending_overflowed(pending_paths, pending_overflowed, filter.overflow_paths);
                return (true, true);
            }

            (true, false)
        }
        NativeFsEvent::Overflow => {
            mark_pending_overflowed(pending_paths, pending_overflowed, filter.overflow_paths);
            (true, false)
        }
    }
}

struct NativeEventFilter<'a> {
    overflow_paths: &'a [PathBuf],
    max_pending_paths: usize,
    watch_roots: &'a [PathBuf],
    supported_extensions: &'a ExtensionPolicy,
    skip_hidden_paths: bool,
    skip_package_paths: bool,
}

fn should_keep_event_path(
    path: &Path,
    may_change_directory_tree: bool,
    identifies_folder: bool,
    watch_roots: &[PathBuf],
    supported_extensions: &ExtensionPolicy,
    skip_hidden_paths: bool,
    skip_package_paths: bool,
) -> bool {
    if skip_hidden_paths && path_has_hidden_component(path, watch_roots) {
        return false;
    }

    if skip_package_paths && path_has_package_component(path, watch_roots) {
        return false;
    }

    if supported_extensions.contains_path(path) {
        return true;
    }

    if !may_change_directory_tree {
        return false;
    }

    identifies_folder || path_is_existing_directory(path) || path.extension().is_none()
}

fn path_has_hidden_component(path: &Path, watch_roots: &[PathBuf]) -> bool {
    relative_event_path(path, watch_roots)
        .components()
        .any(|component| component.as_os_str().as_encoded_bytes().starts_with(b"."))
}

fn path_has_package_component(path: &Path, watch_roots: &[PathBuf]) -> bool {
    relative_event_path(path, watch_roots)
        .components()
        .any(|component| {
            let component_path = Path::new(component.as_os_str());
            matches!(
                component_path
                    .extension()
                    .and_then(|extension| extension.to_str()),
                Some("app" | "bundle" | "framework" | "plugin" | "appex")
            )
        })
}

fn relative_event_path<'a>(path: &'a Path, watch_roots: &[PathBuf]) -> &'a Path {
    watch_roots
        .iter()
        .find_map(|root| path.strip_prefix(root).ok())
        .unwrap_or(path)
}

fn path_is_existing_directory(path: &Path) -> bool {
    path.metadata().is_ok_and(|metadata| metadata.is_dir())
}

fn pending_exceeded_max(pending_paths: &[PathBuf], max_pending_paths: usize) -> bool {
    max_pending_paths > 0 && pending_paths.len() > max_pending_paths
}

fn mark_pending_overflowed(
    pending_paths: &mut Vec<PathBuf>,
    pending_overflowed: &mut bool,
    overflow_paths: &[PathBuf],
) {
    pending_paths.clear();
    pending_paths.extend_from_slice(overflow_paths);
    pending_paths.sort();
    pending_paths.dedup();
    *pending_overflowed = true;
}

fn should_flush_after_event(
    first_event_at: Option<Instant>,
    debounce_delay: Duration,
    max_delivery_delay: Option<Duration>,
) -> bool {
    debounce_delay.is_zero()
        || first_event_at.is_some_and(|first_event_at| {
            max_delivery_delay.is_some_and(|delay| first_event_at.elapsed() >= delay)
        })
}

fn next_flush_timeout(
    first_event_at: Option<Instant>,
    last_event_at: Option<Instant>,
    debounce_delay: Duration,
    max_delivery_delay: Option<Duration>,
) -> Option<Duration> {
    let (Some(first_event_at), Some(last_event_at)) = (first_event_at, last_event_at) else {
        return None;
    };

    let quiet_deadline = last_event_at + debounce_delay;
    let deadline = max_delivery_delay
        .map(|delay| (first_event_at + delay).min(quiet_deadline))
        .unwrap_or(quiet_deadline);
    let now = Instant::now();
    Some(deadline.saturating_duration_since(now))
}

fn flush_pending_batch(
    batch_sender: &mpsc::Sender<RawChangeBatch>,
    notifier: Option<&Arc<dyn Fn() + Send + Sync + 'static>>,
    pending_paths: &mut Vec<PathBuf>,
    pending_overflowed: &mut bool,
    first_event_at: &mut Option<Instant>,
    last_event_at: &mut Option<Instant>,
) -> bool {
    if pending_paths.is_empty() && !*pending_overflowed {
        *first_event_at = None;
        *last_event_at = None;
        return true;
    }

    let batch = RawChangeBatch {
        paths: std::mem::take(pending_paths),
        overflowed: *pending_overflowed,
    };
    *pending_overflowed = false;
    *first_event_at = None;
    *last_event_at = None;

    if batch_sender.send(batch).is_err() {
        return false;
    }

    if let Some(notifier) = notifier {
        notifier();
    }
    true
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{
        ffi::{CStr, c_void},
        os::raw::c_char,
        path::PathBuf,
        ptr,
        sync::{Arc, Mutex, mpsc},
        thread::{self, JoinHandle},
    };

    use fsevent_sys as fs;
    use fsevent_sys::core_foundation as cf;

    use super::{NativeFsEvent, ScriptMetaKitError, ScriptMetaKitResult, WatchPlan};

    pub struct PlatformWatcher {
        paths: cf::CFMutableArrayRef,
        run_loop: Arc<Mutex<Option<CFSendWrapper>>>,
        stop_sender: Option<mpsc::Sender<()>>,
        worker: Option<JoinHandle<()>>,
    }

    struct StreamContext {
        sender: mpsc::Sender<NativeFsEvent>,
    }

    struct CFSendWrapper(usize);

    unsafe impl Send for CFSendWrapper {}

    const RUN_LOOP_TICK_SECONDS: cf::CFTimeInterval = 0.25;

    unsafe extern "C" {
        fn CFRunLoopRunInMode(
            mode: cf::CFStringRef,
            seconds: cf::CFTimeInterval,
            return_after_source_handled: cf::Boolean,
        ) -> i32;
    }

    impl PlatformWatcher {
        pub fn start(
            plan: &WatchPlan,
            event_sender: mpsc::Sender<NativeFsEvent>,
        ) -> ScriptMetaKitResult<Self> {
            let paths = unsafe {
                cf::CFArrayCreateMutable(cf::kCFAllocatorDefault, 0, &cf::kCFTypeArrayCallBacks)
            };
            if paths.is_null() {
                return Err(ScriptMetaKitError::InvalidConfig(
                    "failed to allocate FSEvents path array".to_string(),
                ));
            }

            if plan.physical_roots.is_empty() {
                return Ok(Self {
                    paths,
                    run_loop: Arc::new(Mutex::new(None)),
                    stop_sender: None,
                    worker: None,
                });
            }

            for root in &plan.physical_roots {
                if let Err(error) = append_watch_path(paths, &root.path) {
                    unsafe {
                        cf::CFRelease(paths);
                    }
                    return Err(error);
                }
            }

            let context = Box::into_raw(Box::new(StreamContext {
                sender: event_sender,
            }));
            let stream_context = fs::FSEventStreamContext {
                version: 0,
                info: context as *mut c_void,
                retain: None,
                release: Some(release_context),
                copy_description: None,
            };

            let stream = unsafe {
                let native_event_latency_seconds =
                    plan.native_event_latency_millis as f64 / 1_000.0;
                fs::FSEventStreamCreate(
                    cf::kCFAllocatorDefault,
                    callback,
                    &stream_context,
                    paths,
                    fs::kFSEventStreamEventIdSinceNow,
                    native_event_latency_seconds,
                    fs::kFSEventStreamCreateFlagFileEvents | fs::kFSEventStreamCreateFlagNoDefer,
                )
            };

            if stream.is_null() {
                unsafe {
                    drop(Box::from_raw(context));
                    cf::CFRelease(paths);
                }
                return Err(ScriptMetaKitError::InvalidConfig(
                    "failed to create FSEvents stream".to_string(),
                ));
            }

            let raw_stream = stream;
            let stream = CFSendWrapper(raw_stream as usize);
            let run_loop = Arc::new(Mutex::new(None));
            let worker_run_loop = Arc::clone(&run_loop);
            let (stop_sender, stop_receiver) = mpsc::channel();
            let (start_sender, start_receiver) = mpsc::sync_channel(1);
            let worker = match thread::Builder::new()
                .name("scriptmetakit-fsevents".to_string())
                .spawn(move || {
                    let stream = stream.0 as fs::FSEventStreamRef;
                    unsafe {
                        let run_loop = cf::CFRunLoopGetCurrent();
                        if let Ok(mut stored_run_loop) = worker_run_loop.lock() {
                            *stored_run_loop = Some(CFSendWrapper(run_loop as usize));
                        }
                        fs::FSEventStreamScheduleWithRunLoop(
                            stream,
                            run_loop,
                            cf::kCFRunLoopDefaultMode,
                        );
                        let started = fs::FSEventStreamStart(stream) != 0;
                        if started {
                            let _ = start_sender.send(Ok(()));
                            loop {
                                match stop_receiver.try_recv() {
                                    Ok(()) | Err(mpsc::TryRecvError::Disconnected) => break,
                                    Err(mpsc::TryRecvError::Empty) => {}
                                }
                                CFRunLoopRunInMode(
                                    cf::kCFRunLoopDefaultMode,
                                    RUN_LOOP_TICK_SECONDS,
                                    1,
                                );
                            }
                        } else {
                            let _ = start_sender
                                .send(Err("failed to start FSEvents stream".to_string()));
                        }
                        if started {
                            fs::FSEventStreamStop(stream);
                        }
                        fs::FSEventStreamInvalidate(stream);
                        fs::FSEventStreamRelease(stream);
                        if let Ok(mut stored_run_loop) = worker_run_loop.lock() {
                            *stored_run_loop = None;
                        }
                    }
                }) {
                Ok(worker) => worker,
                Err(error) => {
                    unsafe {
                        fs::FSEventStreamRelease(raw_stream);
                        cf::CFRelease(paths);
                    }
                    return Err(ScriptMetaKitError::InvalidConfig(error.to_string()));
                }
            };

            match start_receiver.recv() {
                Ok(Ok(())) => {}
                Ok(Err(message)) => {
                    let _ = worker.join();
                    unsafe {
                        cf::CFRelease(paths);
                    }
                    return Err(ScriptMetaKitError::InvalidConfig(message));
                }
                Err(error) => {
                    let _ = worker.join();
                    unsafe {
                        cf::CFRelease(paths);
                    }
                    return Err(ScriptMetaKitError::InvalidConfig(error.to_string()));
                }
            }

            Ok(Self {
                paths,
                run_loop,
                stop_sender: Some(stop_sender),
                worker: Some(worker),
            })
        }
    }

    impl Drop for PlatformWatcher {
        fn drop(&mut self) {
            if let Some(stop_sender) = self.stop_sender.take() {
                let _ = stop_sender.send(());
            }
            if let Ok(mut stored_run_loop) = self.run_loop.lock()
                && let Some(run_loop) = stored_run_loop.take()
            {
                unsafe {
                    cf::CFRunLoopStop(run_loop.0 as cf::CFRunLoopRef);
                }
            }
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            unsafe {
                cf::CFRelease(self.paths);
            }
        }
    }

    fn append_watch_path(
        paths: cf::CFMutableArrayRef,
        path: &std::path::Path,
    ) -> ScriptMetaKitResult<()> {
        let Some(path) = path.to_str() else {
            return Err(ScriptMetaKitError::InvalidConfig(
                "watch path is not valid UTF-8".to_string(),
            ));
        };

        let mut error = ptr::null_mut();
        let cf_path = unsafe { cf::str_path_to_cfstring_ref(path, &mut error) };
        if cf_path.is_null() {
            if !error.is_null() {
                unsafe {
                    cf::CFRelease(error as cf::CFRef);
                }
            }
            return Err(ScriptMetaKitError::Io {
                path: PathBuf::from(path),
                message: "failed to create FSEvents path reference".to_string(),
            });
        }

        unsafe {
            cf::CFArrayAppendValue(paths, cf_path);
            cf::CFRelease(cf_path);
        }
        Ok(())
    }

    extern "C" fn release_context(info: *const c_void) {
        if !info.is_null() {
            unsafe {
                drop(Box::from_raw(info as *mut StreamContext));
            }
        }
    }

    extern "C" fn callback(
        _stream_ref: fs::FSEventStreamRef,
        info: *mut c_void,
        num_events: usize,
        event_paths: *mut c_void,
        event_flags: *const fs::FSEventStreamEventFlags,
        _event_ids: *const fs::FSEventStreamEventId,
    ) {
        if info.is_null() || event_paths.is_null() || event_flags.is_null() {
            return;
        }

        let context = unsafe { &*(info as *const StreamContext) };
        let paths = event_paths as *const *const c_char;
        for index in 0..num_events {
            let raw_path = unsafe { *paths.add(index) };
            if raw_path.is_null() {
                continue;
            }
            let Ok(path) = unsafe { CStr::from_ptr(raw_path) }.to_str() else {
                continue;
            };
            let flags = unsafe { *event_flags.add(index) };
            let event = event_from_flags(PathBuf::from(path), flags);
            let _ = context.sender.send(event);
        }
    }

    fn event_from_flags(path: PathBuf, flags: fs::FSEventStreamEventFlags) -> NativeFsEvent {
        if flags
            & (fs::kFSEventStreamEventFlagMustScanSubDirs
                | fs::kFSEventStreamEventFlagUserDropped
                | fs::kFSEventStreamEventFlagKernelDropped
                | fs::kFSEventStreamEventFlagEventIdsWrapped)
            != 0
        {
            return NativeFsEvent::Overflow;
        }

        let may_change_directory_tree = flags
            & (fs::kFSEventStreamEventFlagItemCreated
                | fs::kFSEventStreamEventFlagItemRemoved
                | fs::kFSEventStreamEventFlagItemRenamed
                | fs::kFSEventStreamEventFlagRootChanged
                | fs::kFSEventStreamEventFlagMount
                | fs::kFSEventStreamEventFlagUnmount)
            != 0;
        let identifies_folder = flags & fs::kFSEventStreamEventFlagItemIsDir != 0;

        NativeFsEvent::Changed {
            path,
            may_change_directory_tree,
            identifies_folder,
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        ffi::{OsString, c_void},
        mem,
        os::windows::ffi::{OsStrExt, OsStringExt},
        path::{Path, PathBuf},
        ptr, slice,
        sync::mpsc,
        thread::{self, JoinHandle},
    };

    use windows_sys::Win32::{
        Foundation::{CloseHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE, TRUE, WAIT_OBJECT_0},
        Storage::FileSystem::{
            CreateFileW, FILE_ACTION_ADDED, FILE_ACTION_REMOVED, FILE_ACTION_RENAMED_NEW_NAME,
            FILE_ACTION_RENAMED_OLD_NAME, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
            FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_CREATION,
            FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
            FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SECURITY, FILE_NOTIFY_CHANGE_SIZE,
            FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING, ReadDirectoryChangesW,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects},
        },
    };

    use super::{NativeFsEvent, ScriptMetaKitError, ScriptMetaKitResult, WatchPlan};

    const BUFFER_SIZE: usize = 64 * 1024;

    pub struct PlatformWatcher {
        stop_events: Vec<isize>,
        workers: Vec<JoinHandle<()>>,
    }

    impl PlatformWatcher {
        pub fn start(
            plan: &WatchPlan,
            event_sender: mpsc::Sender<NativeFsEvent>,
        ) -> ScriptMetaKitResult<Self> {
            let mut stop_events = Vec::new();
            let mut workers = Vec::new();

            for root in &plan.physical_roots {
                let directory = match open_directory(&root.path) {
                    Ok(directory) => directory,
                    Err(error) => {
                        stop_workers(&mut stop_events, &mut workers);
                        return Err(error);
                    }
                };
                let stop_event = unsafe { CreateEventW(ptr::null(), TRUE, FALSE, ptr::null()) };
                if stop_event.is_null() || stop_event == INVALID_HANDLE_VALUE {
                    unsafe {
                        CloseHandle(directory);
                    }
                    stop_workers(&mut stop_events, &mut workers);
                    return Err(ScriptMetaKitError::Io {
                        path: root.path.clone(),
                        message: "failed to create Windows watcher stop event".to_string(),
                    });
                }

                let root_path = root.path.clone();
                let sender = event_sender.clone();
                let directory_value = directory as isize;
                let stop_event_value = stop_event as isize;
                let worker = thread::Builder::new()
                    .name("scriptmetakit-read-directory-changes".to_string())
                    .spawn(move || {
                        watch_root(
                            root_path,
                            directory_value as HANDLE,
                            stop_event_value as HANDLE,
                            sender,
                        );
                    })
                    .map_err(|error| {
                        unsafe {
                            CloseHandle(stop_event);
                            CloseHandle(directory);
                        }
                        stop_workers(&mut stop_events, &mut workers);
                        ScriptMetaKitError::InvalidConfig(error.to_string())
                    })?;

                stop_events.push(stop_event as isize);
                workers.push(worker);
            }

            Ok(Self {
                stop_events,
                workers,
            })
        }
    }

    impl Drop for PlatformWatcher {
        fn drop(&mut self) {
            stop_workers(&mut self.stop_events, &mut self.workers);
        }
    }

    fn stop_workers(stop_events: &mut Vec<isize>, workers: &mut Vec<JoinHandle<()>>) {
        for stop_event in stop_events.iter() {
            unsafe {
                SetEvent(*stop_event as HANDLE);
            }
        }
        for worker in workers.drain(..) {
            let _ = worker.join();
        }
        for stop_event in stop_events.drain(..) {
            unsafe {
                CloseHandle(stop_event as HANDLE);
            }
        }
    }

    fn open_directory(path: &Path) -> ScriptMetaKitResult<HANDLE> {
        let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        };

        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(ScriptMetaKitError::Io {
                path: path.to_path_buf(),
                message: "failed to open directory for ReadDirectoryChangesW".to_string(),
            });
        }

        Ok(handle)
    }

    fn watch_root(
        root: PathBuf,
        directory: HANDLE,
        stop_event: HANDLE,
        sender: mpsc::Sender<NativeFsEvent>,
    ) {
        loop {
            let mut buffer = [0u8; BUFFER_SIZE];
            let mut overlapped: OVERLAPPED = unsafe { mem::zeroed() };
            let read_event = unsafe { CreateEventW(ptr::null(), TRUE, FALSE, ptr::null()) };
            if read_event.is_null() || read_event == INVALID_HANDLE_VALUE {
                let _ = sender.send(NativeFsEvent::Overflow);
                break;
            }
            overlapped.hEvent = read_event;

            let mut bytes_returned = 0u32;
            let read_started = unsafe {
                ReadDirectoryChangesW(
                    directory,
                    buffer.as_mut_ptr() as *mut c_void,
                    buffer.len() as u32,
                    TRUE,
                    FILE_NOTIFY_CHANGE_FILE_NAME
                        | FILE_NOTIFY_CHANGE_DIR_NAME
                        | FILE_NOTIFY_CHANGE_ATTRIBUTES
                        | FILE_NOTIFY_CHANGE_SIZE
                        | FILE_NOTIFY_CHANGE_LAST_WRITE
                        | FILE_NOTIFY_CHANGE_CREATION
                        | FILE_NOTIFY_CHANGE_SECURITY,
                    &mut bytes_returned,
                    &mut overlapped,
                    None,
                )
            };

            if read_started == 0 {
                unsafe {
                    CloseHandle(read_event);
                }
                let _ = sender.send(NativeFsEvent::Overflow);
                break;
            }

            let wait_handles = [stop_event, read_event];
            let wait_result =
                unsafe { WaitForMultipleObjects(2, wait_handles.as_ptr(), FALSE, INFINITE) };
            if wait_result == WAIT_OBJECT_0 {
                unsafe {
                    CancelIoEx(directory, &overlapped);
                    CloseHandle(read_event);
                }
                break;
            }

            let mut transferred = 0u32;
            let completed =
                unsafe { GetOverlappedResult(directory, &overlapped, &mut transferred, FALSE) };
            unsafe {
                CloseHandle(read_event);
            }

            if completed == 0 {
                let _ = sender.send(NativeFsEvent::Overflow);
                break;
            }

            if transferred == 0 {
                let _ = sender.send(NativeFsEvent::Overflow);
                continue;
            }

            parse_change_buffer(&root, &buffer[..transferred as usize], &sender);
        }

        unsafe {
            CloseHandle(directory);
        }
    }

    fn parse_change_buffer(root: &Path, buffer: &[u8], sender: &mpsc::Sender<NativeFsEvent>) {
        let mut offset = 0usize;
        while offset < buffer.len() {
            let entry_ptr = unsafe { buffer.as_ptr().add(offset) };
            let entry = unsafe { ptr::read_unaligned(entry_ptr as *const FILE_NOTIFY_INFORMATION) };
            let name_offset = offset + mem::offset_of!(FILE_NOTIFY_INFORMATION, FileName);
            let name_len = entry.FileNameLength as usize / 2;
            if name_offset + name_len * 2 > buffer.len() {
                let _ = sender.send(NativeFsEvent::Overflow);
                break;
            }

            let name = unsafe {
                OsString::from_wide(slice::from_raw_parts(
                    buffer.as_ptr().add(name_offset) as *const u16,
                    name_len,
                ))
            };
            let path = root.join(PathBuf::from(name));
            let may_change_directory_tree = matches!(
                entry.Action,
                FILE_ACTION_ADDED
                    | FILE_ACTION_REMOVED
                    | FILE_ACTION_RENAMED_OLD_NAME
                    | FILE_ACTION_RENAMED_NEW_NAME
            );

            let _ = sender.send(NativeFsEvent::Changed {
                path,
                may_change_directory_tree,
                identifies_folder: false,
            });

            if entry.NextEntryOffset == 0 {
                break;
            }
            offset += entry.NextEntryOffset as usize;
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use std::sync::mpsc;

    use super::{NativeFsEvent, ScriptMetaKitError, ScriptMetaKitResult, WatchPlan};

    pub struct PlatformWatcher;

    impl PlatformWatcher {
        pub fn start(
            _plan: &WatchPlan,
            _event_sender: mpsc::Sender<NativeFsEvent>,
        ) -> ScriptMetaKitResult<Self> {
            Err(ScriptMetaKitError::InvalidConfig(
                "native watcher is implemented for macOS and Windows".to_string(),
            ))
        }
    }
}
