mod file_list;
mod metadata_scan;
mod options;
mod path_resolution;
mod root_preflight;

pub use crate::formats::{ScriptFileInfo, detect_script_file};
pub use file_list::{
    DirectoryScanOutput, FileSystemEntry, scan_file_list_root,
    scan_file_list_root_with_dirty_directories,
};
pub(crate) use file_list::{
    scan_file_list_root_controlled, scan_file_list_root_with_dirty_directories_controlled,
    try_scan_file_list_root_with_owned_dirty_directories_controlled,
};
pub use metadata_scan::{
    CandidateCache, CandidateRecord, MetadataScanOutput, RegisteredRootSignature,
    scan_metadata_roots, scan_metadata_roots_scoped,
};
pub(crate) use metadata_scan::{
    deduplicated_items, file_items_from_cache, registered_root_signatures,
    scan_metadata_roots_scoped_controlled,
};
pub use options::{ExtensionPolicy, RootPreflightOptions, ScannerOptions};
pub use path_resolution::{
    PathKind, PathResolutionStatus, ScannablePathResolution, resolve_registered_path,
};
pub use root_preflight::can_read_directory_contents;
