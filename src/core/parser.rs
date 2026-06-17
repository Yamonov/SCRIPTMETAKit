use std::{borrow::Cow, convert::Infallible};

use url::Url;

use super::{
    DistributionMetadata, ScriptMetaKitError, ScriptMetaKitResult, ScriptMetadata,
    normalize_version_string,
};

const SCRIPT_BEGIN: &str = "SCRIPTMETA-BEGIN";
const SCRIPT_END: &str = "SCRIPTMETA-END";
const DIST_BEGIN: &str = "SCRIPTMETA-DIST-BEGIN";
const DIST_END: &str = "SCRIPTMETA-DIST-END";
const DESCRIPTION_BEGIN: &str = "Description-BEGIN";
const DESCRIPTION_END: &str = "Description-END";
const SCRIPT_METADATA_KEYS: &[&str] = &[
    "Script-ID",
    "Version",
    "Description",
    "Target-App",
    "Min-Target-Version",
    "Meta-URL",
    "Latest-URL",
    "Latest-Version",
    "Latest-Page-URL",
    "Name",
    "Author",
    "Release-Date",
    "Edit-Password-SHA256",
];
const DISTRIBUTION_METADATA_KEYS: &[&str] = &[
    "Script-ID",
    "Version",
    "Latest-Version",
    "Latest-URL",
    "Latest-Page-URL",
    "Name",
    "Author",
    "Release-Date",
];

pub fn parse_script_metadata(text: &str) -> ScriptMetaKitResult<ScriptMetadata> {
    let body = block_between(text, SCRIPT_BEGIN, SCRIPT_END)
        .ok_or_else(|| ScriptMetaKitError::Parse("missing SCRIPTMETA block".to_string()))?;
    let fields = parse_script_metadata_fields(body);
    let script_id = fields.script_id.ok_or_else(|| {
        ScriptMetaKitError::Parse("missing required SCRIPTMETA field `script-id`".to_string())
    })?;

    Ok(ScriptMetadata {
        script_id,
        version: optional_version_value(fields.version)?,
        description: fields.description,
        target_app: fields.target_app,
        min_target_version: fields.min_target_version,
        meta_url: optional_url_value(fields.meta_url)?,
        latest_url: optional_url_value(fields.latest_url)?,
        latest_version: optional_version_value(fields.latest_version)?,
        latest_page_url: optional_url_value(fields.latest_page_url)?,
        name: fields.name,
        author: fields.author,
        release_date: fields.release_date,
        edit_password_sha256: fields.edit_password_sha256,
    })
}

pub fn parse_distribution_metadata(text: &str) -> ScriptMetaKitResult<DistributionMetadata> {
    let body = distribution_body(text)?;
    distribution_metadata_from_fields(parse_distribution_metadata_fields(body.as_ref()))
}

pub fn parse_distribution_metadata_records(
    text: &str,
) -> ScriptMetaKitResult<Vec<DistributionMetadata>> {
    let body = distribution_body(text)?;
    parse_distribution_entries(body.as_ref())
}

pub fn parse_distribution_metadata_for_script(
    text: &str,
    script_id: &str,
) -> ScriptMetaKitResult<DistributionMetadata> {
    let records = parse_distribution_metadata_records(text)?;
    select_distribution_metadata_for_script(&records, script_id)
}

pub(crate) fn select_distribution_metadata_for_script(
    entries: &[DistributionMetadata],
    script_id: &str,
) -> ScriptMetaKitResult<DistributionMetadata> {
    if let Some(entry) = entries
        .iter()
        .rev()
        .find(|entry| entry.script_id.as_deref() == Some(script_id))
    {
        return Ok(entry.clone());
    }

    if entries.len() == 1 || entries.iter().all(|entry| entry.script_id.is_none()) {
        return entries.first().cloned().ok_or_else(|| {
            ScriptMetaKitError::Parse("missing SCRIPTMETA distribution fields".to_string())
        });
    }

    Ok(DistributionMetadata {
        script_id: None,
        latest_version: None,
        latest_url: None,
        latest_page_url: entries
            .iter()
            .find_map(|entry| entry.latest_page_url.clone()),
        note: Some(format!(
            "distribution metadata for script id `{script_id}` was not found"
        )),
    })
}

