//! Explicit language preferences and selected metadata. This module never reads OS locale settings.
use std::collections::{BTreeMap, BTreeSet};

use language_tags::LanguageTag;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use super::{ScriptMetaKitError, ScriptMetaKitResult, normalized_newlines};

/// Ordered, validated BCP 47 preferences supplied by the consumer.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct LanguagePreferences(Vec<String>);

impl LanguagePreferences {
    pub fn new<I, S>(languages: I) -> ScriptMetaKitResult<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut result = Vec::new();
        for language in languages {
            let canonical = normalize_language_tag(language.as_ref().trim())?;
            if !result.contains(&canonical) {
                result.push(canonical);
            }
        }
        Ok(Self(result))
    }

    #[must_use]
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'de> Deserialize<'de> for LanguagePreferences {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = Vec::<String>::deserialize(deserializer)?;
        Self::new(values).map_err(D::Error::custom)
    }
}

/// Validates registered subtags and canonicalizes aliases/case using BCP 47.
pub fn normalize_language_tag(value: &str) -> ScriptMetaKitResult<String> {
    let tag = LanguageTag::parse(value).map_err(|error| {
        ScriptMetaKitError::InvalidConfig(format!("invalid language tag `{value}`: {error}"))
    })?;
    tag.validate().map_err(|error| {
        ScriptMetaKitError::InvalidConfig(format!("invalid language tag `{value}`: {error}"))
    })?;
    tag.canonicalize()
        .map(LanguageTag::into_string)
        .map_err(|error| {
            ScriptMetaKitError::InvalidConfig(format!(
                "cannot canonicalize language tag `{value}`: {error}"
            ))
        })
}

/// A nonfatal source issue. No translation body is retained in diagnostics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MetadataDiagnostic {
    pub code: String,
    pub field: String,
    pub language: Option<String>,
    /// One-based line within the metadata block, when available.
    pub line: Option<usize>,
}

/// Common metadata and selection provenance. Contains no unselected translation map.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MetadataPresentation {
    pub default_language: Option<String>,
    pub name_language: Option<String>,
    pub author_language: Option<String>,
    pub description_language: Option<String>,
    pub tags: Vec<String>,
    pub diagnostics: Vec<MetadataDiagnostic>,
}

/// Full scalar translations for an editing session. Description remains the full
/// raw multilingual body in ScriptMetadataDraft.description, including escapes.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MetadataEditExtensions {
    pub default_language: Option<String>,
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    #[serde(default)]
    pub authors: BTreeMap<String, String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub(crate) fn diagnostic(
    diagnostics: &mut Vec<MetadataDiagnostic>,
    code: &str,
    field: &str,
    language: Option<&str>,
    line: Option<usize>,
) {
    // Keep diagnostics bounded independently of the amount of malformed input.
    const LIMIT: usize = 64;
    if diagnostics.len() < LIMIT {
        diagnostics.push(MetadataDiagnostic {
            code: code.into(),
            field: field.into(),
            language: language.map(str::to_owned),
            line,
        });
    } else if let Some(last) = diagnostics.last_mut() {
        *last = MetadataDiagnostic {
            code: "diagnostics_truncated".into(),
            field: String::new(),
            language: None,
            line: None,
        };
    }
}

pub(crate) fn append_value(target: &mut String, value: &str, separator: &str) {
    if value.trim().is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push_str(separator);
    }
    target.push_str(value);
}

pub(crate) fn lookup_candidates(language: &str) -> impl Iterator<Item = &str> {
    std::iter::successors((!language.is_empty()).then_some(language), |current| {
        let (mut parent, _) = current.rsplit_once('-')?;
        if parent
            .rsplit('-')
            .next()
            .is_some_and(|part| part.len() == 1)
        {
            parent = parent.rsplit_once('-')?.0;
        }
        (!parent.is_empty()).then_some(parent)
    })
}

