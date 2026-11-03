//! Scoped parsing of text-bearing fields. Tags and Description are protected
//! before the legacy parser processes compact metadata fields.
use std::collections::{BTreeMap, BTreeSet};

use super::{
    LanguagePreferences, MetadataDiagnostic, MetadataEditExtensions, MetadataPresentation,
    append_value, description_starts_with_language_label, description_translations, diagnostic,
    is_block_marker, is_valid_metadata_tag, item_separator, normalize_language_tag,
    normalized_newlines, select_text,
};

#[derive(Clone, Debug)]
struct ScalarValue {
    language: Option<String>,
    value: String,
    line: usize,
}

#[derive(Default)]
pub(crate) struct MetadataSections {
    pub legacy_body: String,
    pub raw_description: Option<String>,
    pub tag_lines: Vec<String>,
    pub opaque_blocks: Vec<String>,
    pub has_extensions: bool,
    default_language: Option<String>,
    names: Vec<ScalarValue>,
    authors: Vec<ScalarValue>,
    tags: Vec<String>,
    diagnostics: Vec<MetadataDiagnostic>,
}

pub(crate) fn infer_metadata_line_prefix(body: &str) -> Option<&'static str> {
    for raw in body.lines() {
        let line = raw.trim();
        if item_separator(line).is_some() || line.eq_ignore_ascii_case("Description-BEGIN") {
            return None;
        }
        for prefix in ["//", "--", "#", ";", "*"] {
            if let Some(rest) = line.strip_prefix(prefix)
                && (item_separator(rest.trim_start()).is_some()
                    || rest.trim().eq_ignore_ascii_case("Description-BEGIN"))
            {
                return Some(prefix);
            }
        }
    }
    None
}

pub(crate) fn strip_metadata_line_prefix<'a>(line: &'a str, prefix: Option<&str>) -> &'a str {
    if let Some(prefix) = prefix
        && let Some(rest) = line.trim_start().strip_prefix(prefix)
    {
        return rest.strip_prefix(' ').unwrap_or(rest);
    }
    line
}

fn localized_key<'a>(key: &'a str, base: &str) -> Option<&'a str> {
    let prefix = key.get(..base.len())?;
    if !prefix.eq_ignore_ascii_case(base) {
        return None;
    }
    key[base.len()..].strip_prefix('[')?.strip_suffix(']')
}

fn description_end_marker(line: &str) -> Option<String> {
    let suffix = line.get(line.len().checked_sub(6)?..)?;
    if suffix != "-BEGIN" {
        return None;
    }
    let key = &line[..line.len() - 6];
    (key == "Description" || localized_key(key, "Description").is_some())
        .then(|| format!("{key}-END"))
}

