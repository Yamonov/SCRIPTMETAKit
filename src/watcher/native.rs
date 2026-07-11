#![allow(unsafe_code)]

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    core::{ScriptMetaKitError, ScriptMetaKitResult},
    scanner::ExtensionPolicy,
    watcher::{DEFAULT_MAX_PENDING_PATHS, RawChangeBatch, WatchPlan},
};

pub struct NativeWatcher {
    platform: Option<platform::PlatformWatcher>,
    receiver: Receiver<RawChangeBatch>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Clone, Debug)]
struct NativeEventSender {
    sender: SyncSender<NativeFsEvent>,
    overflowed: Arc<AtomicBool>,
}

impl NativeEventSender {
    fn send(&self, event: NativeFsEvent) -> Result<(), ()> {
        match self.sender.try_send(event) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.overflowed.store(true, Ordering::Release);
                Ok(())
            }
            Err(TrySendError::Disconnected(_)) => Err(()),
        }
    }
}

impl NativeWatcher {
    pub fn start(plan: &WatchPlan) -> ScriptMetaKitResult<Self> {
        Self::start_with_notifier(plan, None)
    }

    pub fn start_with_notifier(
        plan: &WatchPlan,
        notifier: Option<Arc<dyn Fn() + Send + Sync + 'static>>,
    ) -> ScriptMetaKitResult<Self> {
        // Keep native callbacks non-blocking without treating an ordinary
        // multi-path FSEvents batch as overflow. The same configured limit
        // bounds both this handoff queue and the worker's coalesced path set.
        let raw_event_queue_capacity = if plan.max_pending_paths == 0 {
            DEFAULT_MAX_PENDING_PATHS
        } else {
            plan.max_pending_paths
        };
        let (raw_event_sender, event_receiver) = mpsc::sync_channel(raw_event_queue_capacity);
        let event_queue_overflowed = Arc::new(AtomicBool::new(false));
        let event_sender = NativeEventSender {
            sender: raw_event_sender,
            overflowed: Arc::clone(&event_queue_overflowed),
        };
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

        let (batch_sender, batch_receiver) = mpsc::sync_channel(1);
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
                        let (mut pending_changed, mut flush_now) = append_event_to_pending(
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
                        if event_queue_overflowed.swap(false, Ordering::AcqRel) {
                            let (overflow_changed, overflow_flush_now) = append_event_to_pending(
                                NativeFsEvent::Overflow,
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
                            pending_changed |= overflow_changed;
                            flush_now |= overflow_flush_now;
                        }
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
                                &overflow_paths,
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
                            &overflow_paths,
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
                            &overflow_paths,
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
        removes_path: bool,
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
            removes_path,
        } => {
            if !should_keep_event_path(
                &path,
                may_change_directory_tree,
                identifies_folder,
                removes_path,
                &filter,
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
    removes_path: bool,
    filter: &NativeEventFilter<'_>,
) -> bool {
    if filter.skip_hidden_paths && path_has_hidden_component(path, filter.watch_roots) {
        return false;
    }

    if filter.skip_package_paths && path_has_package_component(path, filter.watch_roots) {
        return false;
    }

    if filter.supported_extensions.contains_path(path) {
        return true;
    }

    if !may_change_directory_tree {
        return false;
    }

    if removes_path {
        return true;
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
    batch_sender: &SyncSender<RawChangeBatch>,
    notifier: Option<&Arc<dyn Fn() + Send + Sync + 'static>>,
    overflow_paths: &[PathBuf],
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

    match batch_sender.try_send(batch) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            mark_pending_overflowed(pending_paths, pending_overflowed, overflow_paths);
            let now = Instant::now();
            *first_event_at = Some(now);
            *last_event_at = Some(now);
            return true;
        }
        Err(TrySendError::Disconnected(_)) => return false,
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

    use super::{
        NativeEventSender, NativeFsEvent, ScriptMetaKitError, ScriptMetaKitResult, WatchPlan,
    };

    pub struct PlatformWatcher {
        paths: cf::CFMutableArrayRef,
        run_loop: Arc<Mutex<Option<CFSendWrapper>>>,
        stop_source: Arc<Mutex<Option<CFSendWrapper>>>,
        worker: Option<JoinHandle<()>>,
    }

    struct StreamContext {
        sender: NativeEventSender,
    }

    struct CFSendWrapper(usize);

    unsafe impl Send for CFSendWrapper {}

    type CFRunLoopSourceRef = cf::CFRef;
    type CFHashCode = std::os::raw::c_ulong;

    #[repr(C)]
    struct CFRunLoopSourceContext {
        version: cf::CFIndex,
        info: *mut c_void,
        retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
        release: Option<extern "C" fn(*const c_void)>,
        copy_description: Option<extern "C" fn(*const c_void) -> cf::CFStringRef>,
        equal: Option<extern "C" fn(*const c_void, *const c_void) -> cf::Boolean>,
        hash: Option<extern "C" fn(*const c_void) -> CFHashCode>,
        schedule: Option<extern "C" fn(*mut c_void, cf::CFRunLoopRef, cf::CFStringRef)>,
        cancel: Option<extern "C" fn(*mut c_void, cf::CFRunLoopRef, cf::CFStringRef)>,
        perform: Option<extern "C" fn(*mut c_void)>,
    }

    unsafe extern "C" {
        fn CFRunLoopRun();
        fn CFRunLoopAddSource(
            run_loop: cf::CFRunLoopRef,
            source: CFRunLoopSourceRef,
            mode: cf::CFStringRef,
        );
        fn CFRunLoopRemoveSource(
            run_loop: cf::CFRunLoopRef,
            source: CFRunLoopSourceRef,
            mode: cf::CFStringRef,
        );
        fn CFRunLoopSourceCreate(
            allocator: cf::CFAllocatorRef,
            order: cf::CFIndex,
            context: *mut CFRunLoopSourceContext,
        ) -> CFRunLoopSourceRef;
        fn CFRunLoopSourceSignal(source: CFRunLoopSourceRef);
        fn CFRunLoopWakeUp(run_loop: cf::CFRunLoopRef);
    }

    extern "C" fn stop_current_run_loop(_info: *mut c_void) {
        unsafe {
            cf::CFRunLoopStop(cf::CFRunLoopGetCurrent());
        }
    }

    impl PlatformWatcher {
        pub fn start(
            plan: &WatchPlan,
            event_sender: NativeEventSender,
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
                    stop_source: Arc::new(Mutex::new(None)),
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
            let stop_source = Arc::new(Mutex::new(None));
            let worker_stop_source = Arc::clone(&stop_source);
            let (start_sender, start_receiver) = mpsc::sync_channel(1);
            let worker = match thread::Builder::new()
                .name("scriptmetakit-fsevents".to_string())
                .spawn(move || {
                    let stream = stream.0 as fs::FSEventStreamRef;
                    unsafe {
                        let run_loop = cf::CFRunLoopGetCurrent();
                        let mut stop_context = CFRunLoopSourceContext {
                            version: 0,
                            info: ptr::null_mut(),
                            retain: None,
                            release: None,
                            copy_description: None,
                            equal: None,
                            hash: None,
                            schedule: None,
                            cancel: None,
                            perform: Some(stop_current_run_loop),
                        };
                        let stop_source =
                            CFRunLoopSourceCreate(cf::kCFAllocatorDefault, 0, &mut stop_context);
                        if stop_source.is_null() {
                            let _ = start_sender
                                .send(Err("failed to create FSEvents stop source".to_string()));
                            fs::FSEventStreamRelease(stream);
                            return;
                        }
                        CFRunLoopAddSource(run_loop, stop_source, cf::kCFRunLoopDefaultMode);
                        if let Ok(mut stored_run_loop) = worker_run_loop.lock() {
                            *stored_run_loop = Some(CFSendWrapper(run_loop as usize));
                        }
                        if let Ok(mut stored_stop_source) = worker_stop_source.lock() {
                            *stored_stop_source = Some(CFSendWrapper(stop_source as usize));
                        }
                        fs::FSEventStreamScheduleWithRunLoop(
                            stream,
                            run_loop,
                            cf::kCFRunLoopDefaultMode,
                        );
                        let started = fs::FSEventStreamStart(stream) != 0;
                        if started {
                            let _ = start_sender.send(Ok(()));
                            CFRunLoopRun();
                        } else {
                            let _ = start_sender
                                .send(Err("failed to start FSEvents stream".to_string()));
                        }
                        if started {
                            fs::FSEventStreamStop(stream);
                        }
                        fs::FSEventStreamInvalidate(stream);
                        fs::FSEventStreamRelease(stream);
                        CFRunLoopRemoveSource(run_loop, stop_source, cf::kCFRunLoopDefaultMode);
                        cf::CFRelease(stop_source);
                        if let Ok(mut stored_run_loop) = worker_run_loop.lock() {
                            *stored_run_loop = None;
                        }
                        if let Ok(mut stored_stop_source) = worker_stop_source.lock() {
                            *stored_stop_source = None;
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
                stop_source,
                worker: Some(worker),
            })
        }
    }

    impl Drop for PlatformWatcher {
        fn drop(&mut self) {
            let stop_source = self
                .stop_source
                .lock()
                .ok()
                .and_then(|mut stored_stop_source| stored_stop_source.take());
            if let Ok(mut stored_run_loop) = self.run_loop.lock()
                && let Some(run_loop) = stored_run_loop.take()
            {
                unsafe {
                    if let Some(stop_source) = stop_source {
                        CFRunLoopSourceSignal(stop_source.0 as CFRunLoopSourceRef);
                    } else {
                        cf::CFRunLoopStop(run_loop.0 as cf::CFRunLoopRef);
                    }
                    CFRunLoopWakeUp(run_loop.0 as cf::CFRunLoopRef);
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
            removes_path: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
    };

    use super::{NativeEventSender, NativeFsEvent, flush_pending_batch};
    use crate::watcher::RawChangeBatch;

    #[test]
    fn native_event_queue_marks_overflow_instead_of_growing() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let overflowed = Arc::new(AtomicBool::new(false));
        let sender = NativeEventSender {
            sender,
            overflowed: Arc::clone(&overflowed),
        };

        sender.send(NativeFsEvent::Overflow).expect("first event");
        sender
            .send(NativeFsEvent::Overflow)
            .expect("coalesced overflow");

        assert!(overflowed.load(Ordering::Acquire));
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn full_batch_queue_keeps_one_overflow_batch_pending() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        sender
            .try_send(RawChangeBatch {
                paths: vec![PathBuf::from("/queued")],
                overflowed: false,
            })
            .expect("queued batch");
        let overflow_paths = vec![PathBuf::from("/root")];
        let mut pending_paths = vec![PathBuf::from("/changed")];
        let mut pending_overflowed = false;
        let mut first_event_at = None;
        let mut last_event_at = None;

        assert!(flush_pending_batch(
            &sender,
            None,
            &overflow_paths,
            &mut pending_paths,
            &mut pending_overflowed,
            &mut first_event_at,
            &mut last_event_at,
        ));
        assert_eq!(pending_paths, overflow_paths);
        assert!(pending_overflowed);
        assert!(first_event_at.is_some());
        assert!(last_event_at.is_some());
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        ffi::{OsString, c_void},
        mem,
        os::windows::ffi::{OsStrExt, OsStringExt},
        path::{Path, PathBuf},
        ptr,
        sync::mpsc,
        thread::{self, JoinHandle},
    };

    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE, TRUE, WAIT_FAILED, WAIT_OBJECT_0,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_ACTION_ADDED, FILE_ACTION_REMOVED, FILE_ACTION_RENAMED_NEW_NAME,
            FILE_ACTION_RENAMED_OLD_NAME, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
            FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_CREATION,
            FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
            FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SECURITY, FILE_NOTIFY_CHANGE_SIZE,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
            ReadDirectoryChangesW,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Threading::{
                CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects, WaitForSingleObject,
            },
        },
    };

    use super::{
        NativeEventSender, NativeFsEvent, ScriptMetaKitError, ScriptMetaKitResult, WatchPlan,
    };

    const BUFFER_SIZE: usize = 64 * 1024;
    const RETRY_OPEN_DELAY_MILLIS: u32 = 1_000;

    pub struct PlatformWatcher {
        stop_events: Vec<isize>,
        workers: Vec<JoinHandle<()>>,
    }

    impl PlatformWatcher {
        pub fn start(
            plan: &WatchPlan,
            event_sender: NativeEventSender,
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

    fn watch_root(root: PathBuf, directory: HANDLE, stop_event: HANDLE, sender: NativeEventSender) {
        let mut directory = Some(directory);
        loop {
            let Some(directory_handle) = directory else {
                break;
            };
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
                    directory_handle,
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
                directory =
                    reopen_directory_after_error(&root, directory.take().unwrap(), stop_event);
                if directory.is_none() {
                    break;
                }
                continue;
            }

            let wait_handles = [stop_event, read_event];
            let wait_result =
                unsafe { WaitForMultipleObjects(2, wait_handles.as_ptr(), FALSE, INFINITE) };
            if wait_result == WAIT_OBJECT_0 {
                cancel_pending_read(directory_handle, &overlapped);
                unsafe { CloseHandle(read_event) };
                break;
            }
            if wait_result == WAIT_FAILED || wait_result != WAIT_OBJECT_0 + 1 {
                cancel_pending_read(directory_handle, &overlapped);
                unsafe { CloseHandle(read_event) };
                let _ = sender.send(NativeFsEvent::Overflow);
                directory =
                    reopen_directory_after_error(&root, directory.take().unwrap(), stop_event);
                if directory.is_none() {
                    break;
                }
                continue;
            }

            let mut transferred = 0u32;
            let completed = unsafe {
                GetOverlappedResult(directory_handle, &overlapped, &mut transferred, FALSE)
            };
            unsafe {
                CloseHandle(read_event);
            }

            if completed == 0 {
                let _ = sender.send(NativeFsEvent::Overflow);
                directory =
                    reopen_directory_after_error(&root, directory.take().unwrap(), stop_event);
                if directory.is_none() {
                    break;
                }
                continue;
            }

            if transferred == 0 {
                let _ = sender.send(NativeFsEvent::Overflow);
                continue;
            }

            parse_change_buffer(&root, &buffer[..transferred as usize], &sender);
        }

        if let Some(directory) = directory {
            unsafe {
                CloseHandle(directory);
            }
        }
    }

    fn reopen_directory_after_error(
        root: &Path,
        directory: HANDLE,
        stop_event: HANDLE,
    ) -> Option<HANDLE> {
        unsafe {
            CloseHandle(directory);
        }

        loop {
            let wait_result = unsafe { WaitForSingleObject(stop_event, RETRY_OPEN_DELAY_MILLIS) };
            if wait_result == WAIT_OBJECT_0 || wait_result == WAIT_FAILED {
                return None;
            }
            if let Ok(directory) = open_directory(root) {
                return Some(directory);
            }
        }
    }

    fn cancel_pending_read(directory: HANDLE, overlapped: &OVERLAPPED) {
        unsafe {
            CancelIoEx(directory, overlapped);
            let mut transferred = 0u32;
            let _ = GetOverlappedResult(directory, overlapped, &mut transferred, TRUE);
        }
    }

    fn parse_change_buffer(root: &Path, buffer: &[u8], sender: &NativeEventSender) {
        const FILE_NOTIFY_INFORMATION_HEADER_SIZE: usize = 12;

        let mut offset = 0usize;
        while offset < buffer.len() {
            if buffer.len() - offset < FILE_NOTIFY_INFORMATION_HEADER_SIZE {
                let _ = sender.send(NativeFsEvent::Overflow);
                break;
            }
            let next_entry_offset = u32::from_ne_bytes(
                buffer[offset..offset + 4]
                    .try_into()
                    .expect("FILE_NOTIFY_INFORMATION next offset bytes"),
            );
            let action = u32::from_ne_bytes(
                buffer[offset + 4..offset + 8]
                    .try_into()
                    .expect("FILE_NOTIFY_INFORMATION action bytes"),
            );
            let file_name_byte_len = u32::from_ne_bytes(
                buffer[offset + 8..offset + 12]
                    .try_into()
                    .expect("FILE_NOTIFY_INFORMATION file name length bytes"),
            ) as usize;
            if file_name_byte_len % 2 != 0 {
                let _ = sender.send(NativeFsEvent::Overflow);
                break;
            }
            let name_offset = offset + FILE_NOTIFY_INFORMATION_HEADER_SIZE;
            if name_offset + file_name_byte_len > buffer.len() {
                let _ = sender.send(NativeFsEvent::Overflow);
                break;
            }
            let name_wide = buffer[name_offset..name_offset + file_name_byte_len]
                .chunks_exact(2)
                .map(|code_unit| u16::from_le_bytes([code_unit[0], code_unit[1]]))
                .collect::<Vec<_>>();
            let name = OsString::from_wide(&name_wide);
            let path = root.join(PathBuf::from(name));
            let may_change_directory_tree = matches!(
                action,
                FILE_ACTION_ADDED
                    | FILE_ACTION_REMOVED
                    | FILE_ACTION_RENAMED_OLD_NAME
                    | FILE_ACTION_RENAMED_NEW_NAME
            );
            let removes_path = matches!(action, FILE_ACTION_REMOVED | FILE_ACTION_RENAMED_OLD_NAME);

            let _ = sender.send(NativeFsEvent::Changed {
                path,
                may_change_directory_tree,
                identifies_folder: false,
                removes_path,
            });

            if next_entry_offset == 0 {
                break;
            }
            let next_offset = offset + next_entry_offset as usize;
            if next_offset <= offset || next_offset > buffer.len() {
                let _ = sender.send(NativeFsEvent::Overflow);
                break;
            }
            offset = next_offset;
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use super::{NativeEventSender, ScriptMetaKitError, ScriptMetaKitResult, WatchPlan};

    pub struct PlatformWatcher;

    impl PlatformWatcher {
        pub fn start(
            _plan: &WatchPlan,
            _event_sender: NativeEventSender,
        ) -> ScriptMetaKitResult<Self> {
            Err(ScriptMetaKitError::InvalidConfig(
                "native watcher is implemented for macOS and Windows".to_string(),
            ))
        }
    }
}
