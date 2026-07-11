use std::time::Duration;
use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
};

use serde::{Deserialize, Serialize};

#[derive(Default)]
struct OperationCancellationState {
    flags: AtomicU8,
    wait_lock: Mutex<()>,
    wait_condition: Condvar,
    next_listener_id: AtomicU64,
    listeners: Mutex<BTreeMap<u64, Arc<dyn Fn() + Send + Sync>>>,
}

const OPERATION_ACTIVE: u8 = 1 << 0;
const OPERATION_CANCELLED: u8 = 1 << 1;
const OPERATION_CANCEL_PENDING: u8 = 1 << 2;
const OPERATION_RESERVED: u8 = 1 << 3;

#[derive(Clone, Debug, Default)]
pub struct OperationCancellation {
    state: Arc<OperationCancellationState>,
}

#[derive(Debug)]
pub(crate) struct OperationCancellationScope {
    cancellation: OperationCancellation,
}

#[derive(Debug)]
pub struct OperationCancellationReservation {
    cancellation: OperationCancellation,
}

pub(crate) struct OperationCancellationListener {
    state: Arc<OperationCancellationState>,
    id: u64,
}

impl fmt::Debug for OperationCancellationState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OperationCancellationState")
            .field("flags", &self.flags.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl OperationCancellation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserves the next operation so cancellation requested during the
    /// Swift/FFI hand-off can only apply to that operation.
    #[must_use]
    pub fn reserve_next_operation(&self) -> OperationCancellationReservation {
        self.state
            .flags
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |flags| {
                Some((flags & !OPERATION_CANCEL_PENDING) | OPERATION_RESERVED)
            })
            .expect("operation cancellation reservation cannot fail");
        OperationCancellationReservation {
            cancellation: self.clone(),
        }
    }

    pub(crate) fn begin_scope(&self) -> OperationCancellationScope {
        self.state
            .flags
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |flags| {
                let mut next = OPERATION_ACTIVE;
                if flags & OPERATION_CANCEL_PENDING != 0 {
                    next |= OPERATION_CANCELLED;
                }
                Some(next)
            })
            .expect("operation cancellation state update cannot fail");
        OperationCancellationScope {
            cancellation: self.clone(),
        }
    }

    pub fn cancel(&self) {
        let wait_guard = self
            .state
            .wait_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.state
            .flags
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |flags| {
                Some(if flags & OPERATION_ACTIVE != 0 {
                    flags | OPERATION_CANCELLED
                } else {
                    flags
                })
            })
            .expect("operation cancellation state update cannot fail");
        self.state.wait_condition.notify_all();
        drop(wait_guard);
        self.notify_cancel_listeners();
    }

    /// Cancels the active operation, or the explicitly reserved next operation.
    /// This is for the Swift Task to FFI hand-off only; public "cancel current"
    /// calls must use [`Self::cancel`] so an idle cancel remains a no-op.
    pub fn cancel_current_or_reserved(&self) {
        let wait_guard = self
            .state
            .wait_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.state
            .flags
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |flags| {
                Some(if flags & OPERATION_ACTIVE != 0 {
                    flags | OPERATION_CANCELLED
                } else if flags & OPERATION_RESERVED != 0 {
                    flags | OPERATION_CANCEL_PENDING
                } else {
                    flags
                })
            })
            .expect("operation cancellation state update cannot fail");
        self.state.wait_condition.notify_all();
        drop(wait_guard);
        self.notify_cancel_listeners();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.flags.load(Ordering::Acquire) & OPERATION_CANCELLED != 0
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.state.flags.load(Ordering::Acquire) & OPERATION_ACTIVE != 0
    }

    pub(crate) fn wait_for_cancellation(&self, timeout: Duration) -> bool {
        if self.is_cancelled() {
            return true;
        }
        let guard = self
            .state
            .wait_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.is_cancelled() {
            return true;
        }
        drop(
            self.state
                .wait_condition
                .wait_timeout_while(guard, timeout, |_| !self.is_cancelled())
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        self.is_cancelled()
    }

    pub(crate) fn register_cancel_listener(
        &self,
        listener: impl Fn() + Send + Sync + 'static,
    ) -> OperationCancellationListener {
        let listener = Arc::new(listener) as Arc<dyn Fn() + Send + Sync>;
        let id = self.state.next_listener_id.fetch_add(1, Ordering::Relaxed);
        self.state
            .listeners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id, Arc::clone(&listener));
        if self.is_cancelled() {
            listener();
        }
        OperationCancellationListener {
            state: Arc::clone(&self.state),
            id,
        }
    }

    fn notify_cancel_listeners(&self) {
        let listeners = self
            .state
            .listeners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for listener in listeners {
            listener();
        }
    }
}