pub(crate) fn select_text(
    default: Option<&str>,
    values: &BTreeMap<String, String>,
    default_language: Option<&str>,
    preferences: &LanguagePreferences,
) -> (Option<String>, Option<String>) {
    for preference in preferences.as_slice() {
        for candidate in lookup_candidates(preference) {
            if let Some(value) = values
                .get(candidate)
                .filter(|value| !value.trim().is_empty())
            {
                return (Some(value.clone()), Some(candidate.to_owned()));
            }
            if Some(candidate) == default_language
                && let Some(value) = default.filter(|value| !value.trim().is_empty())
            {
                return (Some(value.to_owned()), default_language.map(str::to_owned));
            }
        }
    }
    if let Some(value) = default.filter(|value| !value.trim().is_empty()) {
        return (Some(value.to_owned()), default_language.map(str::to_owned));
    }
    if let Some(language) = default_language {
        for candidate in lookup_candidates(language) {
            if let Some(value) = values
                .get(candidate)
                .filter(|value| !value.trim().is_empty())
            {
                return (Some(value.clone()), Some(candidate.to_owned()));
            }
        }
    }
    (None, None)
}

pub(crate) fn unescape_description(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\\'
            && chars
                .peek()
                .is_some_and(|next| matches!(next, '[' | ']' | '\\'))
        {
            if let Some(next) = chars.next() {
                result.push(next);
            }
        } else {
            result.push(character);
        }
    }
    result
}

/// Returns a lexical candidate. Only validated BCP 47 tags become boundaries;
/// ordinary headings such as [Note] must remain description text.
pub(crate) fn description_label(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('[')?;
    let end = rest.find(']')?;
    let language = &rest[..end];
    if !language
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    Some((language, &rest[end + 1..]))
}

pub(crate) fn description_starts_with_language_label(body: &str) -> bool {
    body.split(['\r', '\n'])
        .find(|line| !line.trim().is_empty())
        .and_then(|line| description_label(line.trim_start()))
        .is_some_and(|(language, _)| normalize_language_tag(language).is_ok())
}

pub(crate) fn description_translations(
    body: &str,
    default_language: Option<&str>,
    diagnostics: &mut Vec<MetadataDiagnostic>,
) -> Option<BTreeMap<String, String>> {
    let normalized = normalized_newlines(body);
    let default_language = default_language.and_then(|value| normalize_language_tag(value).ok());
    let starts_with_label = description_starts_with_language_label(&normalized);
    if !starts_with_label
        && (default_language.is_none()
            || !normalized.lines().any(|line| {
                description_label(line.trim_start())
                    .is_some_and(|(language, _)| normalize_language_tag(language).is_ok())
            }))
    {
        // No usable default means an unlabeled preamble is legacy text. A body
        // with no actual labels also stays literal, including its backslashes.
        return None;
    }
    let mut result = BTreeMap::<String, String>::new();
    let mut current = default_language;
    let mut lines = Vec::new();
    let flush = |current: &Option<String>,
                 lines: &mut Vec<&str>,
                 result: &mut BTreeMap<String, String>,
                 diagnostics: &mut Vec<MetadataDiagnostic>| {
        if let Some(language) = current {
            let body = unescape_description(lines.join("\n").trim_matches('\n'));
            if !body.trim().is_empty() {
                let target = result.entry(language.clone()).or_default();
                if !target.is_empty() {
                    diagnostic(
                        diagnostics,
                        "repeated_language_joined",
                        "Description",
                        Some(language),
                        None,
                    );
                }
                append_value(target, &body, "\n\n");
            }
        }
        lines.clear();
    };
    for line in normalized.lines() {
        if let Some((raw_language, value)) = description_label(line.trim_start())
            && let Ok(language) = normalize_language_tag(raw_language)
        {
            flush(&current, &mut lines, &mut result, diagnostics);
            current = Some(language);
            if !value.is_empty() {
                lines.push(value);
            }
        } else {
            lines.push(line);
        }
    }
    flush(&current, &mut lines, &mut result, diagnostics);
    Some(result)
}

pub(crate) fn item_separator(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    let mut index = 1;
    while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'-') {
        index += 1;
    }
    if bytes.get(index) == Some(&b'[') {
        let end = value[index + 1..].find(']')? + index + 1;
        if end == index + 1 {
            return None;
        }
        index = end + 1;
    }
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    bytes
        .get(index)
        .filter(|byte| matches!(byte, b'=' | b':'))
        .map(|_| index)
}