fn distribution_body(text: &str) -> ScriptMetaKitResult<Cow<'_, str>> {
    block_between(text, DIST_BEGIN, DIST_END)
        .map(normalize_distribution_body)
        .ok_or_else(|| {
            ScriptMetaKitError::Parse("missing SCRIPTMETA distribution block".to_string())
        })
}

fn parse_distribution_entries(body: &str) -> ScriptMetaKitResult<Vec<DistributionMetadata>> {
    let mut entries = Vec::new();
    let mut fields = DistributionMetadataFields::default();

    for raw_line in body.lines() {
        let line = clean_line(raw_line);
        if line.is_empty() {
            continue;
        }

        try_for_each_metadata_field(
            line,
            DISTRIBUTION_METADATA_KEYS,
            |key, value| -> ScriptMetaKitResult<()> {
                let value = value.trim();
                let key = key.trim();
                if key.is_empty() || value.is_empty() {
                    return Ok(());
                }

                if key.eq_ignore_ascii_case("script-id") && fields.script_id.is_some() {
                    let previous_fields = std::mem::take(&mut fields);
                    if previous_fields.script_id.is_some() {
                        entries.push(distribution_metadata_from_fields(previous_fields)?);
                    }
                }
                set_distribution_metadata_field(&mut fields, key, value);
                Ok(())
            },
        )?;
    }

    if fields.has_any && fields.script_id.is_some() {
        entries.push(distribution_metadata_from_fields(fields)?);
    }

    Ok(entries)
}

fn distribution_metadata_from_fields(
    fields: DistributionMetadataFields,
) -> ScriptMetaKitResult<DistributionMetadata> {
    Ok(DistributionMetadata {
        script_id: fields.script_id,
        latest_version: optional_version_value(fields.version.or(fields.latest_version))?,
        latest_url: optional_url_value(fields.latest_url)?,
        latest_page_url: optional_url_value(fields.latest_page_url)?,
        note: fields.note,
    })
}

fn block_between<'a>(text: &'a str, begin: &str, end: &str) -> Option<&'a str> {
    let begin_index = find_ascii_case_insensitive(text, begin)?;
    let after_begin = begin_index + begin.len();
    let rest = &text[after_begin..];
    let end_index = find_ascii_case_insensitive(rest, end)?;
    Some(&rest[..end_index])
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

#[derive(Default)]
struct ScriptMetadataFields {
    script_id: Option<String>,
    version: Option<String>,
    description: Option<String>,
    target_app: Option<String>,
    min_target_version: Option<String>,
    meta_url: Option<String>,
    latest_url: Option<String>,
    latest_version: Option<String>,
    latest_page_url: Option<String>,
    name: Option<String>,
    author: Option<String>,
    release_date: Option<String>,
    edit_password_sha256: Option<String>,
}

#[derive(Default)]
struct DistributionMetadataFields {
    has_any: bool,
    script_id: Option<String>,
    version: Option<String>,
    latest_version: Option<String>,
    latest_url: Option<String>,
    latest_page_url: Option<String>,
    note: Option<String>,
}

fn parse_distribution_metadata_fields(body: &str) -> DistributionMetadataFields {
    let mut fields = DistributionMetadataFields::default();

    for raw_line in body.lines() {
        let line = clean_line(raw_line);
        if line.is_empty() {
            continue;
        }

        for_each_metadata_field(line, DISTRIBUTION_METADATA_KEYS, |key, value| {
            let value = value.trim();
            let key = key.trim();
            if key.is_empty() || value.is_empty() {
                return;
            }

            set_distribution_metadata_field(&mut fields, key, value);
        });
    }

    fields
}

fn set_distribution_metadata_field(
    fields: &mut DistributionMetadataFields,
    key: &str,
    value: &str,
) {
    let target = if key.eq_ignore_ascii_case("script-id") {
        &mut fields.script_id
    } else if key.eq_ignore_ascii_case("version") {
        &mut fields.version
    } else if key.eq_ignore_ascii_case("latest-version") {
        &mut fields.latest_version
    } else if key.eq_ignore_ascii_case("latest-url") {
        &mut fields.latest_url
    } else if key.eq_ignore_ascii_case("latest-page-url") {
        &mut fields.latest_page_url
    } else {
        return;
    };

    fields.has_any = true;
    *target = Some(value.to_string());
}