impl Drop for OperationCancellationListener {
    fn drop(&mut self) {
        self.state
            .listeners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.id);
    }
}

impl Drop for OperationCancellationScope {
    fn drop(&mut self) {
        self.cancellation
            .state
            .flags
            .fetch_and(!(OPERATION_ACTIVE | OPERATION_CANCELLED), Ordering::AcqRel);
    }
}

impl Drop for OperationCancellationReservation {
    fn drop(&mut self) {
        self.cancellation.state.flags.fetch_and(
            !(OPERATION_RESERVED | OPERATION_CANCEL_PENDING),
            Ordering::AcqRel,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::OperationCancellation;

    #[test]
    fn idle_cancellation_does_not_cancel_the_next_scope() {
        let cancellation = OperationCancellation::new();
        cancellation.cancel();

        let scope = cancellation.begin_scope();
        assert!(cancellation.is_active());
        assert!(!cancellation.is_cancelled());

        drop(scope);
        assert!(!cancellation.is_active());
        assert!(!cancellation.is_cancelled());
    }

    #[test]
    fn cancellation_of_a_reserved_operation_is_consumed_by_that_scope() {
        let cancellation = OperationCancellation::new();
        let reservation = cancellation.reserve_next_operation();
        cancellation.cancel_current_or_reserved();

        let scope = cancellation.begin_scope();
        assert!(cancellation.is_active());
        assert!(cancellation.is_cancelled());

        drop(scope);
        drop(reservation);
        let _next_scope = cancellation.begin_scope();
        assert!(!cancellation.is_cancelled());
    }

    #[test]
    fn dropping_an_unused_reservation_clears_pending_cancellation() {
        let cancellation = OperationCancellation::new();
        let reservation = cancellation.reserve_next_operation();
        cancellation.cancel_current_or_reserved();
        drop(reservation);

        let _scope = cancellation.begin_scope();
        assert!(!cancellation.is_cancelled());
    }

    #[test]
    fn cancellation_is_scoped_to_one_operation() {
        let cancellation = OperationCancellation::new();
        {
            let _scope = cancellation.begin_scope();
            cancellation.cancel();
            assert!(cancellation.is_cancelled());
        }

        let _next_scope = cancellation.begin_scope();
        assert!(!cancellation.is_cancelled());
    }

    #[test]
    fn cancellation_wakes_a_timed_wait_without_polling() {
        let cancellation = OperationCancellation::new();
        let _scope = cancellation.begin_scope();
        let request = cancellation.clone();
        let canceller = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            request.cancel();
        });

        assert!(cancellation.wait_for_cancellation(Duration::from_secs(1)));
        canceller.join().expect("canceller");
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationSummary {
    pub status: OperationStatus,
    pub total_units: usize,
    pub completed_units: usize,
    pub failed_units: usize,
    #[serde(default)]
    pub cancelled: bool,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default)]
    pub reason_code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

impl OperationSummary {
    #[must_use]
    pub fn finished(total_units: usize, completed_units: usize, failed_units: usize) -> Self {
        Self {
            status: OperationStatus::Finished,
            total_units,
            completed_units,
            failed_units,
            cancelled: false,
            timed_out: false,
            reason_code: None,
            message: None,
        }
    }

    #[must_use]
    pub fn cancelled(total_units: usize, completed_units: usize, failed_units: usize) -> Self {
        Self {
            status: OperationStatus::Cancelled,
            total_units,
            completed_units,
            failed_units,
            cancelled: true,
            timed_out: false,
            reason_code: Some("operation_cancelled".to_string()),
            message: Some("operation was cancelled".to_string()),
        }
    }

    #[must_use]
    pub fn timed_out(total_units: usize, completed_units: usize, failed_units: usize) -> Self {
        Self {
            status: OperationStatus::TimedOut,
            total_units,
            completed_units,
            failed_units,
            cancelled: false,
            timed_out: true,
            reason_code: Some("operation_timed_out".to_string()),
            message: Some("operation timed out before all work completed".to_string()),
        }
    }

    #[must_use]
    pub fn partial(total_units: usize, completed_units: usize, failed_units: usize) -> Self {
        Self {
            status: OperationStatus::Partial,
            total_units,
            completed_units,
            failed_units,
            cancelled: false,
            timed_out: false,
            reason_code: Some("operation_partial".to_string()),
            message: Some("operation completed with incomplete roots".to_string()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    #[default]
    Finished,
    Cancelled,
    TimedOut,
    Partial,
}

impl OperationStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Partial => "partial",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileIssue {
    pub root_id: Option<String>,
    pub path: std::path::PathBuf,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub path_kind: Option<String>,
    #[serde(default)]
    pub resolution_status: Option<String>,
    #[serde(default)]
    pub is_directory: bool,
}
