#![doc = "Private safe wrapper around macOS Dispatch I/O for SCRIPTMETAKit."]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg(target_os = "macos")]

use std::{
    ffi::{CString, c_char, c_int, c_void},
    io,
    os::unix::ffi::OsStrExt,
    path::Path,
    ptr::NonNull,
    slice,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvError, RecvTimeoutError, Sender},
    },
    time::Duration,
};

unsafe extern "C" {
    fn smk_dispatch_io_open(
        path: *const c_char,
        high_water: usize,
        data_context: *mut c_void,
        cleanup_context: *mut c_void,
        data_handler: unsafe extern "C" fn(*mut c_void, *const c_void, usize, bool, c_int),
        cleanup_handler: unsafe extern "C" fn(*mut c_void, c_int),
    ) -> *mut c_void;
    fn smk_dispatch_io_cancel(channel: *mut c_void);
    fn smk_dispatch_io_retain(channel: *mut c_void);
    fn smk_dispatch_io_release(channel: *mut c_void);
}

/// One result delivered by a cancelable file read.
#[derive(Debug)]
pub enum ReadEvent {
    Data(Vec<u8>),
    Complete(io::Result<()>),
}

struct CallbackState {
    sender: Sender<ReadEvent>,
    completed: AtomicBool,
    cleaned: Mutex<bool>,
    cleanup_notification: Condvar,
}

unsafe extern "C" fn receive_data(
    context: *mut c_void,
    bytes: *const c_void,
    length: usize,
    done: bool,
    error: c_int,
) {
    // SAFETY: this raw Arc reference belongs only to the read operation. It
    // remains alive through every data callback and is reclaimed below when
    // Dispatch marks that operation done, independently of channel cleanup.
    let state = unsafe { &*context.cast::<CallbackState>() };
    if length > 0 && !bytes.is_null() {
        // SAFETY: Dispatch guarantees this buffer is readable for the duration
        // of the callback. Copy it before returning to Dispatch.
        let bytes = unsafe { slice::from_raw_parts(bytes.cast::<u8>(), length) };
        let _ = state.sender.send(ReadEvent::Data(bytes.to_vec()));
    }
    if done && !state.completed.swap(true, Ordering::AcqRel) {
        let result = if error == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(error))
        };
        let _ = state.sender.send(ReadEvent::Complete(result));
    }
    if done {
        // SAFETY: Dispatch completes one read operation exactly once. This
        // balances the data-context reference created by `Arc::into_raw`.
        drop(unsafe { Arc::from_raw(context.cast::<CallbackState>()) });
    }
}

unsafe extern "C" fn finish_cleanup(context: *mut c_void, error: c_int) {
    // SAFETY: this raw Arc reference belongs only to the channel cleanup
    // callback and remains owned by Dispatch throughout this call.
    let state = unsafe { &*context.cast::<CallbackState>() };
    if error != 0 && !state.completed.swap(true, Ordering::AcqRel) {
        let _ = state
            .sender
            .send(ReadEvent::Complete(Err(io::Error::from_raw_os_error(
                error,
            ))));
    }
    *state
        .cleaned
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
    state.cleanup_notification.notify_all();
    // SAFETY: the raw context owns exactly one Arc reference created by
    // `Arc::into_raw`. Dispatch invokes this cleanup callback once after it has
    // finished scheduling data callbacks for the channel.
    drop(unsafe { Arc::from_raw(context.cast::<CallbackState>()) });
}

/// A macOS file read backed by Dispatch I/O.
///
/// Dropping or explicitly cancelling the reader closes the channel with
/// `DISPATCH_IO_STOP`, allowing the operating system to interrupt an outstanding
/// open or read instead of leaving a blocked worker thread behind.
pub struct CancelableFileReader {
    channel: NonNull<c_void>,
    receiver: Receiver<ReadEvent>,
    closed: Arc<AtomicBool>,
    state: Arc<CallbackState>,
}