fn parse_script_metadata_fields(body: &str) -> ScriptMetadataFields {
    let mut fields = ScriptMetadataFields::default();
    let mut description = String::new();
    let mut description_needs_separator = false;
    let mut in_description = false;

    for raw_line in body.lines() {
        let line = clean_line(raw_line);
        if line.is_empty() {
            if in_description {
                push_description_line(&mut description, &mut description_needs_separator, "");
            }
            continue;
        }

        if line == DESCRIPTION_BEGIN {
            in_description = true;
            description.clear();
            description_needs_separator = false;
            continue;
        }

        if line == DESCRIPTION_END {
            in_description = false;
            fields.description = Some(std::mem::take(&mut description));
            description_needs_separator = false;
            continue;
        }

        if in_description {
            push_description_line(&mut description, &mut description_needs_separator, line);
            continue;
        }

        for_each_metadata_field(line, SCRIPT_METADATA_KEYS, |key, value| {
            let value = value.trim();
            if value.is_empty() {
                return;
            }

            set_script_metadata_field(&mut fields, key.trim(), value);
        });
    }

    fields
}

fn push_description_line(buffer: &mut String, needs_separator: &mut bool, line: &str) {
    if *needs_separator {
        buffer.push('\n');
    } else {
        *needs_separator = true;
    }
    buffer.push_str(line);
}

fn set_script_metadata_field(fields: &mut ScriptMetadataFields, key: &str, value: &str) {
    let target = if key.eq_ignore_ascii_case("script-id") {
        &mut fields.script_id
    } else if key.eq_ignore_ascii_case("version") {
        &mut fields.version
    } else if key.eq_ignore_ascii_case("description") {
        &mut fields.description
    } else if key.eq_ignore_ascii_case("target-app") {
        &mut fields.target_app
    } else if key.eq_ignore_ascii_case("min-target-version") {
        &mut fields.min_target_version
    } else if key.eq_ignore_ascii_case("meta-url") {
        &mut fields.meta_url
    } else if key.eq_ignore_ascii_case("latest-url") {
        &mut fields.latest_url
    } else if key.eq_ignore_ascii_case("latest-version") {
        &mut fields.latest_version
    } else if key.eq_ignore_ascii_case("latest-page-url") {
        &mut fields.latest_page_url
    } else if key.eq_ignore_ascii_case("name") {
        &mut fields.name
    } else if key.eq_ignore_ascii_case("author") {
        &mut fields.author
    } else if key.eq_ignore_ascii_case("release-date") {
        &mut fields.release_date
    } else if key.eq_ignore_ascii_case("edit-password-sha256") {
        &mut fields.edit_password_sha256
    } else {
        return;
    };

    *target = Some(value.to_string());
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let colon_index = line.find(':');
    let equals_index = line.find('=');
    let separator_index = match (colon_index, equals_index) {
        (Some(colon), Some(equals)) => colon.min(equals),
        (Some(colon), None) => colon,
        (None, Some(equals)) => equals,
        (None, None) => return None,
    };

    Some((&line[..separator_index], &line[separator_index + 1..]))
}

fn for_each_metadata_field<'a>(
    line: &'a str,
    known_keys: &[&str],
    mut handle: impl FnMut(&'a str, &'a str),
) {
    let result: Result<(), Infallible> =
        try_for_each_metadata_field(line, known_keys, |key, value| {
            handle(key, value);
            Ok(())
        });
    if let Err(error) = result {
        match error {}
    }
}

