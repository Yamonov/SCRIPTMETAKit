mod distribution;
mod error;
mod localization;
mod metadata;
mod metadata_sections;
mod operation;
mod parser;
mod text;
mod version;

pub use distribution::{DistributionMetadata, DistributionResolution};
pub use error::{ScriptMetaKitError, ScriptMetaKitResult};
pub use localization::{
    LanguagePreferences, MetadataDiagnostic, MetadataEditExtensions, MetadataPresentation,
    is_valid_metadata_tag, normalize_language_tag, normalize_metadata_tags,
};
pub(crate) use localization::{
    append_value, description_starts_with_language_label, description_translations, diagnostic,
    is_block_marker, item_separator, select_text,
};
pub use metadata::{
    ParserOptions, ScriptMetaEditCapability, ScriptMetaEditState, ScriptMetaItem,
    ScriptMetaItemRef, ScriptMetadata, ScriptRuntimeKind,
};
pub(crate) use metadata_sections::metadata_sections;
#[cfg(feature = "blocking-http")]
pub(crate) use operation::OperationCancellationListener;
pub use operation::{
    FileIssue, OperationCancellation, OperationCancellationReservation, OperationStatus,
    OperationSummary,
};
pub(crate) use parser::{
    clean_metadata_line, contains_inline_metadata_field, has_script_metadata_block,
    has_script_metadata_block_with_options, select_distribution_metadata_for_script,
};
pub use parser::{
    normalize_metadata_url, parse_distribution_metadata, parse_distribution_metadata_for_script,
    parse_distribution_metadata_records, parse_script_metadata, parse_script_metadata_with_options,
};
pub use text::{
    DecodedScriptText, ScriptTextEncoding, decode_script_text, decode_script_text_strict,
    decode_script_text_with_encoding, encode_script_text,
};
pub(crate) use text::{decode_script_text_prefix, normalized_newlines};
pub use version::{VersionOrdering, compare_versions, normalize_version_string};
