use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::{
    ItemId,
    core::{
        ScriptMetaKitError, ScriptMetaKitResult, ScriptRuntimeKind, clean_metadata_line,
        decode_script_text, decode_script_text_with_encoding, encode_script_text,
        has_script_metadata_block, normalize_metadata_url, normalize_version_string,
    },
    formats::{self, CommentSyntax, compiled_osa},
};

const SCRIPT_BEGIN: &str = "SCRIPTMETA-BEGIN";
const SCRIPT_END: &str = "SCRIPTMETA-END";
const DIST_BEGIN: &str = "SCRIPTMETA-DIST-BEGIN";
const DIST_END: &str = "SCRIPTMETA-DIST-END";
const DESCRIPTION_BEGIN: &str = "Description-BEGIN";
const DESCRIPTION_END: &str = "Description-END";
const BACKUP_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_SCRIPT_METADATA_PREVIEW_BYTES: usize = 8 * 1024;
pub const MAX_SCRIPT_METADATA_PREVIEW_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptIdUniquenessReport {
    pub total_items: usize,
    pub unique_script_ids: usize,
    pub duplicates: Vec<ScriptIdDuplicate>,
}

impl ScriptIdUniquenessReport {
    #[must_use]
    pub fn is_unique(&self) -> bool {
        self.duplicates.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptIdDuplicate {
    pub script_id: String,
    pub item_ids: Vec<ItemId>,
    pub file_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptIdUniquenessItem {
    pub item_id: ItemId,
    pub file_path: PathBuf,
    pub script_id: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetadataDraft {
    pub script_id: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub target_app: Option<String>,
    pub min_target_version: Option<String>,
    pub meta_url: Option<Url>,
    pub name: Option<String>,
    pub author: Option<String>,
    pub release_date: Option<String>,
    pub edit_password_sha256: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributionMetadataDraft {
    pub script_id: String,
    pub version: Option<String>,
    pub latest_url: Option<Url>,
    pub latest_page_url: Option<Url>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptMetaCommentStyle {
    JavaScriptBlock,
    AppleScriptBlock,
    PlainText,
}

impl ScriptMetaCommentStyle {
    #[must_use]
    pub fn for_path(path: &Path) -> Option<Self> {
        scriptmeta_comment_style_from_syntax(formats::comment_syntax_for_path(path)?)
    }

    #[must_use]
    fn for_path_and_source(path: &Path, source: &str) -> Option<Self> {
        if compiled_osa::is_compiled_osa_path(path) {
            if first_scriptmeta_tagged_block(source, Self::JavaScriptBlock).is_some() {
                return Some(Self::JavaScriptBlock);
            }
            if first_scriptmeta_tagged_block(source, Self::AppleScriptBlock).is_some() {
                return Some(Self::AppleScriptBlock);
            }
            return match compiled_osa::language_hint_from_source(source) {
                Some(ScriptRuntimeKind::JavaScriptForAutomation) => Some(Self::JavaScriptBlock),
                _ => Some(Self::AppleScriptBlock),
            };
        }
        Self::for_path(path)
    }

    #[must_use]
    pub const fn opening_delimiter(self) -> &'static str {
        match self {
            Self::JavaScriptBlock => "/*",
            Self::AppleScriptBlock => "(*",
            Self::PlainText => "",
        }
    }

    #[must_use]
    pub const fn closing_delimiter(self) -> &'static str {
        match self {
            Self::JavaScriptBlock => "*/",
            Self::AppleScriptBlock => "*)",
            Self::PlainText => "",
        }
    }

    #[must_use]
    pub const fn escaped_closing_delimiter(self) -> &'static str {
        match self {
            Self::JavaScriptBlock => "* /",
            Self::AppleScriptBlock => "* )",
            Self::PlainText => "",
        }
    }

    #[must_use]
    pub const fn skippable_header_line_prefixes(self) -> &'static [&'static str] {
        match self {
            Self::JavaScriptBlock => &["#", "//"],
            Self::AppleScriptBlock => &["#"],
            Self::PlainText => &["#"],
        }
    }
}

fn scriptmeta_comment_style_from_syntax(syntax: CommentSyntax) -> Option<ScriptMetaCommentStyle> {
    match syntax {
        CommentSyntax::JavaScriptBlock => Some(ScriptMetaCommentStyle::JavaScriptBlock),
        CommentSyntax::AppleScriptBlock => Some(ScriptMetaCommentStyle::AppleScriptBlock),
        CommentSyntax::PlainText => Some(ScriptMetaCommentStyle::PlainText),
        CommentSyntax::None => None,
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptMetaWriteMode {
    #[default]
    InsertOrReplace,
    InsertOnly,
    ReplaceOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptMetaWriteOperation {
    Inserted,
    Replaced,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetadataTextWriteResult {
    pub text: String,
    pub operation: ScriptMetaWriteOperation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetadataFileWriteResult {
    pub file_path: PathBuf,
    pub operation: ScriptMetaWriteOperation,
    pub backup: Option<ScriptMetaBackupRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetadataEditReadResult {
    pub file_path: PathBuf,
    pub draft: ScriptMetadataDraft,
    pub comment_style: ScriptMetaCommentStyle,
    pub line_ending: String,
    pub has_existing_block: bool,
    pub existing_lines: Vec<String>,
    pub unknown_lines: Vec<String>,
    pub existing_block_text: Option<String>,
    pub source_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetadataEditPreviewResult {
    pub file_path: PathBuf,
    pub preview_text: String,
    pub preview_byte_count: usize,
    pub file_size: Option<u64>,
    pub comment_style: Option<ScriptMetaCommentStyle>,
    pub line_ending: String,
    pub has_scriptmeta_marker_in_preview: bool,
    pub is_truncated: bool,
    pub requires_full_read: bool,
    pub file_state_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetaBackupOptions {
    pub root_directory: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptMetaBackupReason {
    BeforeSave,
    BeforeRestore,
    ResetInitial,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetaBackupRecord {
    pub id: String,
    pub created_at_millis: u64,
    pub backup_file_name: String,
    pub backup_file_path: PathBuf,
    pub file_size: u64,
    pub reason: ScriptMetaBackupReason,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct ScriptMetaBackupIndexRecord {
    id: String,
    created_at_millis: u64,
    backup_file_name: String,
    file_size: u64,
    reason: ScriptMetaBackupReason,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptMetaBackupGeneration {
    pub id: String,
    pub sequence_number: usize,
    pub created_at_millis: u64,
    pub file_path: PathBuf,
    pub file_size: u64,
    pub reason: ScriptMetaBackupReason,
    pub is_current_file: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct ScriptMetaBackupIndex {
    schema_version: u32,
    file_id: String,
    original_path: PathBuf,
    file_name: String,
    first_generation_id: Option<String>,
    generations: Vec<ScriptMetaBackupIndexRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct LegacyScriptaBackupIndex {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(rename = "fileID")]
    _file_id: String,
    #[serde(rename = "originalPath")]
    _original_path: String,
    #[serde(rename = "fileName")]
    _file_name: String,
    #[serde(rename = "firstGenerationID")]
    first_generation_id: Option<String>,
    generations: Vec<LegacyScriptaBackupRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct LegacyScriptaBackupRecord {
    id: String,
    #[serde(rename = "createdAt")]
    _created_at: String,
    #[serde(rename = "backupFileName")]
    backup_file_name: String,
    #[serde(rename = "fileSize")]
    file_size: u64,
    #[serde(rename = "sha256")]
    _sha256: String,
    reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ScriptMetaTaggedBlock {
    content: String,
    tagged_range: std::ops::Range<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ScriptMetaBlockElement {
    Raw(String),
    Key { key: String, value: String },
    Description(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScriptMetaInsertionPoint {
    index: usize,
    needs_leading_blank_line: bool,
}

#[must_use]
pub fn validate_script_id_uniqueness(items: &[ScriptIdUniquenessItem]) -> ScriptIdUniquenessReport {
    let mut groups: BTreeMap<&str, Vec<&ScriptIdUniquenessItem>> = BTreeMap::new();
    for item in items {
        if item.script_id.is_empty() {
            continue;
        }
        groups
            .entry(item.script_id.as_str())
            .or_default()
            .push(item);
    }

    let duplicates = groups
        .iter()
        .filter(|(_, entries)| entries.len() > 1)
        .map(|(script_id, entries)| ScriptIdDuplicate {
            script_id: (*script_id).to_string(),
            item_ids: entries.iter().map(|item| item.item_id.clone()).collect(),
            file_paths: entries.iter().map(|item| item.file_path.clone()).collect(),
        })
        .collect();

    ScriptIdUniquenessReport {
        total_items: items.len(),
        unique_script_ids: groups.len(),
        duplicates,
    }
}

pub fn render_script_metadata_block(draft: &ScriptMetadataDraft) -> ScriptMetaKitResult<String> {
    render_script_metadata_block_with_existing(draft, None, "\n")
}

pub fn render_script_metadata_for_style(
    draft: &ScriptMetadataDraft,
    style: ScriptMetaCommentStyle,
) -> ScriptMetaKitResult<String> {
    let block = render_script_metadata_block(draft)?;
    Ok(wrap_metadata_block(&block, style))
}

pub fn append_script_metadata_to_text(
    source: &str,
    path: &Path,
    draft: &ScriptMetadataDraft,
) -> ScriptMetaKitResult<String> {
    Ok(
        write_script_metadata_to_text(source, path, draft, ScriptMetaWriteMode::InsertOrReplace)?
            .text,
    )
}

pub fn write_script_metadata_to_text(
    source: &str,
    path: &Path,
    draft: &ScriptMetadataDraft,
    mode: ScriptMetaWriteMode,
) -> ScriptMetaKitResult<ScriptMetadataTextWriteResult> {
    let style = ScriptMetaCommentStyle::for_path_and_source(path, source).ok_or_else(|| {
        ScriptMetaKitError::InvalidConfig(format!(
            "{} does not support inline SCRIPTMETA editing",
            path.display()
        ))
    })?;
    let line_ending = detected_line_ending(source);
    let existing_block = first_scriptmeta_tagged_block(source, style);

    if existing_block.is_some() && mode == ScriptMetaWriteMode::InsertOnly {
        return Err(ScriptMetaKitError::InvalidConfig(
            "SCRIPTMETA block already exists".to_string(),
        ));
    }
    if existing_block.is_none() && mode == ScriptMetaWriteMode::ReplaceOnly {
        return Err(ScriptMetaKitError::InvalidConfig(
            "SCRIPTMETA block was not found".to_string(),
        ));
    }

    let rendered_block = render_script_metadata_block_with_existing(
        draft,
        existing_block.as_ref().map(|block| block.content.as_str()),
        line_ending,
    )?;
    let rendered_block = comment_safe_metadata_text(&rendered_block, style);

    if let Some(block) = existing_block {
        let mut output = String::with_capacity(source.len() + rendered_block.len());
        output.push_str(&source[..block.tagged_range.start]);
        output.push_str(&rendered_block);
        output.push_str(&source[block.tagged_range.end..]);
        return Ok(ScriptMetadataTextWriteResult {
            text: output,
            operation: ScriptMetaWriteOperation::Replaced,
        });
    }

    Ok(ScriptMetadataTextWriteResult {
        text: text_by_inserting_new_block(source, &rendered_block, line_ending, style),
        operation: ScriptMetaWriteOperation::Inserted,
    })
}

pub fn read_script_metadata_draft_from_text(
    source: &str,
    path: &Path,
) -> ScriptMetaKitResult<ScriptMetadataEditReadResult> {
    let style = ScriptMetaCommentStyle::for_path_and_source(path, source).ok_or_else(|| {
        ScriptMetaKitError::InvalidConfig(format!(
            "{} does not support inline SCRIPTMETA editing",
            path.display()
        ))
    })?;
    let line_ending = detected_line_ending(source).to_string();
    let existing_block = first_scriptmeta_tagged_block(source, style);
    let (draft, existing_lines, unknown_lines, existing_block_text) =
        if let Some(block) = existing_block {
            let elements = parse_block_elements(&block.content);
            (
                draft_from_block_elements(&elements),
                normalized_newlines(&block.content)
                    .split('\n')
                    .map(str::to_string)
                    .collect(),
                unknown_content_lines(&elements),
                Some(block.content),
            )
        } else {
            (ScriptMetadataDraft::default(), Vec::new(), Vec::new(), None)
        };

    Ok(ScriptMetadataEditReadResult {
        file_path: path.to_path_buf(),
        draft,
        comment_style: style,
        line_ending,
        has_existing_block: existing_block_text.is_some(),
        existing_lines,
        unknown_lines,
        existing_block_text,
        source_fingerprint: file_content_fingerprint(source.as_bytes()),
    })
}

pub fn read_script_metadata_edit_preview_from_file(
    path: &Path,
    max_bytes: usize,
) -> ScriptMetaKitResult<ScriptMetadataEditPreviewResult> {
    let max_bytes = if max_bytes == 0 {
        DEFAULT_SCRIPT_METADATA_PREVIEW_BYTES
    } else {
        max_bytes
    };
    if max_bytes > MAX_SCRIPT_METADATA_PREVIEW_BYTES {
        return Err(ScriptMetaKitError::InvalidConfig(format!(
            "metadata preview byte limit must not exceed {MAX_SCRIPT_METADATA_PREVIEW_BYTES}"
        )));
    }
    let metadata = fs::metadata(path).map_err(|error| ScriptMetaKitError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let file_size = metadata.len();
    let file_state_fingerprint = file_state_fingerprint(&metadata);

    if compiled_osa::is_compiled_osa_path(path) {
        let source = match compiled_osa::decompile_compiled_osa_source(path, None) {
            Ok(source) => source.source,
            Err(error) if compiled_osa_error_allows_text_fallback(error.kind) => {
                let bytes = read_script_bytes_file(path)?;
                decode_script_text_with_encoding(&bytes)
                    .ok_or_else(|| ScriptMetaKitError::Io {
                        path: path.to_path_buf(),
                        message: "script text encoding is not supported".to_string(),
                    })?
                    .text
            }
            Err(error) => {
                return Err(ScriptMetaKitError::Io {
                    path: path.to_path_buf(),
                    message: error.message,
                });
            }
        };
        let preview_text = utf8_prefix_at_most(&source, max_bytes).to_string();
        let preview_byte_count = preview_text.len();
        let is_truncated = source.len() > preview_byte_count;
        return Ok(script_metadata_edit_preview_result(
            path,
            preview_text,
            preview_byte_count,
            file_size,
            file_state_fingerprint,
            is_truncated,
        ));
    }

    let file = File::open(path).map_err(|error| ScriptMetaKitError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut reader = file.take(u64::try_from(max_bytes).unwrap_or(u64::MAX));
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;

    let preview_text = decode_script_text(&bytes);
    let preview_byte_count = bytes.len();
    let is_truncated = file_size > u64::try_from(preview_byte_count).unwrap_or(u64::MAX);
    Ok(script_metadata_edit_preview_result(
        path,
        preview_text,
        preview_byte_count,
        file_size,
        file_state_fingerprint,
        is_truncated,
    ))
}

fn script_metadata_edit_preview_result(
    path: &Path,
    preview_text: String,
    preview_byte_count: usize,
    file_size: u64,
    file_state_fingerprint: String,
    is_truncated: bool,
) -> ScriptMetadataEditPreviewResult {
    let comment_style = ScriptMetaCommentStyle::for_path_and_source(path, &preview_text)
        .or_else(|| ScriptMetaCommentStyle::for_path(path));
    let has_scriptmeta_marker_in_preview = has_script_metadata_block(&preview_text);
    let line_ending = detected_line_ending(&preview_text).to_string();
    ScriptMetadataEditPreviewResult {
        file_path: path.to_path_buf(),
        preview_text,
        preview_byte_count,
        file_size: Some(file_size),
        comment_style,
        line_ending,
        has_scriptmeta_marker_in_preview,
        is_truncated,
        requires_full_read: is_truncated,
        file_state_fingerprint,
    }
}

fn utf8_prefix_at_most(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }

    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

pub fn read_script_metadata_draft_from_file(
    path: &Path,
) -> ScriptMetaKitResult<ScriptMetadataEditReadResult> {
    if compiled_osa::is_compiled_osa_path(path) {
        let source = match compiled_osa::decompile_compiled_osa_source(path, None) {
            Ok(source) => source.source,
            Err(error) if compiled_osa_error_allows_text_fallback(error.kind) => {
                let bytes = read_script_bytes_file(path)?;
                decode_script_text_with_encoding(&bytes)
                    .ok_or_else(|| ScriptMetaKitError::Io {
                        path: path.to_path_buf(),
                        message: "script text encoding is not supported".to_string(),
                    })?
                    .text
            }
            Err(error) => {
                return Err(ScriptMetaKitError::Io {
                    path: path.to_path_buf(),
                    message: error.message,
                });
            }
        };
        return read_script_metadata_draft_from_text(&source, path);
    }

    let bytes = read_script_bytes_file(path)?;
    let decoded =
        decode_script_text_with_encoding(&bytes).ok_or_else(|| ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: "script text encoding is not supported".to_string(),
        })?;
    let mut result = read_script_metadata_draft_from_text(&decoded.text, path)?;
    result.source_fingerprint = file_content_fingerprint(&bytes);
    Ok(result)
}

pub fn append_script_metadata_to_file(
    path: &Path,
    draft: &ScriptMetadataDraft,
) -> ScriptMetaKitResult<()> {
    write_script_metadata_to_file(path, draft, ScriptMetaWriteMode::InsertOrReplace, None)?;
    Ok(())
}

pub fn write_script_metadata_to_file(
    path: &Path,
    draft: &ScriptMetadataDraft,
    mode: ScriptMetaWriteMode,
    backup_options: Option<&ScriptMetaBackupOptions>,
) -> ScriptMetaKitResult<ScriptMetadataFileWriteResult> {
    write_script_metadata_to_file_if_unchanged(path, draft, mode, backup_options, None)
}

pub fn write_script_metadata_to_file_if_unchanged(
    path: &Path,
    draft: &ScriptMetadataDraft,
    mode: ScriptMetaWriteMode,
    backup_options: Option<&ScriptMetaBackupOptions>,
    expected_source_fingerprint: Option<&str>,
) -> ScriptMetaKitResult<ScriptMetadataFileWriteResult> {
    ensure_script_file_writable(path)?;
    if compiled_osa::is_compiled_osa_path(path) {
        return write_script_metadata_to_compiled_osa_file(
            path,
            draft,
            mode,
            backup_options,
            expected_source_fingerprint,
        );
    }

    let bytes = read_script_bytes_file(path)?;
    let initial_fingerprint = file_content_fingerprint(&bytes);
    ensure_expected_source_fingerprint(path, expected_source_fingerprint, &initial_fingerprint)?;
    let decoded =
        decode_script_text_with_encoding(&bytes).ok_or_else(|| ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: "script text encoding is not supported".to_string(),
        })?;
    let updated = write_script_metadata_to_text(&decoded.text, path, draft, mode)?;
    let encoded = encode_script_text(&updated.text, decoded.encoding).ok_or_else(|| {
        ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: "updated script text cannot be encoded with the original encoding".to_string(),
        }
    })?;
    ensure_file_fingerprint(path, &initial_fingerprint)?;
    let backup = match backup_options {
        Some(options) => Some(create_scriptmeta_backup(
            path,
            options,
            ScriptMetaBackupReason::BeforeSave,
        )?),
        None => None,
    };

    if let Err(error) = ensure_file_fingerprint(path, &initial_fingerprint) {
        if let (Some(options), Some(record)) = (backup_options, backup.as_ref()) {
            remove_scriptmeta_backup_if_possible(path, options, record);
        }
        return Err(error);
    }

    if let Err(error) = write_bytes_file_atomic(path, &encoded) {
        if !error.is_committed()
            && let (Some(options), Some(record)) = (backup_options, backup.as_ref())
        {
            remove_scriptmeta_backup_if_possible(path, options, record);
        }
        return Err(ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        });
    }

    Ok(ScriptMetadataFileWriteResult {
        file_path: path.to_path_buf(),
        operation: updated.operation,
        backup,
    })
}

fn write_script_metadata_to_compiled_osa_file(
    path: &Path,
    draft: &ScriptMetadataDraft,
    mode: ScriptMetaWriteMode,
    backup_options: Option<&ScriptMetaBackupOptions>,
    expected_source_fingerprint: Option<&str>,
) -> ScriptMetaKitResult<ScriptMetadataFileWriteResult> {
    let initial_file_fingerprint = file_content_fingerprint(&read_script_bytes_file(path)?);
    let source = match compiled_osa::decompile_compiled_osa_source(path, None) {
        Ok(source) => source,
        Err(error) if compiled_osa_error_allows_text_fallback(error.kind) => {
            return write_script_metadata_to_text_file(
                path,
                draft,
                mode,
                backup_options,
                expected_source_fingerprint,
            );
        }
        Err(error) => {
            return Err(ScriptMetaKitError::Io {
                path: path.to_path_buf(),
                message: error.message,
            });
        }
    };
    ensure_expected_source_fingerprint(
        path,
        expected_source_fingerprint,
        &file_content_fingerprint(source.source.as_bytes()),
    )?;
    let updated = write_script_metadata_to_text(&source.source, path, draft, mode)?;
    let temp_path = compiled_osa_temp_path(path);
    let mut temp_guard = TemporaryEditFile::new(temp_path.clone());
    let language_hint = source
        .language_hint
        .or_else(|| compiled_osa::language_hint_from_source(&updated.text));
    if let Err(error) =
        compiled_osa::compile_osa_source_to_path(&updated.text, language_hint, &temp_path, None)
    {
        let _ = fs::remove_file(&temp_path);
        return Err(ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: error.message,
        });
    }

    let result = write_compiled_osa_temp_result(
        path,
        &temp_path,
        updated.operation,
        backup_options,
        &initial_file_fingerprint,
    );
    if result.is_ok() {
        temp_guard.disarm();
    }
    result
}

fn write_compiled_osa_temp_result(
    path: &Path,
    temp_path: &Path,
    operation: ScriptMetaWriteOperation,
    backup_options: Option<&ScriptMetaBackupOptions>,
    initial_file_fingerprint: &str,
) -> ScriptMetaKitResult<ScriptMetadataFileWriteResult> {
    if let Err(error) = copy_replacement_metadata(path, temp_path) {
        let _ = fs::remove_file(temp_path);
        return Err(ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        });
    }
    ensure_file_fingerprint(path, initial_file_fingerprint)?;
    let backup = match backup_options {
        Some(options) => Some(create_scriptmeta_backup(
            path,
            options,
            ScriptMetaBackupReason::BeforeSave,
        )?),
        None => None,
    };

    if let Err(error) = ensure_file_fingerprint(path, initial_file_fingerprint) {
        let _ = fs::remove_file(temp_path);
        if let (Some(options), Some(record)) = (backup_options, backup.as_ref()) {
            remove_scriptmeta_backup_if_possible(path, options, record);
        }
        return Err(error);
    }

    if let Err(error) = replace_file(temp_path, path) {
        let _ = fs::remove_file(temp_path);
        if !error.is_committed()
            && let (Some(options), Some(record)) = (backup_options, backup.as_ref())
        {
            remove_scriptmeta_backup_if_possible(path, options, record);
        }
        return Err(ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        });
    }

    Ok(ScriptMetadataFileWriteResult {
        file_path: path.to_path_buf(),
        operation,
        backup,
    })
}

fn write_script_metadata_to_text_file(
    path: &Path,
    draft: &ScriptMetadataDraft,
    mode: ScriptMetaWriteMode,
    backup_options: Option<&ScriptMetaBackupOptions>,
    expected_source_fingerprint: Option<&str>,
) -> ScriptMetaKitResult<ScriptMetadataFileWriteResult> {
    let bytes = read_script_bytes_file(path)?;
    let initial_fingerprint = file_content_fingerprint(&bytes);
    ensure_expected_source_fingerprint(path, expected_source_fingerprint, &initial_fingerprint)?;
    let decoded =
        decode_script_text_with_encoding(&bytes).ok_or_else(|| ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: "script text encoding is not supported".to_string(),
        })?;
    let updated = write_script_metadata_to_text(&decoded.text, path, draft, mode)?;
    let encoded = encode_script_text(&updated.text, decoded.encoding).ok_or_else(|| {
        ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: "updated script text cannot be encoded with the original encoding".to_string(),
        }
    })?;
    ensure_file_fingerprint(path, &initial_fingerprint)?;
    let backup = match backup_options {
        Some(options) => Some(create_scriptmeta_backup(
            path,
            options,
            ScriptMetaBackupReason::BeforeSave,
        )?),
        None => None,
    };

    if let Err(error) = ensure_file_fingerprint(path, &initial_fingerprint) {
        if let (Some(options), Some(record)) = (backup_options, backup.as_ref()) {
            remove_scriptmeta_backup_if_possible(path, options, record);
        }
        return Err(error);
    }

    if let Err(error) = write_bytes_file_atomic(path, &encoded) {
        if !error.is_committed()
            && let (Some(options), Some(record)) = (backup_options, backup.as_ref())
        {
            remove_scriptmeta_backup_if_possible(path, options, record);
        }
        return Err(ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        });
    }

    Ok(ScriptMetadataFileWriteResult {
        file_path: path.to_path_buf(),
        operation: updated.operation,
        backup,
    })
}

fn compiled_osa_error_allows_text_fallback(kind: compiled_osa::CompiledOsaErrorKind) -> bool {
    matches!(kind, compiled_osa::CompiledOsaErrorKind::NotOsaScript)
}

pub fn render_distribution_metadata_block(
    records: &[DistributionMetadataDraft],
) -> ScriptMetaKitResult<String> {
    if records.is_empty() {
        return Err(ScriptMetaKitError::InvalidConfig(
            "at least one SCRIPTMETA-DIST record is required".to_string(),
        ));
    }

    for record in records {
        validate_distribution_metadata_draft(record)?;
    }

    let mut output = String::with_capacity(records.len() * 120);
    output.push_str(DIST_BEGIN);
    output.push('\n');
    for (index, record) in records.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        push_field(&mut output, "Script-ID", Some(record.script_id.as_str()));
        push_normalized_version_field(&mut output, "Version", record.version.as_deref());
        push_field(
            &mut output,
            "Latest-URL",
            record.latest_url.as_ref().map(Url::as_str),
        );
        push_field(
            &mut output,
            "Latest-Page-URL",
            record.latest_page_url.as_ref().map(Url::as_str),
        );
    }
    output.push_str(DIST_END);
    output.push('\n');
    Ok(output)
}

pub fn generate_edit_password_sha256(password: &str) -> ScriptMetaKitResult<String> {
    let password = password.trim();
    if password.is_empty() {
        return Err(ScriptMetaKitError::InvalidConfig(
            "edit password is required".to_string(),
        ));
    }
    let salt = random_edit_password_salt();
    Ok(format!(
        "{salt}:{}",
        edit_password_digest_sha256(salt.as_str(), password)
    ))
}

#[must_use]
pub fn verify_edit_password_sha256(password: &str, stored_value: &str) -> bool {
    let Some((salt, expected_digest)) = parsed_edit_password_sha256(stored_value) else {
        return false;
    };
    edit_password_digest_sha256(salt, password) == expected_digest
}

#[must_use]
pub fn is_valid_edit_password_sha256(value: &str) -> bool {
    parsed_edit_password_sha256(value).is_some()
}

pub fn create_scriptmeta_backup(
    path: &Path,
    options: &ScriptMetaBackupOptions,
    reason: ScriptMetaBackupReason,
) -> ScriptMetaKitResult<ScriptMetaBackupRecord> {
    let backup_directory = backup_generations_directory(path, options);
    let index_path = backup_index_path(path, options);
    fs::create_dir_all(&backup_directory).map_err(|error| ScriptMetaKitError::Io {
        path: backup_directory.clone(),
        message: error.to_string(),
    })?;

    let mut index = load_backup_index(path, options)?;
    let generation_id = format!("{}-{}", backup_timestamp_millis(), short_uuid());
    let backup_file_name = backup_file_name(&generation_id, path);
    let backup_path = backup_directory.join(&backup_file_name);

    let file_size = fs::copy(path, &backup_path).map_err(|error| ScriptMetaKitError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let record = ScriptMetaBackupIndexRecord {
        id: generation_id,
        created_at_millis: now_millis(),
        backup_file_name,
        file_size,
        reason,
    };

    if index.first_generation_id.is_none() {
        index.first_generation_id = Some(record.id.clone());
    }
    index.generations.push(record.clone());
    if let Err(error) = write_backup_index(&index, &index_path) {
        let _ = fs::remove_file(&backup_path);
        return Err(error);
    }

    Ok(scriptmeta_backup_record_from_index(&record, backup_path))
}

pub fn scriptmeta_backup_generations(
    path: &Path,
    options: &ScriptMetaBackupOptions,
) -> ScriptMetaKitResult<Vec<ScriptMetaBackupGeneration>> {
    let index = load_backup_index(path, options)?;
    let backup_directory = backup_generations_directory(path, options);
    let mut generations = Vec::with_capacity(index.generations.len() + 1);

    for record in &index.generations {
        let backup_path = backup_directory.join(&record.backup_file_name);
        if !backup_path.exists() {
            continue;
        }
        generations.push(ScriptMetaBackupGeneration {
            id: record.id.clone(),
            sequence_number: generations.len(),
            created_at_millis: record.created_at_millis,
            file_path: backup_path,
            file_size: record.file_size,
            reason: record.reason,
            is_current_file: false,
        });
    }

    if let Ok(file_size) = file_size(path) {
        let current_modified_at = file_modified_millis(path);
        // Listing backups should stay cheap: do not hash the current script just
        // to decide whether to show a non-restorable "current file" row.
        let should_add_current = generations.last().is_some_and(|generation| {
            generation.file_size != file_size
                || current_modified_at
                    .is_some_and(|modified_at| modified_at > generation.created_at_millis)
        });
        if should_add_current {
            generations.push(ScriptMetaBackupGeneration {
                id: format!("current-{}", backup_file_id(path)),
                sequence_number: generations.len(),
                created_at_millis: current_modified_at.unwrap_or_else(now_millis),
                file_path: path.to_path_buf(),
                file_size,
                reason: ScriptMetaBackupReason::BeforeSave,
                is_current_file: true,
            });
        }
    }

    Ok(generations)
}

pub fn restore_scriptmeta_backup(
    path: &Path,
    options: &ScriptMetaBackupOptions,
    generation_id: &str,
) -> ScriptMetaKitResult<ScriptMetaBackupRecord> {
    let generations = scriptmeta_backup_generations(path, options)?;
    let Some(generation) = generations
        .iter()
        .find(|generation| generation.id == generation_id)
    else {
        return Err(ScriptMetaKitError::InvalidConfig(format!(
            "backup generation `{generation_id}` was not found"
        )));
    };
    if generation.is_current_file {
        return Err(ScriptMetaKitError::InvalidConfig(
            "current file generation cannot be restored".to_string(),
        ));
    }

    let before_restore =
        create_scriptmeta_backup(path, options, ScriptMetaBackupReason::BeforeRestore)?;
    let bytes = fs::read(&generation.file_path).map_err(|error| ScriptMetaKitError::Io {
        path: generation.file_path.clone(),
        message: error.to_string(),
    })?;
    write_bytes_file_atomic(path, &bytes).map_err(|error| ScriptMetaKitError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    clear_scriptmeta_backups_after_preserving(
        path,
        options,
        generation_id,
        Some(&before_restore.id),
    )?;
    Ok(before_restore)
}

pub fn clear_scriptmeta_backups(
    path: &Path,
    options: &ScriptMetaBackupOptions,
) -> ScriptMetaKitResult<()> {
    let directory = backup_file_directory(path, options);
    if !directory.exists() {
        return Ok(());
    }
    fs::remove_dir_all(&directory).map_err(|error| ScriptMetaKitError::Io {
        path: directory,
        message: error.to_string(),
    })
}

pub fn reset_scriptmeta_backups_with_current_as_initial(
    path: &Path,
    options: &ScriptMetaBackupOptions,
) -> ScriptMetaKitResult<ScriptMetaBackupRecord> {
    let transaction_id = Uuid::new_v4();
    let staged_root = options
        .root_directory
        .join(format!(".reset-{transaction_id}"));
    let staged_options = ScriptMetaBackupOptions {
        root_directory: staged_root.clone(),
    };
    let mut record =
        match create_scriptmeta_backup(path, &staged_options, ScriptMetaBackupReason::ResetInitial)
        {
            Ok(record) => record,
            Err(error) => {
                let _ = fs::remove_dir_all(&staged_root);
                return Err(error);
            }
        };

    let target_directory = backup_file_directory(path, options);
    let staged_directory = backup_file_directory(path, &staged_options);
    let target_parent = target_directory.parent().unwrap_or(&options.root_directory);
    if let Err(error) = fs::create_dir_all(target_parent) {
        let _ = fs::remove_dir_all(&staged_root);
        return Err(ScriptMetaKitError::Io {
            path: target_parent.to_path_buf(),
            message: error.to_string(),
        });
    }

    let previous_directory = target_parent.join(format!(
        ".{}.reset-previous-{transaction_id}",
        target_directory
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default()
    ));
    let had_previous_directory = target_directory.exists();
    if had_previous_directory && let Err(error) = fs::rename(&target_directory, &previous_directory)
    {
        let _ = fs::remove_dir_all(&staged_root);
        return Err(ScriptMetaKitError::Io {
            path: target_directory,
            message: error.to_string(),
        });
    }

    if let Err(error) = fs::rename(&staged_directory, &target_directory) {
        let rollback_error = had_previous_directory
            .then(|| fs::rename(&previous_directory, &target_directory).err())
            .flatten();
        let _ = fs::remove_dir_all(&staged_root);
        let message = match rollback_error {
            Some(rollback_error) => format!(
                "{error}; restoring the previous backup history also failed: {rollback_error}"
            ),
            None => error.to_string(),
        };
        return Err(ScriptMetaKitError::Io {
            path: target_directory,
            message,
        });
    }

    if had_previous_directory {
        let _ = fs::remove_dir_all(previous_directory);
    }
    let _ = fs::remove_dir_all(staged_root);
    record.backup_file_path =
        backup_generations_directory(path, options).join(&record.backup_file_name);
    Ok(record)
}

fn validate_script_metadata_draft(draft: &ScriptMetadataDraft) -> ScriptMetaKitResult<()> {
    validate_required_field("Script-ID", draft.script_id.as_str())?;
    validate_optional_version("Version", draft.version.as_deref())?;
    validate_optional_version("Min-Target-Version", draft.min_target_version.as_deref())?;
    if let Some(edit_password_sha256) = draft
        .edit_password_sha256
        .as_deref()
        .filter(|value| has_value(Some(value)))
        && !is_valid_edit_password_sha256(edit_password_sha256)
    {
        return Err(ScriptMetaKitError::InvalidConfig(
            "Edit-Password-SHA256 must be <salt>:<sha256>".to_string(),
        ));
    }
    if has_value(draft.edit_password_sha256.as_deref()) && !has_value(draft.author.as_deref()) {
        return Err(ScriptMetaKitError::InvalidConfig(
            "Author is required when Edit-Password-SHA256 is present".to_string(),
        ));
    }
    Ok(())
}

fn render_script_metadata_block_with_existing(
    draft: &ScriptMetadataDraft,
    existing_content: Option<&str>,
    line_ending: &str,
) -> ScriptMetaKitResult<String> {
    validate_script_metadata_draft(draft)?;

    let existing_unknown_lines = existing_content
        .map(parse_block_elements)
        .map(|elements| unknown_content_lines(&elements))
        .unwrap_or_default();
    let mut lines = Vec::with_capacity(16 + existing_unknown_lines.len());
    append_field_line(&mut lines, "Script-ID", Some(draft.script_id.as_str()));
    append_normalized_version_field_line(&mut lines, "Version", draft.version.as_deref());
    append_field_line(
        &mut lines,
        "Meta-URL",
        draft.meta_url.as_ref().map(Url::as_str),
    );
    append_field_line(&mut lines, "Name", draft.name.as_deref());
    append_field_line(&mut lines, "Author", draft.author.as_deref());
    append_field_line(&mut lines, "Release-Date", draft.release_date.as_deref());
    append_field_line(&mut lines, "Target-App", draft.target_app.as_deref());
    append_field_line(
        &mut lines,
        "Min-Target-Version",
        normalized_version_string_ref(draft.min_target_version.as_deref()).as_deref(),
    );
    append_field_line(
        &mut lines,
        "Edit-Password-SHA256",
        draft.edit_password_sha256.as_deref(),
    );
    lines.extend(existing_unknown_lines);
    append_description_lines(&mut lines, draft.description.as_deref());
    trim_outer_blank_lines(&mut lines);

    let mut output = String::with_capacity(256);
    output.push_str(SCRIPT_BEGIN);
    output.push_str(line_ending);
    output.push_str(&lines.join(line_ending));
    if !lines.is_empty() {
        output.push_str(line_ending);
    }
    output.push_str(SCRIPT_END);
    output.push_str(line_ending);
    Ok(output)
}

fn append_field_line(lines: &mut Vec<String>, key: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    lines.push(format!("{key}={value}"));
}

fn append_normalized_version_field_line(lines: &mut Vec<String>, key: &str, value: Option<&str>) {
    let Some(value) = normalized_version_string_ref(value) else {
        return;
    };
    lines.push(format!("{key}={value}"));
}

fn append_description_lines(lines: &mut Vec<String>, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    if value.contains('\n') || value.contains('\r') {
        lines.push(DESCRIPTION_BEGIN.to_string());
        lines.extend(normalized_newlines(value).split('\n').map(str::to_string));
        lines.push(DESCRIPTION_END.to_string());
    } else {
        append_field_line(lines, "Description", Some(value));
    }
}

fn unknown_content_lines(elements: &[ScriptMetaBlockElement]) -> Vec<String> {
    let mut output = Vec::new();
    for element in elements {
        match element {
            ScriptMetaBlockElement::Raw(line) => {
                if !line.trim().is_empty() {
                    output.push(line.clone());
                }
            }
            ScriptMetaBlockElement::Key { key, value } => {
                if !is_known_script_metadata_key(key) {
                    output.push(format!("{key}={value}"));
                }
            }
            ScriptMetaBlockElement::Description(_) => {}
        }
    }
    output
}

fn parse_block_elements(content: &str) -> Vec<ScriptMetaBlockElement> {
    let normalized = normalized_newlines(content);
    let lines: Vec<_> = normalized.split('\n').collect();
    let mut elements = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = clean_metadata_line(lines[index]);
        let trimmed = line.trim();
        if trimmed == DESCRIPTION_BEGIN {
            let mut body = Vec::new();
            index += 1;
            while index < lines.len() {
                let body_line = clean_metadata_line(lines[index]);
                if body_line.trim() == DESCRIPTION_END {
                    break;
                }
                body.push(body_line);
                index += 1;
            }
            elements.push(ScriptMetaBlockElement::Description(
                body.join("\n").trim().to_string(),
            ));
            if index < lines.len() {
                index += 1;
            }
            continue;
        }

        if let Some(separator_index) = script_metadata_separator_index(line) {
            let key = line[..separator_index].trim();
            let value = line[separator_index + 1..].trim();
            elements.push(ScriptMetaBlockElement::Key {
                key: key.to_string(),
                value: value.to_string(),
            });
        } else {
            elements.push(ScriptMetaBlockElement::Raw(line.to_string()));
        }
        index += 1;
    }
    elements
}

fn script_metadata_separator_index(line: &str) -> Option<usize> {
    match (line.find(':'), line.find('=')) {
        (Some(colon), Some(equals)) => Some(colon.min(equals)),
        (Some(colon), None) => Some(colon),
        (None, Some(equals)) => Some(equals),
        (None, None) => None,
    }
}

fn draft_from_block_elements(elements: &[ScriptMetaBlockElement]) -> ScriptMetadataDraft {
    let mut draft = ScriptMetadataDraft::default();
    for element in elements {
        match element {
            ScriptMetaBlockElement::Key { key, value } => {
                set_draft_field(&mut draft, key, value);
            }
            ScriptMetaBlockElement::Description(value) => {
                if !value.trim().is_empty() {
                    draft.description = Some(value.trim().to_string());
                }
            }
            ScriptMetaBlockElement::Raw(_) => {}
        }
    }
    draft
}

fn set_draft_field(draft: &mut ScriptMetadataDraft, key: &str, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    if key.eq_ignore_ascii_case("script-id") {
        draft.script_id = value.to_string();
    } else if key.eq_ignore_ascii_case("version") {
        draft.version = normalize_version_string(value).or_else(|| Some(value.to_string()));
    } else if key.eq_ignore_ascii_case("description") {
        draft.description = Some(value.to_string());
    } else if key.eq_ignore_ascii_case("target-app") {
        draft.target_app = Some(value.to_string());
    } else if key.eq_ignore_ascii_case("min-target-version") {
        draft.min_target_version =
            normalize_version_string(value).or_else(|| Some(value.to_string()));
    } else if key.eq_ignore_ascii_case("meta-url") {
        draft.meta_url = normalize_metadata_url(value).or_else(|| Url::parse(value).ok());
    } else if key.eq_ignore_ascii_case("name") {
        draft.name = Some(value.to_string());
    } else if key.eq_ignore_ascii_case("author") {
        draft.author = Some(value.to_string());
    } else if key.eq_ignore_ascii_case("release-date") {
        draft.release_date = Some(value.to_string());
    } else if key.eq_ignore_ascii_case("edit-password-sha256") {
        draft.edit_password_sha256 = Some(value.to_string());
    }
}

fn is_known_script_metadata_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "script-id"
            | "version"
            | "description"
            | "target-app"
            | "min-target-version"
            | "meta-url"
            | "name"
            | "author"
            | "release-date"
            | "edit-password-sha256"
    )
}

fn trim_outer_blank_lines(lines: &mut Vec<String>) {
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
}

fn text_by_inserting_new_block(
    source: &str,
    tagged_block: &str,
    line_ending: &str,
    style: ScriptMetaCommentStyle,
) -> String {
    let tagged_block = tagged_block.trim_end_matches(['\r', '\n']);
    if let Some(comment_end) = leading_block_comment_end_index(source, style) {
        let mut insertion = String::new();
        if source[..comment_end]
            .chars()
            .next_back()
            .is_some_and(|ch| ch != '\n' && ch != '\r')
        {
            insertion.push_str(line_ending);
        }
        insertion.push_str(line_ending);
        insertion.push_str(tagged_block);
        insertion.push_str(line_ending);
        insertion.push_str(line_ending);

        let mut output = String::with_capacity(source.len() + insertion.len());
        output.push_str(&source[..comment_end]);
        output.push_str(&insertion);
        output.push_str(&source[comment_end..]);
        return output;
    }

    let insertion_point = insertion_point_after_leading_target_directives(source);
    let insertion = wrapped_tagged_block_insertion(
        tagged_block,
        line_ending,
        insertion_point.needs_leading_blank_line,
        style,
    );
    let mut output = String::with_capacity(source.len() + insertion.len());
    output.push_str(&source[..insertion_point.index]);
    output.push_str(&insertion);
    output.push_str(&source[insertion_point.index..]);
    output
}

fn first_scriptmeta_tagged_block(
    text: &str,
    style: ScriptMetaCommentStyle,
) -> Option<ScriptMetaTaggedBlock> {
    let mut search_start = 0usize;
    while search_start < text.len() {
        let begin_start =
            search_start + find_ascii_case_insensitive(&text[search_start..], SCRIPT_BEGIN)?;
        let begin_end = begin_start + SCRIPT_BEGIN.len();
        let end_start = begin_end + find_ascii_case_insensitive(&text[begin_end..], SCRIPT_END)?;
        let end_end = end_start + SCRIPT_END.len();
        if is_tagged_range_inside_block_comment(text, begin_start, end_end, style)
            || is_standalone_tagged_range(text, begin_start, end_end)
        {
            return Some(ScriptMetaTaggedBlock {
                content: text[begin_end..end_start].to_string(),
                tagged_range: begin_start..end_end,
            });
        }
        search_start = begin_end;
    }
    None
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn is_tagged_range_inside_block_comment(
    text: &str,
    range_start: usize,
    range_end: usize,
    style: ScriptMetaCommentStyle,
) -> bool {
    if style == ScriptMetaCommentStyle::PlainText {
        return false;
    }
    let opening = style.opening_delimiter();
    let closing = style.closing_delimiter();
    let Some(comment_start) = text[..range_start].rfind(opening) else {
        return false;
    };
    if !is_only_whitespace_since_line_start(text, comment_start) {
        return false;
    }
    let after_comment_start = comment_start + opening.len();
    let Some(comment_end_start) = text[after_comment_start..].find(closing) else {
        return false;
    };
    after_comment_start + comment_end_start >= range_end
}

fn is_standalone_tagged_range(text: &str, range_start: usize, range_end: usize) -> bool {
    is_only_whitespace_since_line_start(text, range_start)
        && is_only_whitespace_until_line_end(text, range_end)
}

fn leading_block_comment_end_index(text: &str, style: ScriptMetaCommentStyle) -> Option<usize> {
    if style == ScriptMetaCommentStyle::PlainText {
        return None;
    }
    let mut index = 0usize;
    let mut scanned_lines = 0usize;
    while index < text.len() && scanned_lines < 40 {
        let line = source_line(text, index);
        let trimmed = text[line.content_range.clone()].trim();
        if trimmed.is_empty()
            || style
                .skippable_header_line_prefixes()
                .iter()
                .any(|prefix| trimmed.starts_with(prefix))
        {
            index = line.next_index;
            scanned_lines += 1;
            continue;
        }
        if trimmed.starts_with(style.opening_delimiter()) {
            let comment_start = index + text[index..].find(style.opening_delimiter())?;
            let after_start = comment_start + style.opening_delimiter().len();
            return text[after_start..]
                .find(style.closing_delimiter())
                .map(|offset| after_start + offset);
        }
        break;
    }
    None
}

fn insertion_point_after_leading_target_directives(text: &str) -> ScriptMetaInsertionPoint {
    let mut index = 0usize;
    let mut insertion_index = 0usize;
    let mut found_directive = false;
    let mut found_blank_line_after_directive = false;
    while index < text.len() {
        let line = source_line(text, index);
        let trimmed = text[line.content_range.clone()].trim();
        if trimmed.is_empty() {
            if found_directive {
                insertion_index = line.next_index;
                found_blank_line_after_directive = true;
            }
            index = line.next_index;
            continue;
        }
        if trimmed.starts_with('#') {
            found_directive = true;
            found_blank_line_after_directive = false;
            insertion_index = line.next_index;
            index = line.next_index;
            continue;
        }
        break;
    }
    ScriptMetaInsertionPoint {
        index: if found_directive { insertion_index } else { 0 },
        needs_leading_blank_line: found_directive && !found_blank_line_after_directive,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceLine {
    content_range: std::ops::Range<usize>,
    next_index: usize,
}

fn source_line(text: &str, index: usize) -> SourceLine {
    let bytes = text.as_bytes();
    let mut current = index;
    while current < bytes.len() {
        match bytes[current] {
            b'\n' => {
                return SourceLine {
                    content_range: index..current,
                    next_index: current + 1,
                };
            }
            b'\r' => {
                let next_index = if bytes.get(current + 1) == Some(&b'\n') {
                    current + 2
                } else {
                    current + 1
                };
                return SourceLine {
                    content_range: index..current,
                    next_index,
                };
            }
            _ => current += 1,
        }
    }
    SourceLine {
        content_range: index..text.len(),
        next_index: text.len(),
    }
}

fn is_only_whitespace_since_line_start(text: &str, index: usize) -> bool {
    text[..index]
        .rsplit(['\n', '\r'])
        .next()
        .is_none_or(|prefix| prefix.trim().is_empty())
}

fn is_only_whitespace_until_line_end(text: &str, index: usize) -> bool {
    text[index..]
        .split(['\n', '\r'])
        .next()
        .is_none_or(|suffix| suffix.trim().is_empty())
}

fn wrapped_tagged_block_insertion(
    tagged_block: &str,
    line_ending: &str,
    needs_leading_blank_line: bool,
    style: ScriptMetaCommentStyle,
) -> String {
    let insertion = match style {
        ScriptMetaCommentStyle::JavaScriptBlock | ScriptMetaCommentStyle::AppleScriptBlock => {
            [
                style.opening_delimiter(),
                "",
                tagged_block.trim_end_matches(['\r', '\n']),
                "",
                style.closing_delimiter(),
            ]
            .join(line_ending)
                + line_ending
                + line_ending
        }
        ScriptMetaCommentStyle::PlainText => {
            tagged_block.trim_end_matches(['\r', '\n']).to_string() + line_ending + line_ending
        }
    };
    if needs_leading_blank_line {
        line_ending.to_string() + &insertion
    } else {
        insertion
    }
}

fn comment_safe_metadata_text(value: &str, style: ScriptMetaCommentStyle) -> String {
    let closing = style.closing_delimiter();
    if closing.is_empty() {
        return value.to_string();
    }
    value.replace(closing, style.escaped_closing_delimiter())
}

fn detected_line_ending(text: &str) -> &str {
    if text.contains("\r\n") {
        "\r\n"
    } else if text.contains('\r') {
        "\r"
    } else {
        "\n"
    }
}

fn normalized_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn load_backup_index(
    path: &Path,
    options: &ScriptMetaBackupOptions,
) -> ScriptMetaKitResult<ScriptMetaBackupIndex> {
    let index_path = backup_index_path(path, options);
    let file_id = backup_file_id(path);
    let original_path = path.to_path_buf();
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "script".to_string());

    if !index_path.exists() {
        return Ok(ScriptMetaBackupIndex {
            schema_version: BACKUP_SCHEMA_VERSION,
            file_id,
            original_path,
            file_name,
            first_generation_id: None,
            generations: Vec::new(),
        });
    }

    let data = fs::read(&index_path).map_err(|error| ScriptMetaKitError::Io {
        path: index_path.clone(),
        message: error.to_string(),
    })?;
    match serde_json::from_slice::<ScriptMetaBackupIndex>(&data) {
        Ok(mut index) => {
            if index.schema_version != BACKUP_SCHEMA_VERSION {
                return Err(ScriptMetaKitError::Cache(format!(
                    "unsupported backup schema {}",
                    index.schema_version
                )));
            }
            validate_backup_file_names(&index.generations)?;
            index.file_id = file_id;
            index.original_path = original_path;
            index.file_name = file_name;
            Ok(index)
        }
        Err(index_error) => {
            if let Ok(legacy_index) = serde_json::from_slice::<LegacyScriptaBackupIndex>(&data) {
                return legacy_scripta_backup_index(
                    legacy_index,
                    path,
                    options,
                    file_id,
                    original_path,
                    file_name,
                );
            }
            Ok(fallback_backup_index_from_generations(
                path,
                options,
                file_id,
                original_path,
                file_name,
                Some(index_error.to_string()),
            ))
        }
    }
}

fn write_backup_index(index: &ScriptMetaBackupIndex, index_path: &Path) -> ScriptMetaKitResult<()> {
    let data = serde_json::to_vec_pretty(index)
        .map_err(|error| ScriptMetaKitError::Cache(error.to_string()))?;
    write_bytes_file_atomic(index_path, &data).map_err(|error| ScriptMetaKitError::Io {
        path: index_path.to_path_buf(),
        message: error.to_string(),
    })
}

fn legacy_scripta_backup_index(
    legacy_index: LegacyScriptaBackupIndex,
    path: &Path,
    options: &ScriptMetaBackupOptions,
    file_id: String,
    original_path: PathBuf,
    file_name: String,
) -> ScriptMetaKitResult<ScriptMetaBackupIndex> {
    if legacy_index.schema_version != BACKUP_SCHEMA_VERSION {
        return Err(ScriptMetaKitError::Cache(format!(
            "unsupported legacy backup schema {}",
            legacy_index.schema_version
        )));
    }
    for record in &legacy_index.generations {
        validate_backup_file_name(&record.backup_file_name)?;
    }

    let backup_directory = backup_generations_directory(path, options);
    let generations = legacy_index
        .generations
        .into_iter()
        .filter_map(|record| {
            let backup_path = backup_directory.join(&record.backup_file_name);
            backup_path.exists().then(|| ScriptMetaBackupIndexRecord {
                id: record.id,
                created_at_millis: file_modified_millis(&backup_path).unwrap_or(0),
                backup_file_name: record.backup_file_name,
                file_size: record.file_size,
                reason: legacy_scripta_backup_reason(&record.reason),
            })
        })
        .collect::<Vec<_>>();

    let first_generation_id = legacy_index
        .first_generation_id
        .filter(|id| generations.iter().any(|record| &record.id == id))
        .or_else(|| generations.first().map(|record| record.id.clone()));

    Ok(ScriptMetaBackupIndex {
        schema_version: BACKUP_SCHEMA_VERSION,
        file_id,
        original_path,
        file_name,
        first_generation_id,
        generations,
    })
}

fn fallback_backup_index_from_generations(
    path: &Path,
    options: &ScriptMetaBackupOptions,
    file_id: String,
    original_path: PathBuf,
    file_name: String,
    _decode_error: Option<String>,
) -> ScriptMetaBackupIndex {
    let backup_directory = backup_generations_directory(path, options);
    let mut generation_paths = fs::read_dir(&backup_directory)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    generation_paths.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

    let generations = generation_paths
        .into_iter()
        .filter_map(|generation_path| {
            let metadata = fs::metadata(&generation_path).ok()?;
            let backup_file_name = generation_path.file_name()?.to_string_lossy().into_owned();
            Some(ScriptMetaBackupIndexRecord {
                id: generation_path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_else(|| backup_file_name.clone()),
                created_at_millis: file_modified_millis(&generation_path).unwrap_or(0),
                backup_file_name,
                file_size: metadata.len(),
                reason: ScriptMetaBackupReason::BeforeSave,
            })
        })
        .collect::<Vec<_>>();

    ScriptMetaBackupIndex {
        schema_version: BACKUP_SCHEMA_VERSION,
        file_id,
        original_path,
        file_name,
        first_generation_id: generations.first().map(|record| record.id.clone()),
        generations,
    }
}

fn legacy_scripta_backup_reason(reason: &str) -> ScriptMetaBackupReason {
    match reason {
        "beforeRestore" => ScriptMetaBackupReason::BeforeRestore,
        "resetInitial" => ScriptMetaBackupReason::ResetInitial,
        _ => ScriptMetaBackupReason::BeforeSave,
    }
}

fn validate_backup_file_names(records: &[ScriptMetaBackupIndexRecord]) -> ScriptMetaKitResult<()> {
    for record in records {
        validate_backup_file_name(&record.backup_file_name)?;
    }
    Ok(())
}

fn validate_backup_file_name(file_name: &str) -> ScriptMetaKitResult<()> {
    let mut components = Path::new(file_name).components();
    let is_single_file_name = matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none();
    if !is_single_file_name {
        return Err(ScriptMetaKitError::Cache(format!(
            "backup file name `{file_name}` must not contain a directory path"
        )));
    }
    Ok(())
}

fn scriptmeta_backup_record_from_index(
    record: &ScriptMetaBackupIndexRecord,
    backup_file_path: PathBuf,
) -> ScriptMetaBackupRecord {
    ScriptMetaBackupRecord {
        id: record.id.clone(),
        created_at_millis: record.created_at_millis,
        backup_file_name: record.backup_file_name.clone(),
        backup_file_path,
        file_size: record.file_size,
        reason: record.reason,
    }
}

fn clear_scriptmeta_backups_after_preserving(
    path: &Path,
    options: &ScriptMetaBackupOptions,
    generation_id: &str,
    preserved_generation_id: Option<&str>,
) -> ScriptMetaKitResult<()> {
    let mut index = load_backup_index(path, options)?;
    let Some(selected_index) = index
        .generations
        .iter()
        .position(|record| record.id == generation_id)
    else {
        return Err(ScriptMetaKitError::InvalidConfig(format!(
            "backup generation `{generation_id}` was not found"
        )));
    };
    let removed_records = index.generations.split_off(selected_index + 1);
    if removed_records.is_empty() {
        return Ok(());
    }
    let backup_directory = backup_generations_directory(path, options);
    let mut removed_paths = Vec::new();
    let mut preserved_records = Vec::new();
    for record in removed_records {
        if preserved_generation_id.is_some_and(|id| id == record.id) {
            preserved_records.push(record);
        } else {
            removed_paths.push(backup_directory.join(record.backup_file_name));
        }
    }
    index.generations.extend(preserved_records);
    write_backup_index(&index, &backup_index_path(path, options))?;
    for backup_path in removed_paths {
        if backup_path.exists() {
            let _ = fs::remove_file(backup_path);
        }
    }
    Ok(())
}

fn remove_scriptmeta_backup_if_possible(
    path: &Path,
    options: &ScriptMetaBackupOptions,
    record: &ScriptMetaBackupRecord,
) {
    if record.backup_file_path.exists() {
        let _ = fs::remove_file(&record.backup_file_path);
    }
    let Ok(mut index) = load_backup_index(path, options) else {
        return;
    };
    index.generations.retain(|entry| entry.id != record.id);
    if index
        .first_generation_id
        .as_ref()
        .is_some_and(|id| id == &record.id)
    {
        index.first_generation_id = index.generations.first().map(|entry| entry.id.clone());
    }
    let _ = write_backup_index(&index, &backup_index_path(path, options));
}

fn backup_file_directory(path: &Path, options: &ScriptMetaBackupOptions) -> PathBuf {
    options
        .root_directory
        .join("v1")
        .join("files")
        .join(backup_file_id(path))
}

fn backup_generations_directory(path: &Path, options: &ScriptMetaBackupOptions) -> PathBuf {
    backup_file_directory(path, options).join("generations")
}

fn backup_index_path(path: &Path, options: &ScriptMetaBackupOptions) -> PathBuf {
    backup_file_directory(path, options).join("index.json")
}

fn backup_file_id(path: &Path) -> String {
    path_key(path.to_string_lossy().as_bytes())
}

fn backup_file_name(generation_id: &str, path: &Path) -> String {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if !extension.is_empty() => format!("{generation_id}.{extension}"),
        _ => generation_id.to_string(),
    }
}

fn file_size(path: &Path) -> ScriptMetaKitResult<u64> {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
}

fn file_modified_millis(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

fn file_state_fingerprint(metadata: &fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0);
    format!("size:{}:modified:{modified}", metadata.len())
}

fn read_script_bytes_file(path: &Path) -> ScriptMetaKitResult<Vec<u8>> {
    fs::read(path).map_err(|error| ScriptMetaKitError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn ensure_script_file_writable(path: &Path) -> ScriptMetaKitResult<()> {
    let inspection = formats::inspect_script_file(path);
    if inspection.is_file_locked {
        return Err(ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: "script file is locked".to_string(),
        });
    }
    if inspection.is_read_only {
        return Err(ScriptMetaKitError::Io {
            path: path.to_path_buf(),
            message: "script file or its parent directory is read-only".to_string(),
        });
    }
    Ok(())
}

fn compiled_osa_temp_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(
        ".{}.{}.compiled.tmp.scpt",
        path.file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default(),
        Uuid::new_v4()
    ))
}

#[derive(Debug)]
enum AtomicWriteError {
    BeforeCommit(std::io::Error),
    #[cfg(not(windows))]
    DurabilityAfterCommit(std::io::Error),
}

impl AtomicWriteError {
    fn is_committed(&self) -> bool {
        #[cfg(not(windows))]
        {
            matches!(self, Self::DurabilityAfterCommit(_))
        }
        #[cfg(windows)]
        {
            false
        }
    }
}

impl std::fmt::Display for AtomicWriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BeforeCommit(error) => error.fmt(formatter),
            #[cfg(not(windows))]
            Self::DurabilityAfterCommit(error) => write!(
                formatter,
                "file replacement succeeded, but directory durability sync failed: {error}"
            ),
        }
    }
}

impl From<std::io::Error> for AtomicWriteError {
    fn from(error: std::io::Error) -> Self {
        Self::BeforeCommit(error)
    }
}

fn write_bytes_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), AtomicWriteError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temp_path = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default(),
        Uuid::new_v4()
    ));
    let mut temp_guard = TemporaryEditFile::new(temp_path.clone());
    {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    if path.exists() {
        copy_replacement_metadata(path, &temp_path)?;
    }
    replace_file(&temp_path, path)?;
    temp_guard.disarm();
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn copy_replacement_metadata(source_path: &Path, target_path: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let source = CString::new(source_path.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "source path contains a null byte",
        )
    })?;
    let target = CString::new(target_path.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target path contains a null byte",
        )
    })?;
    // SAFETY: both paths are valid, null-terminated C strings that remain alive
    // for the duration of copyfile. A null state is explicitly permitted.
    let result = unsafe {
        libc::copyfile(
            source.as_ptr(),
            target.as_ptr(),
            std::ptr::null_mut(),
            libc::COPYFILE_ACL | libc::COPYFILE_XATTR,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    fs::set_permissions(target_path, fs::metadata(source_path)?.permissions())
}

#[cfg(not(target_os = "macos"))]
fn copy_replacement_metadata(source_path: &Path, target_path: &Path) -> std::io::Result<()> {
    fs::set_permissions(target_path, fs::metadata(source_path)?.permissions())
}

struct TemporaryEditFile {
    path: PathBuf,
    cleanup: bool,
}

impl TemporaryEditFile {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            cleanup: true,
        }
    }

    fn disarm(&mut self) {
        self.cleanup = false;
    }
}

impl Drop for TemporaryEditFile {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(not(windows))]
fn replace_file(temp_path: &Path, target_path: &Path) -> Result<(), AtomicWriteError> {
    fs::rename(temp_path, target_path).map_err(AtomicWriteError::BeforeCommit)?;
    #[cfg(test)]
    if TEST_DIRECTORY_SYNC_FAILURE.with(|path| path.borrow().as_deref() == Some(target_path)) {
        return Err(AtomicWriteError::DurabilityAfterCommit(
            std::io::Error::other("injected directory sync failure"),
        ));
    }
    let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
    let sync_result = fs::File::open(parent).and_then(|directory| directory.sync_all());
    sync_result.map_err(AtomicWriteError::DurabilityAfterCommit)
}

#[cfg(all(test, not(windows)))]
thread_local! {
    static TEST_DIRECTORY_SYNC_FAILURE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn replace_file(temp_path: &Path, target_path: &Path) -> Result<(), AtomicWriteError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let temp_path_wide = temp_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target_path_wide = target_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: the UTF-16 buffers are null-terminated and remain alive for the
    // duration of the call. MoveFileExW does not retain the pointers.
    let moved = unsafe {
        MoveFileExW(
            temp_path_wide.as_ptr(),
            target_path_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(AtomicWriteError::BeforeCommit(
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

fn backup_timestamp_millis() -> u64 {
    now_millis()
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn short_uuid() -> String {
    Uuid::new_v4()
        .to_string()
        .replace('-', "")
        .chars()
        .take(12)
        .collect()
}

fn path_key(bytes: &[u8]) -> String {
    let mut state = 0xcbf29ce484222325u64;
    for byte in bytes {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x100000001b3);
    }
    format!("{state:016x}")
}

fn random_edit_password_salt() -> String {
    Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(16)
        .collect()
}

fn edit_password_digest_sha256(salt: &str, password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(b":");
    hasher.update(password.as_bytes());
    digest_to_hex(&hasher.finalize())
}

fn file_content_fingerprint(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    digest_to_hex(&hasher.finalize())
}

fn ensure_expected_source_fingerprint(
    path: &Path,
    expected: Option<&str>,
    actual: &str,
) -> ScriptMetaKitResult<()> {
    if expected.is_some_and(|expected| expected != actual) {
        return Err(ScriptMetaKitError::Conflict(format!(
            "{} changed after it was loaded",
            path.display()
        )));
    }
    Ok(())
}

fn ensure_file_fingerprint(path: &Path, expected: &str) -> ScriptMetaKitResult<()> {
    let bytes = read_script_bytes_file(path)?;
    ensure_expected_source_fingerprint(path, Some(expected), &file_content_fingerprint(&bytes))
}

fn parsed_edit_password_sha256(value: &str) -> Option<(&str, &str)> {
    let trimmed = value.trim();
    let separator = trimmed.find(':')?;
    let salt = &trimmed[..separator];
    let digest = &trimmed[separator + 1..];
    if salt.is_empty() || digest.len() != 64 {
        return None;
    }
    if !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    if digest.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return None;
    }
    Some((salt, digest))
}

fn digest_to_hex(digest: &[u8]) -> String {
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("write to String should not fail");
    }
    output
}

fn validate_distribution_metadata_draft(
    draft: &DistributionMetadataDraft,
) -> ScriptMetaKitResult<()> {
    validate_required_field("Script-ID", draft.script_id.as_str())?;
    validate_optional_version("Version", draft.version.as_deref())?;
    if !has_value(draft.version.as_deref()) && draft.latest_url.is_none() {
        return Err(ScriptMetaKitError::InvalidConfig(
            "SCRIPTMETA-DIST requires Version or Latest-URL".to_string(),
        ));
    }
    Ok(())
}

fn validate_optional_version(name: &str, value: Option<&str>) -> ScriptMetaKitResult<()> {
    if let Some(value) = value.filter(|value| !value.trim().is_empty())
        && normalize_version_string(value).is_none()
    {
        return Err(ScriptMetaKitError::InvalidConfig(format!(
            "{name} is invalid"
        )));
    }
    Ok(())
}

fn validate_required_field(name: &str, value: &str) -> ScriptMetaKitResult<()> {
    if value.trim().is_empty() {
        return Err(ScriptMetaKitError::InvalidConfig(format!(
            "{name} is required"
        )));
    }
    Ok(())
}

fn has_value(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn push_field(output: &mut String, key: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let _ = writeln!(output, "{key}={value}");
}

fn push_normalized_version_field(output: &mut String, key: &str, value: Option<&str>) {
    let Some(value) = normalized_version_string_ref(value) else {
        return;
    };
    let _ = writeln!(output, "{key}={value}");
}

fn normalized_version_string_ref(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(normalize_version_string)
}

fn wrap_metadata_block(block: &str, style: ScriptMetaCommentStyle) -> String {
    match style {
        ScriptMetaCommentStyle::JavaScriptBlock => {
            let mut output = String::with_capacity(block.len() + 7);
            output.push_str("/*\n");
            output.push_str(block);
            output.push_str("*/\n");
            output
        }
        ScriptMetaCommentStyle::AppleScriptBlock => {
            let mut output = String::with_capacity(block.len() + 7);
            output.push_str("(*\n");
            output.push_str(block);
            output.push_str("*)\n");
            output
        }
        ScriptMetaCommentStyle::PlainText => block.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(target_os = "macos")]
    use std::process::Command;

    use tempfile::tempdir;
    use url::Url;

    use super::{
        DistributionMetadataDraft, ScriptIdUniquenessItem, ScriptMetaBackupOptions,
        ScriptMetaBackupReason, ScriptMetaCommentStyle, ScriptMetaWriteMode, ScriptMetadataDraft,
        append_script_metadata_to_text, backup_file_directory, backup_index_path,
        clear_scriptmeta_backups, create_scriptmeta_backup, generate_edit_password_sha256,
        is_valid_edit_password_sha256, read_script_metadata_draft_from_file,
        render_distribution_metadata_block, render_script_metadata_for_style,
        reset_scriptmeta_backups_with_current_as_initial, restore_scriptmeta_backup,
        scriptmeta_backup_generations, validate_script_id_uniqueness, verify_edit_password_sha256,
        write_script_metadata_to_file, write_script_metadata_to_file_if_unchanged,
        write_script_metadata_to_text,
    };
    use crate::core::{parse_distribution_metadata_for_script, parse_script_metadata};

    #[test]
    fn validates_script_id_uniqueness() {
        let items = vec![
            item("/tmp/a.jsx", "com.example.same"),
            item("/tmp/b.jsx", "com.example.same"),
            item("/tmp/c.jsx", "com.example.other"),
        ];

        let report = validate_script_id_uniqueness(&items);

        assert_eq!(report.total_items, 3);
        assert_eq!(report.unique_script_ids, 2);
        assert!(!report.is_unique());
        assert_eq!(report.duplicates.len(), 1);
        assert_eq!(report.duplicates[0].script_id, "com.example.same");
        assert_eq!(report.duplicates[0].item_ids, ["/tmp/a.jsx", "/tmp/b.jsx"]);
    }

    #[test]
    fn validates_script_id_uniqueness_ignores_empty_ids() {
        let items = vec![
            item("/tmp/a.jsx", ""),
            item("/tmp/b.jsx", ""),
            item("/tmp/c.jsx", "com.example.one"),
        ];

        let report = validate_script_id_uniqueness(&items);

        assert_eq!(report.total_items, 3);
        assert_eq!(report.unique_script_ids, 1);
        assert!(report.is_unique());
    }

    #[test]
    fn conditional_metadata_write_rejects_an_external_change() {
        let directory = tempdir().expect("tempdir");
        let script_path = directory.path().join("conditional.jsx");
        fs::write(
            &script_path,
            "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.conditional\n// Version=1.0.0\n// SCRIPTMETA-END\n",
        )
        .expect("initial script");
        let loaded = read_script_metadata_draft_from_file(&script_path).expect("loaded draft");

        fs::write(
            &script_path,
            "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.conditional\n// Version=1.0.1\n// SCRIPTMETA-END\n",
        )
        .expect("external edit");
        let error = write_script_metadata_to_file_if_unchanged(
            &script_path,
            &ScriptMetadataDraft {
                script_id: "com.example.conditional".to_string(),
                version: Some("2.0.0".to_string()),
                ..ScriptMetadataDraft::default()
            },
            ScriptMetaWriteMode::InsertOrReplace,
            None,
            Some(&loaded.source_fingerprint),
        )
        .expect_err("external change must be rejected");

        assert!(matches!(error, crate::ScriptMetaKitError::Conflict(_)));
        let current = fs::read_to_string(&script_path).expect("current script");
        assert!(current.contains("Version=1.0.1"));
        assert!(!current.contains("Version=2.0.0"));
    }

    #[test]
    fn renders_and_parses_javascript_script_metadata() {
        let draft = ScriptMetadataDraft {
            script_id: "com.example.tool".to_string(),
            version: Some("v1. 2 .3".to_string()),
            description: Some("Line 1\nLine 2".to_string()),
            target_app: Some("Photoshop".to_string()),
            min_target_version: Some("25.0".to_string()),
            meta_url: Some(Url::parse("https://example.com/SCRIPTMETA.txt").expect("url")),
            name: Some("Example Tool".to_string()),
            author: Some("Example Author".to_string()),
            release_date: Some("2026-05-30".to_string()),
            edit_password_sha256: None,
        };

        let rendered =
            render_script_metadata_for_style(&draft, ScriptMetaCommentStyle::JavaScriptBlock)
                .expect("render");
        let parsed = parse_script_metadata(&rendered).expect("parse");

        assert_eq!(parsed.script_id, "com.example.tool");
        assert_eq!(parsed.version.as_deref(), Some("1.2.3"));
        assert_eq!(parsed.description.as_deref(), Some("Line 1\nLine 2"));
        assert_eq!(parsed.target_app.as_deref(), Some("Photoshop"));
        assert_eq!(parsed.min_target_version.as_deref(), Some("25.0"));
    }

    #[test]
    fn appends_script_metadata_for_supported_text_script() {
        let draft = ScriptMetadataDraft {
            script_id: "com.example.appended".to_string(),
            ..ScriptMetadataDraft::default()
        };

        let updated =
            append_script_metadata_to_text("alert('hello');\n", Path::new("example.jsx"), &draft)
                .expect("append");

        assert!(updated.contains("/*\n\nSCRIPTMETA-BEGIN\n"));
        assert!(updated.contains("Script-ID=com.example.appended\n"));
        assert!(parse_script_metadata(&updated).is_ok());
    }

    #[test]
    fn replaces_existing_script_metadata() {
        let draft = ScriptMetadataDraft {
            script_id: "com.example.new".to_string(),
            name: Some("New Name".to_string()),
            ..ScriptMetadataDraft::default()
        };

        let updated = append_script_metadata_to_text(
            "/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.old\nUnknown-Key=keep\nSCRIPTMETA-END\n*/\n",
            Path::new("example.jsx"),
            &draft,
        )
        .expect("replace");

        assert!(updated.contains("Script-ID=com.example.new\n"));
        assert!(updated.contains("Name=New Name\n"));
        assert!(updated.contains("Unknown-Key=keep\n"));
        assert!(!updated.contains("Script-ID=com.example.old\n"));
    }

    #[test]
    fn replaces_case_insensitive_colon_script_metadata() {
        let draft = ScriptMetadataDraft {
            script_id: "com.example.new".to_string(),
            name: Some("New Name".to_string()),
            ..ScriptMetadataDraft::default()
        };

        let updated = append_script_metadata_to_text(
            "/*\nscriptmeta-begin\nscript-id: com.example.old\nUnknown-Key: keep\nscriptmeta-end\n*/\n",
            Path::new("example.jsx"),
            &draft,
        )
        .expect("replace");

        assert!(updated.contains("Script-ID=com.example.new\n"));
        assert!(updated.contains("Name=New Name\n"));
        assert!(updated.contains("Unknown-Key=keep\n"));
        assert!(!updated.contains("script-id: com.example.old\n"));
        assert!(parse_script_metadata(&updated).is_ok());
    }

    #[test]
    fn replaces_script_id_inside_star_prefixed_jsdoc_metadata() {
        let source = "/**\n * SCRIPTMETA-BEGIN\n * Script-ID=com.example.old\n * Unknown-Key=keep\n * SCRIPTMETA-END\n */\nalert('x');\n";
        let read = super::read_script_metadata_draft_from_text(source, Path::new("example.jsx"))
            .expect("read");
        assert_eq!(read.draft.script_id, "com.example.old");

        let draft = ScriptMetadataDraft {
            script_id: "com.example.new".to_string(),
            ..ScriptMetadataDraft::default()
        };
        let updated = write_script_metadata_to_text(
            source,
            Path::new("example.jsx"),
            &draft,
            ScriptMetaWriteMode::InsertOrReplace,
        )
        .expect("write");

        assert!(!updated.text.contains("Script-ID=com.example.old"));
        assert!(updated.text.contains("Script-ID=com.example.new"));
        assert!(updated.text.contains("Unknown-Key=keep"));
        assert_eq!(
            parse_script_metadata(&updated.text)
                .expect("parse")
                .script_id,
            "com.example.new"
        );
    }

    #[test]
    fn insert_only_rejects_existing_script_metadata() {
        let draft = ScriptMetadataDraft {
            script_id: "com.example.new".to_string(),
            ..ScriptMetadataDraft::default()
        };

        let result = write_script_metadata_to_text(
            "/*\nSCRIPTMETA-BEGIN\nScript-ID=com.example.old\nSCRIPTMETA-END\n*/\n",
            Path::new("example.jsx"),
            &draft,
            ScriptMetaWriteMode::InsertOnly,
        );

        assert!(result.is_err());
    }

    #[test]
    fn inserts_script_metadata_into_leading_block_comment() {
        let draft = ScriptMetadataDraft {
            script_id: "com.example.inserted".to_string(),
            ..ScriptMetadataDraft::default()
        };

        let updated = append_script_metadata_to_text(
            "/*\nExisting header.\n*/\nalert('hello');\n",
            Path::new("example.jsx"),
            &draft,
        )
        .expect("insert");

        assert!(updated.starts_with("/*\nExisting header.\n\nSCRIPTMETA-BEGIN\n"));
        assert!(updated.contains("SCRIPTMETA-END\n\n*/\nalert"));
        assert!(parse_script_metadata(&updated).is_ok());
    }

    #[test]
    fn ignores_scriptmeta_marker_inside_non_comment_text() {
        let draft = ScriptMetadataDraft {
            script_id: "com.example.inserted".to_string(),
            ..ScriptMetadataDraft::default()
        };

        let updated = append_script_metadata_to_text(
            "var marker = 'SCRIPTMETA-BEGIN SCRIPTMETA-END';\n",
            Path::new("example.jsx"),
            &draft,
        )
        .expect("insert");

        assert!(updated.contains("var marker"));
        assert!(updated.contains("Script-ID=com.example.inserted\n"));
    }

    #[test]
    fn writes_backup_and_restores_generation() {
        let directory = tempdir().expect("tempdir");
        let script_path = directory.path().join("example.jsx");
        let backup_root = directory.path().join("backups");
        fs::write(&script_path, "alert('before');\n").expect("write script");
        let backup_options = ScriptMetaBackupOptions {
            root_directory: backup_root,
        };
        let draft = ScriptMetadataDraft {
            script_id: "com.example.backup".to_string(),
            ..ScriptMetadataDraft::default()
        };

        let result = write_script_metadata_to_file(
            &script_path,
            &draft,
            ScriptMetaWriteMode::InsertOrReplace,
            Some(&backup_options),
        )
        .expect("write metadata");
        let backup_record = result.backup.as_ref().expect("backup");
        assert!(backup_record.backup_file_path.exists());

        let generations =
            scriptmeta_backup_generations(&script_path, &backup_options).expect("generations");
        let first_backup = generations
            .iter()
            .find(|generation| !generation.is_current_file)
            .expect("backup");
        assert_eq!(
            fs::read_to_string(&first_backup.file_path).expect("read backup"),
            "alert('before');\n"
        );

        restore_scriptmeta_backup(&script_path, &backup_options, &first_backup.id)
            .expect("restore");
        assert_eq!(
            fs::read_to_string(&script_path).expect("read restored"),
            "alert('before');\n"
        );

        clear_scriptmeta_backups(&script_path, &backup_options).expect("clear backups");
    }

    #[test]
    fn rejects_backup_index_paths_outside_the_generation_directory() {
        let directory = tempdir().expect("tempdir");
        let script_path = directory.path().join("example.jsx");
        fs::write(&script_path, "alert('current');\n").expect("write script");
        let backup_options = ScriptMetaBackupOptions {
            root_directory: directory.path().join("backups"),
        };
        let index_path = backup_index_path(&script_path, &backup_options);
        fs::create_dir_all(index_path.parent().expect("index parent")).expect("backup directory");
        fs::write(
            &index_path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "file_id": "ignored",
                "original_path": script_path,
                "file_name": "example.jsx",
                "first_generation_id": "malicious",
                "generations": [{
                    "id": "malicious",
                    "created_at_millis": 1,
                    "backup_file_name": "../../outside.jsx",
                    "file_size": 1,
                    "reason": "before_save"
                }]
            }))
            .expect("index json"),
        )
        .expect("index");

        let error = scriptmeta_backup_generations(&script_path, &backup_options)
            .expect_err("unsafe backup path");
        assert!(
            error
                .to_string()
                .contains("must not contain a directory path")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn directory_sync_failure_reports_committed_write_and_keeps_backup() {
        let directory = tempdir().expect("tempdir");
        let script_path = directory.path().join("example.jsx");
        fs::write(&script_path, "alert('before');\n").expect("write script");
        let backup_options = ScriptMetaBackupOptions {
            root_directory: directory.path().join("backups"),
        };
        let draft = ScriptMetadataDraft {
            script_id: "com.example.committed-sync-failure".to_string(),
            ..ScriptMetadataDraft::default()
        };

        super::TEST_DIRECTORY_SYNC_FAILURE
            .with(|failure| *failure.borrow_mut() = Some(script_path.clone()));
        let result = write_script_metadata_to_file(
            &script_path,
            &draft,
            ScriptMetaWriteMode::InsertOrReplace,
            Some(&backup_options),
        );
        super::TEST_DIRECTORY_SYNC_FAILURE.with(|failure| *failure.borrow_mut() = None);

        let error = result.expect_err("directory sync failure");
        assert!(error.to_string().contains("file replacement succeeded"));
        assert!(
            fs::read_to_string(&script_path)
                .expect("committed script")
                .contains("Script-ID=com.example.committed-sync-failure")
        );
        let generations =
            scriptmeta_backup_generations(&script_path, &backup_options).expect("generations");
        let backup = generations
            .iter()
            .find(|generation| !generation.is_current_file)
            .expect("before-save backup");
        assert!(backup.file_path.exists());
        assert_eq!(
            fs::read_to_string(&backup.file_path).expect("backup"),
            "alert('before');\n"
        );
    }

    #[test]
    fn reset_backups_replaces_history_only_after_staging_the_current_file() {
        let directory = tempdir().expect("tempdir");
        let script_path = directory.path().join("example.jsx");
        fs::write(&script_path, "alert('old');\n").expect("write script");
        let backup_options = ScriptMetaBackupOptions {
            root_directory: directory.path().join("backups"),
        };
        create_scriptmeta_backup(
            &script_path,
            &backup_options,
            ScriptMetaBackupReason::BeforeSave,
        )
        .expect("old backup");
        fs::write(&script_path, "alert('current');\n").expect("update script");

        let reset = reset_scriptmeta_backups_with_current_as_initial(&script_path, &backup_options)
            .expect("reset backups");
        let generations = scriptmeta_backup_generations(&script_path, &backup_options)
            .expect("reset generations");

        assert!(reset.backup_file_path.exists());
        assert_eq!(generations.len(), 1);
        assert_eq!(generations[0].id, reset.id);
        assert_eq!(generations[0].reason, ScriptMetaBackupReason::ResetInitial);
        assert_eq!(
            fs::read_to_string(&generations[0].file_path).expect("read reset backup"),
            "alert('current');\n"
        );
    }

    #[test]
    fn failed_backup_reset_keeps_the_previous_history() {
        let directory = tempdir().expect("tempdir");
        let script_path = directory.path().join("example.jsx");
        fs::write(&script_path, "alert('old');\n").expect("write script");
        let backup_options = ScriptMetaBackupOptions {
            root_directory: directory.path().join("backups"),
        };
        let previous = create_scriptmeta_backup(
            &script_path,
            &backup_options,
            ScriptMetaBackupReason::BeforeSave,
        )
        .expect("old backup");
        fs::remove_file(&script_path).expect("remove current script");

        assert!(
            reset_scriptmeta_backups_with_current_as_initial(&script_path, &backup_options)
                .is_err()
        );
        let generations = scriptmeta_backup_generations(&script_path, &backup_options)
            .expect("previous generations");
        assert_eq!(generations.len(), 1);
        assert_eq!(generations[0].id, previous.id);
        assert!(previous.backup_file_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_metadata_write_preserves_executable_permissions() {
        let directory = tempdir().expect("tempdir");
        let script_path = directory.path().join("example.jsx");
        fs::write(&script_path, "alert('before');\n").expect("write script");
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
            .expect("make executable");
        let draft = ScriptMetadataDraft {
            script_id: "com.example.permissions".to_string(),
            ..ScriptMetadataDraft::default()
        };

        write_script_metadata_to_file(
            &script_path,
            &draft,
            ScriptMetaWriteMode::InsertOrReplace,
            None,
        )
        .expect("write metadata");

        assert_eq!(
            fs::metadata(&script_path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }

    #[test]
    fn metadata_write_rejects_read_only_files_without_changing_them() {
        let directory = tempdir().expect("tempdir");
        let script_path = directory.path().join("example.jsx");
        let original = "alert('before');\n";
        fs::write(&script_path, original).expect("write script");
        let mut permissions = fs::metadata(&script_path).expect("metadata").permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&script_path, permissions).expect("make read-only");
        let draft = ScriptMetadataDraft {
            script_id: "com.example.readonly".to_string(),
            ..ScriptMetadataDraft::default()
        };

        let result = write_script_metadata_to_file(
            &script_path,
            &draft,
            ScriptMetaWriteMode::InsertOrReplace,
            None,
        );

        #[cfg(unix)]
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o644))
            .expect("restore permissions");
        #[cfg(not(unix))]
        {
            let mut permissions = fs::metadata(&script_path).expect("metadata").permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(&script_path, permissions).expect("restore permissions");
        }
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(&script_path).expect("read script"),
            original
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn atomic_metadata_write_preserves_extended_attributes() {
        let directory = tempdir().expect("tempdir");
        let script_path = directory.path().join("example.jsx");
        fs::write(&script_path, "alert('before');\n").expect("write script");
        let attribute = "com.example.ScriptMetaKit.test";
        let set_status = Command::new("/usr/bin/xattr")
            .args(["-w", attribute, "preserved"])
            .arg(&script_path)
            .status()
            .expect("set xattr");
        assert!(set_status.success());
        let draft = ScriptMetadataDraft {
            script_id: "com.example.xattr".to_string(),
            ..ScriptMetadataDraft::default()
        };

        write_script_metadata_to_file(
            &script_path,
            &draft,
            ScriptMetaWriteMode::InsertOrReplace,
            None,
        )
        .expect("write metadata");

        let output = Command::new("/usr/bin/xattr")
            .args(["-p", attribute])
            .arg(&script_path)
            .output()
            .expect("read xattr");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "preserved");
    }

    #[test]
    fn reads_legacy_scripta_backup_index() {
        let directory = tempdir().expect("tempdir");
        let script_path = directory.path().join("Legacy.jsx");
        fs::write(&script_path, "alert('current');\n").expect("write script");
        let backup_options = ScriptMetaBackupOptions {
            root_directory: directory.path().join("backups"),
        };
        let backup_directory = backup_file_directory(&script_path, &backup_options);
        let generations_directory = backup_directory.join("generations");
        fs::create_dir_all(&generations_directory).expect("backup dir");
        let backup_file_name = "20260606-153000-abcdef123456.jsx";
        let backup_path = generations_directory.join(backup_file_name);
        fs::write(&backup_path, "alert('legacy');\n").expect("backup file");
        let backup_size = fs::metadata(&backup_path).expect("backup metadata").len();
        let legacy_index = serde_json::json!({
            "schemaVersion": 1,
            "fileID": "legacy",
            "originalPath": script_path.to_string_lossy(),
            "fileName": "Legacy.jsx",
            "firstGenerationID": "20260606-153000-abcdef123456",
            "generations": [
                {
                    "id": "20260606-153000-abcdef123456",
                    "createdAt": "2026-06-06T06:30:00Z",
                    "backupFileName": backup_file_name,
                    "fileSize": backup_size,
                    "sha256": "unused",
                    "reason": "beforeRestore",
                }
            ]
        });
        fs::write(
            backup_directory.join("index.json"),
            serde_json::to_vec_pretty(&legacy_index).expect("legacy index json"),
        )
        .expect("legacy index");

        let generations =
            scriptmeta_backup_generations(&script_path, &backup_options).expect("generations");
        let legacy_generation = generations
            .iter()
            .find(|generation| !generation.is_current_file)
            .expect("legacy generation");

        assert_eq!(legacy_generation.id, "20260606-153000-abcdef123456");
        assert_eq!(legacy_generation.file_path, backup_path);
        assert_eq!(legacy_generation.file_size, backup_size);
        assert_eq!(
            legacy_generation.reason,
            ScriptMetaBackupReason::BeforeRestore
        );
    }

    #[test]
    fn renders_and_parses_distribution_metadata() {
        let records = vec![
            DistributionMetadataDraft {
                script_id: "com.example.a".to_string(),
                version: Some("v2. 0 .0".to_string()),
                latest_url: None,
                latest_page_url: Some(Url::parse("https://example.com/a").expect("url")),
            },
            DistributionMetadataDraft {
                script_id: "com.example.b".to_string(),
                version: None,
                latest_url: Some(Url::parse("https://example.com/b/latest").expect("url")),
                latest_page_url: None,
            },
        ];

        let rendered = render_distribution_metadata_block(&records).expect("render");
        assert!(!rendered.contains("Name="));
        assert!(!rendered.contains("Author="));
        assert!(!rendered.contains("Note="));
        let parsed_a =
            parse_distribution_metadata_for_script(&rendered, "com.example.a").expect("parse a");
        let parsed_b =
            parse_distribution_metadata_for_script(&rendered, "com.example.b").expect("parse b");

        assert_eq!(parsed_a.latest_version.as_deref(), Some("2.0.0"));
        assert_eq!(
            parsed_b.latest_url.as_ref().map(Url::as_str),
            Some("https://example.com/b/latest")
        );
    }

    #[test]
    fn generates_and_verifies_edit_password_sha256() {
        let stored = generate_edit_password_sha256("secret").expect("generated password hash");

        assert!(is_valid_edit_password_sha256(stored.as_str()));
        assert!(!is_valid_edit_password_sha256("invalid"));
        assert!(verify_edit_password_sha256("secret", stored.as_str()));
        assert!(!verify_edit_password_sha256("different", stored.as_str()));
        assert!(!verify_edit_password_sha256("secret", "invalid"));
    }

    fn item(file_path: &str, script_id: &str) -> ScriptIdUniquenessItem {
        ScriptIdUniquenessItem {
            item_id: file_path.to_string(),
            file_path: file_path.into(),
            script_id: script_id.to_string(),
        }
    }
}