impl CancelableFileReader {
    pub fn open(path: &Path, chunk_bytes: usize) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Dispatch I/O requires an absolute file path",
            ));
        }
        if chunk_bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Dispatch I/O chunk size must be greater than zero",
            ));
        }

        let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "file path contains a NUL byte")
        })?;
        let (sender, receiver) = mpsc::channel();
        let state = Arc::new(CallbackState {
            sender,
            completed: AtomicBool::new(false),
            cleaned: Mutex::new(false),
            cleanup_notification: Condvar::new(),
        });
        let data_context = Arc::into_raw(Arc::clone(&state))
            .cast_mut()
            .cast::<c_void>();
        let cleanup_context = Arc::into_raw(Arc::clone(&state))
            .cast_mut()
            .cast::<c_void>();

        // SAFETY: The path is a valid absolute C string for this call. The raw
        // Arc context remains owned until Dispatch's cleanup callback, and the
        // callback signatures exactly match the C shim.
        let channel = unsafe {
            smk_dispatch_io_open(
                path.as_ptr(),
                chunk_bytes,
                data_context,
                cleanup_context,
                receive_data,
                finish_cleanup,
            )
        };
        let Some(channel) = NonNull::new(channel) else {
            // SAFETY: A NULL channel means Dispatch did not accept the operation
            // and therefore will not invoke the cleanup callback.
            drop(unsafe { Arc::from_raw(data_context.cast::<CallbackState>()) });
            // SAFETY: a NULL channel means the cleanup callback will not own
            // or reclaim its separate raw reference either.
            drop(unsafe { Arc::from_raw(cleanup_context.cast::<CallbackState>()) });
            return Err(io::Error::other("could not create a Dispatch I/O channel"));
        };

        Ok(Self {
            channel,
            receiver,
            closed: Arc::new(AtomicBool::new(false)),
            state,
        })
    }

    pub fn receive_timeout(&self, timeout: Duration) -> Result<ReadEvent, RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }

    pub fn receive(&self) -> Result<ReadEvent, RecvError> {
        self.receiver.recv()
    }

    pub fn cancel(&self) {
        self.cancellation_handle().cancel();
    }

    #[must_use]
    pub fn cancellation_handle(&self) -> CancelableFileReaderCancellation {
        // SAFETY: the reader owns a live channel. The returned handle balances
        // this retain in its destructor.
        unsafe { smk_dispatch_io_retain(self.channel.as_ptr()) };
        CancelableFileReaderCancellation {
            channel: self.channel.as_ptr() as usize,
            closed: Arc::clone(&self.closed),
            state: Arc::clone(&self.state),
        }
    }
}

pub struct CancelableFileReaderCancellation {
    channel: usize,
    closed: Arc<AtomicBool>,
    state: Arc<CallbackState>,
}

impl Clone for CancelableFileReaderCancellation {
    fn clone(&self) -> Self {
        // SAFETY: every live cancellation handle retains the channel.
        unsafe { smk_dispatch_io_retain(self.channel as *mut c_void) };
        Self {
            channel: self.channel,
            closed: Arc::clone(&self.closed),
            state: Arc::clone(&self.state),
        }
    }
}

impl CancelableFileReaderCancellation {
    pub fn cancel(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            if self.state.completed.load(Ordering::Acquire) {
                return;
            }
            // SAFETY: the owning reader retains the Dispatch channel until the
            // cancellation listener is dropped before the reader itself.
            unsafe { smk_dispatch_io_cancel(self.channel as *mut c_void) };
        }
    }
}

impl Drop for CancelableFileReaderCancellation {
    fn drop(&mut self) {
        // SAFETY: this balances the retain performed when the handle was
        // created or cloned.
        unsafe { smk_dispatch_io_release(self.channel as *mut c_void) };
    }
}

impl Drop for CancelableFileReader {
    fn drop(&mut self) {
        self.cancel();
        // SAFETY: This releases the +1 reference returned by the C shim exactly
        // once. Dispatch keeps its operation alive until cleanup completes.
        unsafe { smk_dispatch_io_release(self.channel.as_ptr()) };
        let cleaned = self
            .state
            .cleaned
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !*cleaned {
            drop(
                self.state
                    .cleanup_notification
                    .wait_timeout(cleaned, Duration::from_millis(100))
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            );
        } else {
            drop(cleaned);
        }
    }
}
