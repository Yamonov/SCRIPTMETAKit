mod config;
mod event;
mod root;
mod snapshot;

pub use config::{CacheOptions, ScriptMetaKitConfig, UpdateCheckOptions, WatcherOptions};
pub use event::{
    ProgressUpdate, ScriptMetaKitEvent, UpdateCheckProgress, UpdateCheckProgressPhase,
};
pub use root::{
    CachePolicy, RefreshPolicy, RootPriority, RootPurpose, RootRegistration, path_based_root_id,
};
pub use snapshot::{
    CacheInvalidationReason, CacheScope, DirectoryState, DirectoryStateMap, FileEntryChange,
    FileEntryChangeKind, FileIdentity, FileListSnapshot, RefreshRequest, RootError, RootSnapshot,
    RootStatus, ScanChangeSummary, ScanMode, ScanRequest, ScanResult, ScriptMetaCatalogSnapshot,
    UpdateCheckRequest, UpdateCheckResult, UpdateFailure, UpdateStatus, unresolved_distribution,
};
