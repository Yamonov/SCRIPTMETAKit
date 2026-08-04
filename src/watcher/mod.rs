#[cfg(feature = "native-watch")]
mod native;
mod plan;
mod policy;

#[cfg(feature = "native-watch")]
pub use native::NativeWatcher;
pub use plan::{
    DEFAULT_DEBOUNCE_DELAY_MILLIS, DEFAULT_MAX_DELIVERY_DELAY_MILLIS, DEFAULT_MAX_PENDING_PATHS,
    DEFAULT_NATIVE_EVENT_LATENCY_MILLIS, IgnoredWatchPath, LogicalWatchRoot, PhysicalWatchRoot,
    RawChangeBatch, RootChange, RootChangeBatch, WatchIgnoreReason, WatchPathEvent,
    WatchPathEventKind, WatchPlan, WatchRenameCandidate, WatchRenameConfidence, WatchRescanReason,
    WatchRescanTarget, build_watch_plan,
};
pub use policy::{MonitorRootStrategy, OverflowPolicy, WatchPolicy};

pub(crate) use plan::{
    ChangeRoutingOptions, build_watch_plan_with_resolved_targets, normalize_path,
    route_change_batch,
};
