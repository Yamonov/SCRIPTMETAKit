mod distribution;
mod error;
mod metadata;
mod operation;
mod parser;
mod text;
mod version;

pub use distribution::{DistributionMetadata, DistributionResolution};
pub use error::{ScriptMetaKitError, ScriptMetaKitResult};
pub use metadata::{
    ParserOptions, ScriptMetaEditCapability, ScriptMetaEditState, ScriptMetaItem,
    ScriptMetaItemRef, ScriptMetadata, ScriptRuntimeKind,
};
pub use operation::{FileIssue, OperationCancellation, OperationStatus, OperationSummary};
pub(crate) use parser::select_distribution_metadata_for_script;
pub use parser::{
    normalize_metadata_url, parse_distribution_metadata, parse_distribution_metadata_for_script,
    parse_distribution_metadata_records, parse_script_metadata,
};
pub use text::{
    DecodedScriptText, ScriptTextEncoding, decode_script_text, decode_script_text_strict,
    decode_script_text_with_encoding, encode_script_text,
};
pub use version::{VersionOrdering, compare_versions, normalize_version_string};