pub(crate) fn is_block_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let key = lower
        .strip_suffix("-begin")
        .or_else(|| lower.strip_suffix("-end"));
    key.is_some_and(|key| item_separator(&format!("{key}=")).is_some())
}

#[must_use]
pub fn is_valid_metadata_tag(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.contains([',', '\r', '\n']) {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    if [
        "scriptmeta-begin",
        "scriptmeta-end",
        "scriptmeta-dist-begin",
        "scriptmeta-dist-end",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || is_block_marker(value)
    {
        return false;
    }
    !value.char_indices().any(|(index, _)| {
        (index == 0
            || value[..index]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace))
            && item_separator(&value[index..]).is_some()
    })
}

/// Trims and merges Tags in first-appearance order, rejecting reserved values.
pub fn normalize_metadata_tags<I, S>(values: I) -> ScriptMetaKitResult<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for value in values {
        let value = value.as_ref().trim();
        if value.is_empty() {
            continue;
        }
        if !is_valid_metadata_tag(value) {
            return Err(ScriptMetaKitError::InvalidConfig(format!(
                "reserved or invalid Tags value: {value}"
            )));
        }
        if seen.insert(value.to_owned()) {
            result.push(value.to_owned());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_validate_canonicalize_and_keep_priority() {
        let values = LanguagePreferences::new(["ja", "EN-us", "fr", "en-US"]).unwrap();
        assert_eq!(values.as_slice(), &["ja", "en-US", "fr"]);
        assert!(LanguagePreferences::new(["jp"]).is_err());
        assert!(LanguagePreferences::new(["ja_jp"]).is_err());
    }

    #[test]
    fn preferences_try_english_then_french_before_default() {
        let values = BTreeMap::from([("fr".into(), "Français".into())]);
        assert_eq!(
            select_text(
                Some("Deutsch"),
                &values,
                Some("de"),
                &LanguagePreferences::new(["ja", "en", "fr"]).unwrap()
            ),
            (Some("Français".into()), Some("fr".into()))
        );
    }

    #[test]
    fn preference_fallback_removes_extensions_and_tries_the_script_before_base_language() {
        let values = BTreeMap::from([
            ("zh-Hant".into(), "Traditional".into()),
            ("zh".into(), "Base".into()),
        ]);
        let selected = select_text(
            Some("Default"),
            &values,
            Some("de"),
            &LanguagePreferences::new(["zh-Hant-TW-u-co-stroke", "en"]).unwrap(),
        );
        assert_eq!(
            selected,
            (Some("Traditional".into()), Some("zh-Hant".into()))
        );
    }

    #[test]
    fn repeated_description_is_joined_even_when_identical() {
        let mut diagnostics = Vec::new();
        let result =
            description_translations("[en]same\n[EN]\nsame", None, &mut diagnostics).unwrap();
        assert_eq!(result["en"], "same\n\nsame");
    }

    #[test]
    fn escaped_label_and_unknown_backslash_sequences_remain_text() {
        let result = description_translations(
            "[ja]\n\\[en\\] C:\\temp\\new\n[en]English",
            None,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(result["ja"], "[en] C:\\temp\\new");
    }

    #[test]
    fn nonlanguage_brackets_remain_description_content() {
        let result = description_translations(
            "[ja]日本語\n[jp]invalid\n[en]English",
            None,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(result["ja"], "日本語\n[jp]invalid");
        assert!(
            description_translations("[Note]\nA legacy heading", None, &mut Vec::new()).is_none()
        );
    }

    #[test]
    fn tags_are_exact_deduplicated_and_reserved_syntax_is_rejected() {
        assert_eq!(
            normalize_metadata_tags([" 画像 ", "image", "画像", "Image"]).unwrap(),
            ["画像", "image", "Image"]
        );
        for value in [
            "Name[en]=Example",
            "a Name=Example",
            "Description-END",
            "client:print",
            "a,b",
        ] {
            assert!(!is_valid_metadata_tag(value), "{value}");
        }
        assert!(is_valid_metadata_tag("色:印刷"));
        assert!(is_valid_metadata_tag("#print"));
    }
}