fn try_for_each_metadata_field<'a, E>(
    line: &'a str,
    known_keys: &[&str],
    mut handle: impl FnMut(&'a str, &'a str) -> Result<(), E>,
) -> Result<(), E> {
    let fields = find_metadata_field_boundaries(line, known_keys);
    if fields.is_empty() {
        if let Some((key, value)) = split_key_value(line) {
            handle(key, value)?;
        }
        return Ok(());
    }

    for (index, field) in fields.iter().enumerate() {
        let value_end = fields
            .get(index + 1)
            .map_or(line.len(), |next| next.key_start);
        handle(
            line[field.key_start..field.key_end].trim(),
            line[field.value_start..value_end].trim(),
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct MetadataFieldBoundary {
    key_start: usize,
    key_end: usize,
    value_start: usize,
}

fn find_metadata_field_boundaries(line: &str, known_keys: &[&str]) -> Vec<MetadataFieldBoundary> {
    let mut fields = Vec::new();
    for (index, _) in line.char_indices() {
        if !is_metadata_key_boundary(line, index) {
            continue;
        }

        if let Some(field) = metadata_field_at(line, index, known_keys) {
            fields.push(field);
        }
    }
    fields
}

fn metadata_field_at(
    line: &str,
    key_start: usize,
    known_keys: &[&str],
) -> Option<MetadataFieldBoundary> {
    for key in known_keys {
        let key_end = key_start + key.len();
        if key_end > line.len() || !line.is_char_boundary(key_end) {
            continue;
        }
        let candidate = &line[key_start..key_end];
        if !candidate.eq_ignore_ascii_case(key) {
            continue;
        }
        let value_start = metadata_value_start(line, key_end)?;
        return Some(MetadataFieldBoundary {
            key_start,
            key_end,
            value_start,
        });
    }
    None
}

fn metadata_value_start(line: &str, key_end: usize) -> Option<usize> {
    for (offset, ch) in line[key_end..].char_indices() {
        if ch.is_whitespace() {
            continue;
        }
        let separator_start = key_end + offset;
        if ch == ':' || ch == '=' {
            return Some(separator_start + ch.len_utf8());
        }
        return None;
    }
    None
}

fn is_metadata_key_boundary(line: &str, index: usize) -> bool {
    index == 0
        || line[..index]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
}

fn clean_line(line: &str) -> &str {
    let mut trimmed = line.trim();
    for prefix in ["//", "--", "#", "*", "/*", "*/", ";"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            trimmed = rest.trim();
        }
    }
    trimmed
}

fn normalize_distribution_body(body: &str) -> Cow<'_, str> {
    if !body.contains('<') && !body.contains('&') && !body.contains("\\u") && !body.contains("\\/")
    {
        return Cow::Borrowed(body);
    }

    let escaped = decode_json_html_escapes(body);
    let text = strip_html_tags(&escaped);
    Cow::Owned(decode_basic_html_entities(&text))
}

fn decode_json_html_escapes(text: &str) -> Cow<'_, str> {
    if !text.contains("\\u") && !text.contains("\\/") && !text.contains("\\\"") {
        return Cow::Borrowed(text);
    }

    Cow::Owned(
        text.replace("\\u003C", "<")
            .replace("\\u003c", "<")
            .replace("\\u003E", ">")
            .replace("\\u003e", ">")
            .replace("\\u002F", "/")
            .replace("\\u002f", "/")
            .replace("\\/", "/")
            .replace("\\\"", "\""),
    )
}

fn strip_html_tags(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut index = 0usize;
    while index < text.len() {
        let bytes = text.as_bytes();
        if bytes[index] == b'<'
            && let Some(end_offset) = bytes[index..].iter().position(|byte| *byte == b'>')
        {
            let tag_end = index + end_offset;
            let tag = &text[index + 1..tag_end];
            if is_html_break_tag(tag) {
                output.push('\n');
            } else if !output.ends_with(char::is_whitespace) {
                output.push(' ');
            }
            index = tag_end + 1;
            continue;
        }

        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        output.push(ch);
        index += ch.len_utf8();
    }
    output
}

fn is_html_break_tag(tag: &str) -> bool {
    let trimmed = tag.trim_start();
    let tag_name_start = if let Some(rest) = trimmed.strip_prefix('/') {
        rest.trim_start()
    } else {
        trimmed
    };
    let tag_name_end = tag_name_start
        .find(|ch: char| ch.is_whitespace() || ch == '/')
        .unwrap_or(tag_name_start.len());
    let tag_name = &tag_name_start[..tag_name_end];
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "br" | "p" | "div" | "li" | "tr" | "section" | "article"
    )
}

fn decode_basic_html_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

fn optional_version_value(value: Option<String>) -> ScriptMetaKitResult<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    normalize_version_string(&value)
        .map(Some)
        .ok_or_else(|| ScriptMetaKitError::Parse(format!("invalid version `{value}`")))
}

fn optional_url_value(value: Option<String>) -> ScriptMetaKitResult<Option<Url>> {
    match value {
        Some(value) => normalize_metadata_url(&value)
            .map(Some)
            .ok_or_else(|| ScriptMetaKitError::Parse(format!("invalid URL `{value}`"))),
        None => Ok(None),
    }
}