pub(crate) fn metadata_sections(body: &str) -> MetadataSections {
    let normalized = normalized_newlines(body);
    let prefix = infer_metadata_line_prefix(&normalized);
    let mut result = MetadataSections::default();
    let mut legacy = Vec::<(&str, bool)>::new();
    let mut in_tags = false;
    let mut description_end: Option<String> = None;
    let mut description_lines = Vec::<&str>::new();
    let mut ignored_description = false;
    let mut seen_tags = BTreeSet::new();

    for (index, raw_line) in normalized.lines().enumerate() {
        let raw = strip_metadata_line_prefix(raw_line, prefix);
        let line = raw.trim();
        if let Some(end) = &description_end {
            if (end == "Description-END" && line == end)
                || (end != "Description-END" && line.eq_ignore_ascii_case(end))
            {
                if !ignored_description {
                    if result.raw_description.is_some() {
                        diagnostic(
                            &mut result.diagnostics,
                            "repeated_description_block",
                            "Description",
                            None,
                            Some(index + 1),
                        );
                    }
                    result.raw_description = Some(description_lines.join("\n"));
                } else {
                    description_lines.push(raw);
                    result.opaque_blocks.push(description_lines.join("\n"));
                }
                description_lines.clear();
                description_end = None;
            } else {
                description_lines.push(raw);
            }
            continue;
        }
        if in_tags {
            if line.is_empty() {
                result.tag_lines.push(raw.to_owned());
                continue;
            }
            if item_separator(line).is_none() && !is_block_marker(line) {
                result.tag_lines.push(raw.to_owned());
                collect_tags(line, &mut result.tags, &mut seen_tags);
                in_tags = line.ends_with(',');
                continue;
            }
            in_tags = false;
        }
        if let Some(end) = description_end_marker(line) {
            ignored_description = !line.eq_ignore_ascii_case("Description-BEGIN");
            if ignored_description {
                diagnostic(
                    &mut result.diagnostics,
                    "unsupported_description_block",
                    "Description",
                    None,
                    Some(index + 1),
                );
                description_lines.push(raw);
            }
            description_end = Some(end);
            continue;
        }
        let Some(separator) = item_separator(line) else {
            legacy.push((raw, false));
            continue;
        };
        let key = line[..separator].trim();
        let value = line[separator + 1..].trim();
        if key.eq_ignore_ascii_case("Tags") {
            result.has_extensions = true;
            result.tag_lines.push(raw.to_owned());
            collect_tags(value, &mut result.tags, &mut seen_tags);
            in_tags = value.ends_with(',');
        } else if key.eq_ignore_ascii_case("Meta-Default-language") {
            result.has_extensions = true;
            if !value.is_empty() {
                if result.default_language.is_some() {
                    diagnostic(
                        &mut result.diagnostics,
                        "repeated_default_language",
                        key,
                        None,
                        Some(index + 1),
                    );
                } else {
                    result.default_language = Some(value.to_owned());
                }
            }
        } else if key.eq_ignore_ascii_case("Description") {
            result.raw_description = Some(value.to_owned());
        } else if key.eq_ignore_ascii_case("Name") || key.eq_ignore_ascii_case("Author") {
            let entries = if key.eq_ignore_ascii_case("Name") {
                &mut result.names
            } else {
                &mut result.authors
            };
            entries.push(ScalarValue {
                language: None,
                value: value.to_owned(),
                line: index + 1,
            });
            // Legacy compact fields remain available only in legacy documents.
            legacy.push((raw, true));
        } else if let Some(language) = localized_key(key, "Name") {
            result.has_extensions = true;
            result.names.push(ScalarValue {
                language: Some(language.to_owned()),
                value: value.to_owned(),
                line: index + 1,
            });
        } else if let Some(language) = localized_key(key, "Author") {
            result.has_extensions = true;
            result.authors.push(ScalarValue {
                language: Some(language.to_owned()),
                value: value.to_owned(),
                line: index + 1,
            });
        } else {
            legacy.push((raw, false));
        }
    }
    if description_end.is_some() {
        diagnostic(
            &mut result.diagnostics,
            "unclosed_description",
            "Description",
            None,
            None,
        );
    }
    if !result.has_extensions
        && result
            .raw_description
            .as_deref()
            .is_some_and(description_starts_with_language_label)
    {
        result.has_extensions = true;
    }
    result.legacy_body = legacy
        .into_iter()
        .filter_map(|(line, scalar)| (!scalar || !result.has_extensions).then_some(line))
        .collect::<Vec<_>>()
        .join("\n");
    result
}

fn collect_tags(value: &str, tags: &mut Vec<String>, seen: &mut BTreeSet<String>) {
    for value in value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if seen.insert(value.to_owned()) {
            tags.push(value.to_owned());
        }
    }
}

fn scalar_values(
    entries: &[ScalarValue],
    legacy_default: Option<&str>,
    default_language: Option<&str>,
    field: &str,
    diagnostics: &mut Vec<MetadataDiagnostic>,
    keep_invalid: bool,
) -> (Option<String>, BTreeMap<String, String>) {
    let mut values = BTreeMap::<String, String>::new();
    let mut plain = String::new();
    let mut has_plain = false;
    if entries.iter().all(|entry| entry.language.is_some())
        && let Some(value) = legacy_default.filter(|value| !value.trim().is_empty())
    {
        has_plain = true;
        if let Some(language) = default_language {
            values.insert(language.to_owned(), value.to_owned());
        } else {
            plain.push_str(value);
        }
    }
    for entry in entries
        .iter()
        .filter(|entry| !entry.value.trim().is_empty())
    {
        let language = match &entry.language {
            Some(raw) => match normalize_language_tag(raw) {
                Ok(language) => Some(language),
                Err(_) => {
                    diagnostic(
                        diagnostics,
                        "invalid_language",
                        field,
                        Some(raw),
                        Some(entry.line),
                    );
                    if keep_invalid {
                        Some(raw.clone())
                    } else {
                        continue;
                    }
                }
            },
            None => {
                has_plain = true;
                default_language.map(str::to_owned)
            }
        };
        let target = if let Some(language) = language.as_deref() {
            values.entry(language.to_owned()).or_default()
        } else {
            &mut plain
        };
        if !target.is_empty() {
            diagnostic(
                diagnostics,
                "repeated_language_joined",
                field,
                language.as_deref(),
                Some(entry.line),
            );
        }
        append_value(target, &entry.value, " ");
    }
    let default = if has_plain {
        if let Some(language) = default_language {
            values.remove(language)
        } else {
            (!plain.is_empty()).then_some(plain)
        }
    } else {
        None
    };
    (default, values)
}

