use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ItemId, RootId,
    catalog::{CacheInvalidationReason, CacheScope, ScanMode, ScanResult, UpdateCheckResult},
    watcher::{RootChangeBatch, WatchPlan},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ScriptMetaKitEvent {
    RootRegistered {
        root_id: RootId,
    },
    RootRemoved {
        root_id: RootId,
    },
    WatchStarted {
        plan: WatchPlan,
    },
    WatchStopped,
    ChangeDetected {
        batch: RootChangeBatch,
    },
    RootMarkedDirty {
        root_id: RootId,
    },
    ScanStarted {
        root_ids: Vec<RootId>,
        mode: ScanMode,
    },
    ScanFinished {
        result: Box<ScanResult>,
    },
    FileListChanged {
        root_id: RootId,
    },
    CatalogChanged {
        source_revision: Uuid,
    },
    CacheLoaded {
        scope: CacheScope,
    },
    CacheSaved {
        scope: CacheScope,
    },
    CacheInvalidated {
        scope: CacheScope,
        reason: CacheInvalidationReason,
    },
    UpdateCheckStarted {
        item_count: usize,
    },
    UpdateCheckProgress {
        progress: ProgressUpdate,
    },
    UpdateCheckFinished {
        result: Box<UpdateCheckResult>,
    },
    RootMissing {
        root_id: RootId,
    },
    RootUnreadable {
        root_id: RootId,
    },
    ScanTimedOut {
        root_id: RootId,
    },
    WatchOverflowed {
        affected_roots: Vec<RootId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProgressUpdate {
    pub id: String,
    pub text: String,
    pub state: ProgressState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressState {
    Active,
    ActiveError,
    PersistentError,
    Finished,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateCheckProgress {
    pub completed_items: usize,
    pub total_items: usize,
    pub item_id: Option<ItemId>,
    pub script_id: Option<String>,
    pub phase: UpdateCheckProgressPhase,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCheckProgressPhase {
    Started,
    Checking,
    Retrying,
    FinishedItem,
    FailedItem,
    Cancelled,
    Finished,
}
