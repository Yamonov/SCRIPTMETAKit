use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{MetadataDiagnostic, ScriptMetaKitError, ScriptMetaKitResult};

pub type CustomMetadataFields = BTreeMap<String, Vec<String>>;

/// Canonical consumer selection. An empty set selects no custom fields.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<String>", into = "Vec<String>")]
pub struct CustomMetadataSelection(Vec<String>);

impl CustomMetadataSelection {
    pub fn new<I, S>(keys: I) -> ScriptMetaKitResult<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut values = Vec::new();
        let mut all = false;
        for key in keys {
            let key = key.as_ref();
            if key.eq_ignore_ascii_case("X-") {
                all = true;
            } else {
                values.push(normalize_custom_metadata_key(key)?);
            }
        }
        if all {
            return Ok(Self(vec!["X-".into()]));
        }
        values.sort_unstable();
        values.dedup();
        Ok(Self(values))
    }

    #[must_use]
    pub fn keys(&self) -> &[String] {
        &self.0
    }

    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.is_all() || self.0.iter().any(|value| value.eq_ignore_ascii_case(key))
    }

    #[must_use]
    pub fn is_all(&self) -> bool {
        self.0.first().is_some_and(|value| value == "X-")
    }

    #[must_use]
    pub fn is_subset_of(&self, previous: &Self) -> bool {
        previous.is_all() || (!self.is_all() && self.0.iter().all(|key| previous.contains(key)))
    }
}

impl TryFrom<Vec<String>> for CustomMetadataSelection {
    type Error = ScriptMetaKitError;

    fn try_from(value: Vec<String>) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<CustomMetadataSelection> for Vec<String> {
    fn from(value: CustomMetadataSelection) -> Self {
        value.0
    }
}

pub(crate) fn has_custom_metadata_prefix(value: &str) -> bool {
    value
        .get(..2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("X-"))
}

fn valid_key(key: &str) -> bool {
    if !has_custom_metadata_prefix(key) {
        return false;
    }
    let Some((creator, field)) = key[2..].split_once('-') else {
        return false;
    };
    creator
        .split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_alphanumeric()))
        && field
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && field
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && field
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

pub fn normalize_custom_metadata_key(key: &str) -> ScriptMetaKitResult<String> {
    if !valid_key(key) || contains_reserved_marker(key) {
        return Err(ScriptMetaKitError::InvalidConfig(format!(
            "invalid custom metadata key `{key}`"
        )));
    }
    Ok(format!("X-{}", key[2..].to_ascii_lowercase()))
}

fn contains_reserved_marker(value: &str) -> bool {
    [
        "SCRIPTMETA-BEGIN",
        "SCRIPTMETA-END",
        "SCRIPTMETA-DIST-BEGIN",
        "SCRIPTMETA-DIST-END",
        "Description-BEGIN",
        "Description-END",
    ]
    .iter()
    .any(|marker| {
        value
            .as_bytes()
            .windows(marker.len())
            .any(|part| part.eq_ignore_ascii_case(marker.as_bytes()))
    })
}

/// Validates the source value before trimming, without copying its contents.
pub(crate) fn custom_metadata_issue(key: &str, value: &str) -> Option<&'static str> {
    if !valid_key(key) {
        Some("invalid_custom_metadata_key")
    } else if value.contains(['\r', '\n']) {
        Some("custom_metadata_line_break")
    } else if contains_reserved_marker(key) || contains_reserved_marker(value) {
        Some("custom_metadata_reserved_marker")
    } else if super::parser::contains_inline_metadata_field_raw(value, false) {
        Some("custom_metadata_legacy_field")
    } else {
        None
    }
}

/// Checks the canonical line that would be written. Quotes remain literal.
#[must_use]
pub fn validate_custom_metadata_field(key: &str, value: &str) -> Vec<MetadataDiagnostic> {
    let issue = if value.contains(['\r', '\n']) {
        Some("custom_metadata_line_break")
    } else {
        custom_metadata_issue(key, value.trim())
    };
    issue.map_or_else(Vec::new, |code| {
        vec![MetadataDiagnostic {
            code: code.into(),
            field: key.into(),
            language: None,
            line: None,
        }]
    })
}