impl MetadataSections {
    pub fn is_structurally_complete(&self) -> bool {
        !self.diagnostics.iter().any(|issue| {
            matches!(
                issue.code.as_str(),
                "unclosed_description" | "repeated_description_block"
            )
        })
    }

    pub fn editing_extensions(
        &self,
        legacy_name: Option<&str>,
        legacy_author: Option<&str>,
    ) -> (
        Option<String>,
        Option<String>,
        Option<MetadataEditExtensions>,
    ) {
        if !self.has_extensions {
            return (
                legacy_name.map(str::to_owned),
                legacy_author.map(str::to_owned),
                None,
            );
        }
        let default_language = self
            .default_language
            .as_deref()
            .and_then(|value| normalize_language_tag(value).ok());
        let (name, names) = scalar_values(
            &self.names,
            legacy_name,
            default_language.as_deref(),
            "Name",
            &mut Vec::new(),
            true,
        );
        let (author, authors) = scalar_values(
            &self.authors,
            legacy_author,
            default_language.as_deref(),
            "Author",
            &mut Vec::new(),
            true,
        );
        (
            name,
            author,
            Some(MetadataEditExtensions {
                default_language: default_language.or_else(|| self.default_language.clone()),
                names,
                authors,
                tags: self.tags.clone(),
            }),
        )
    }

    pub fn project(
        &self,
        legacy_name: Option<&str>,
        legacy_author: Option<&str>,
        legacy_description: Option<&str>,
        preferences: &LanguagePreferences,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        MetadataPresentation,
    ) {
        let mut presentation = MetadataPresentation {
            diagnostics: self.diagnostics.clone(),
            ..MetadataPresentation::default()
        };
        if let Some(raw) = &self.default_language {
            match normalize_language_tag(raw) {
                Ok(value) => presentation.default_language = Some(value),
                Err(_) => diagnostic(
                    &mut presentation.diagnostics,
                    "invalid_language",
                    "Meta-Default-language",
                    Some(raw),
                    None,
                ),
            }
        } else if self.has_extensions {
            diagnostic(
                &mut presentation.diagnostics,
                "missing_default_language",
                "Meta-Default-language",
                None,
                None,
            );
        }
        for value in &self.tags {
            if is_valid_metadata_tag(value) {
                presentation.tags.push(value.clone());
            } else {
                diagnostic(
                    &mut presentation.diagnostics,
                    "invalid_tag",
                    "Tags",
                    None,
                    None,
                );
            }
        }
        let default_language = presentation.default_language.as_deref();
        let (name_default, names) = if self.has_extensions {
            scalar_values(
                &self.names,
                legacy_name,
                default_language,
                "Name",
                &mut presentation.diagnostics,
                false,
            )
        } else {
            (legacy_name.map(str::to_owned), BTreeMap::new())
        };
        let (author_default, authors) = if self.has_extensions {
            scalar_values(
                &self.authors,
                legacy_author,
                default_language,
                "Author",
                &mut presentation.diagnostics,
                false,
            )
        } else {
            (legacy_author.map(str::to_owned), BTreeMap::new())
        };
        if self.has_extensions
            && name_default
                .as_deref()
                .is_none_or(|name| name.trim().is_empty())
        {
            diagnostic(
                &mut presentation.diagnostics,
                "missing_name",
                "Name",
                None,
                None,
            );
        }
        let (name, name_language) = select_text(
            name_default.as_deref(),
            &names,
            default_language,
            preferences,
        );
        let (author, author_language) = select_text(
            author_default.as_deref(),
            &authors,
            default_language,
            preferences,
        );
        let raw_description = self.raw_description.as_deref().or(legacy_description);
        let (description, description_language) = if let Some(raw) = raw_description {
            if let Some(values) =
                description_translations(raw, default_language, &mut presentation.diagnostics)
            {
                let selected = select_text(None, &values, default_language, preferences);
                if selected.0.is_none() && !values.is_empty() {
                    diagnostic(
                        &mut presentation.diagnostics,
                        "description_unavailable",
                        "Description",
                        None,
                        None,
                    );
                }
                selected
            } else {
                select_text(Some(raw), &BTreeMap::new(), default_language, preferences)
            }
        } else {
            (None, None)
        };
        presentation.name_language = name_language;
        presentation.author_language = author_language;
        presentation.description_language = description_language;
        (name, author, description, presentation)
    }
}
