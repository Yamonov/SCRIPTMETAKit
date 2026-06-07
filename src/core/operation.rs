use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
pub struct OperationCancellation {
    cancelled: Arc<AtomicBool>,
}

impl OperationCancellation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
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