#[must_use]
pub fn normalize_metadata_url(value: &str) -> Option<Url> {
    let mut value = value.trim();
    loop {
        let trimmed = value
            .trim_start_matches(['"', '\''])
            .trim_end_matches(['"', '\''])
            .trim();
        if trimmed.len() == value.len() {
            break;
        }
        value = trimmed;
    }
    if value.is_empty() {
        return None;
    }

    if let Some(url) = parse_metadata_url(value) {
        return Some(url);
    }

    let repaired = if value.starts_with("https:/") && !value.starts_with("https://") {
        Some(format!("https://{}", &value["https:/".len()..]))
    } else if value.starts_with("http:/") && !value.starts_with("http://") {
        Some(format!("http://{}", &value["http:/".len()..]))
    } else {
        None
    };
    if let Some(repaired) = repaired
        && let Some(url) = parse_metadata_url(&repaired)
    {
        return Some(url);
    }

    if !value.contains("://") {
        return parse_metadata_url(&format!("https://{value}"));
    }

    None
}

fn parse_metadata_url(value: &str) -> Option<Url> {
    let url = Url::parse(value).ok()?;
    match url.scheme() {
        "file" => url.to_file_path().is_ok().then_some(url),
        _ => url.host_str().is_some().then_some(url),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_metadata_url, parse_distribution_metadata_for_script, parse_script_metadata,
    };

    #[test]
    fn normalizes_script_versions_and_urls() {
        let metadata = parse_script_metadata(
            r#"
SCRIPTMETA-BEGIN
Script-ID: com.example.normalized
Version: v1. 2 .3
Meta-URL: "github.com/Yamonov/SCRIPTMETA"
Latest-Version: release 2.0
SCRIPTMETA-END
"#,
        )
        .expect("metadata");

        assert_eq!(metadata.version.as_deref(), Some("1.2.3"));
        assert_eq!(
            metadata.meta_url.as_ref().map(url::Url::as_str),
            Some("https://github.com/Yamonov/SCRIPTMETA")
        );
        assert_eq!(metadata.latest_version.as_deref(), Some("2.0"));
    }

    #[test]
    fn repairs_common_url_forms() {
        assert_eq!(
            normalize_metadata_url("https:/example.com/path").map(|url| url.to_string()),
            Some("https://example.com/path".to_string())
        );
        assert_eq!(
            normalize_metadata_url("'example.com/path'").map(|url| url.to_string()),
            Some("https://example.com/path".to_string())
        );
        assert_eq!(
            normalize_metadata_url("\"example.com/path").map(|url| url.to_string()),
            Some("https://example.com/path".to_string())
        );
        assert_eq!(
            normalize_metadata_url("example.com/path'").map(|url| url.to_string()),
            Some("https://example.com/path".to_string())
        );
        assert!(normalize_metadata_url("https://").is_none());
    }

    #[test]
    fn distribution_version_prefers_version_over_latest_version() {
        let metadata = parse_distribution_metadata_for_script(
            r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.dist
Version: v2. 0
Latest-Version: 9.9
SCRIPTMETA-DIST-END
"#,
            "com.example.dist",
        )
        .expect("distribution metadata");

        assert_eq!(metadata.latest_version.as_deref(), Some("2.0"));
    }

    #[test]
    fn distribution_duplicate_script_id_uses_later_record() {
        let metadata = parse_distribution_metadata_for_script(
            r#"
SCRIPTMETA-DIST-BEGIN
Script-ID: com.example.duplicate
Version: 1.0.0
Script-ID: com.example.other
Version: 3.0.0
Script-ID: com.example.duplicate
Version: 2.0.0
SCRIPTMETA-DIST-END
"#,
            "com.example.duplicate",
        )
        .expect("distribution metadata");

        assert_eq!(metadata.latest_version.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn description_markers_require_canonical_case() {
        let metadata = parse_script_metadata(
            r#"
SCRIPTMETA-BEGIN
Script-ID: com.example.description
Description-BEGIN
first line
description-end
second line
Description-END
SCRIPTMETA-END
"#,
        )
        .expect("metadata");

        assert_eq!(
            metadata.description.as_deref(),
            Some("first line\ndescription-end\nsecond line")
        );
    }

    #[test]
    fn script_metadata_block_markers_are_ascii_case_insensitive() {
        let metadata = parse_script_metadata(
            r#"
scriptmeta-begin
Script-ID: com.example.case
Version: 1.0.0
scriptmeta-end
"#,
        )
        .expect("metadata");

        assert_eq!(metadata.script_id, "com.example.case");
    }
}
