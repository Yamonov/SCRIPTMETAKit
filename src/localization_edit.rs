//! Lossless Description editing and explicit language operations for one editing session.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::core::{description_label, normalize_language_tag, unescape_description};
use crate::{MetadataEditExtensions, ScriptMetaKitError, ScriptMetaKitResult, ScriptMetadataDraft};

/// Exact editable values. None is absent; empty and whitespace-only strings are retained.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalizedMetadataText {
    pub name: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataLocalizedField {
    Name,
    Author,
    Description,
}

impl LocalizedMetadataText {
    fn field(&self, field: MetadataLocalizedField) -> &Option<String> {
        match field {
            MetadataLocalizedField::Name => &self.name,
            MetadataLocalizedField::Author => &self.author,
            MetadataLocalizedField::Description => &self.description,
        }
    }
    fn field_mut(&mut self, field: MetadataLocalizedField) -> &mut Option<String> {
        match field {
            MetadataLocalizedField::Name => &mut self.name,
            MetadataLocalizedField::Author => &mut self.author,
            MetadataLocalizedField::Description => &mut self.description,
        }
    }
    fn merge_unqualified(&mut self, plain: &Self) -> ScriptMetaKitResult<()> {
        for field in [
            MetadataLocalizedField::Name,
            MetadataLocalizedField::Author,
            MetadataLocalizedField::Description,
        ] {
            if let (Some(existing), Some(value)) = (self.field(field), plain.field(field))
                && existing != value
            {
                return Err(invalid(
                    "unqualified text conflicts with the selected language",
                ));
            }
        }
        for field in [
            MetadataLocalizedField::Name,
            MetadataLocalizedField::Author,
            MetadataLocalizedField::Description,
        ] {
            if self.field(field).is_none() {
                *self.field_mut(field) = plain.field(field).clone();
            }
        }
        Ok(())
    }
}

/// Editable translations, independent of display preferences and OS settings.
/// Original Description text is retained only in this editing model, not in scan results.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetadataLocalizationEditor {
    default_language: Option<String>,
    translations: BTreeMap<String, LocalizedMetadataText>,
    unqualified: LocalizedMetadataText,
    original_description: Option<String>,
    original_default_language: Option<String>,
    original_tags: Vec<String>,
}

/// Operations also available through the C text-service API.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum MetadataLocalizationOperation {
    AddLanguage {
        language: String,
    },
    SetDefaultLanguage {
        language: String,
    },
    ClearDefaultLanguage,
    RemoveLanguage {
        language: String,
    },
    SetText {
        field: MetadataLocalizedField,
        language: Option<String>,
        value: Option<String>,
    },
}

fn invalid(message: &str) -> ScriptMetaKitError {
    ScriptMetaKitError::InvalidConfig(message.into())
}

impl MetadataLocalizationEditor {
    pub fn from_draft(draft: &ScriptMetadataDraft) -> ScriptMetaKitResult<Self> {
        let default_language = draft
            .extensions
            .as_ref()
            .and_then(|value| value.default_language.as_deref())
            .map(normalize_language_tag)
            .transpose()?;
        let mut result = Self {
            default_language: default_language.clone(),
            translations: BTreeMap::new(),
            unqualified: LocalizedMetadataText {
                name: draft.name.clone(),
                author: draft.author.clone(),
                description: None,
            },
            original_description: draft
                .description
                .as_deref()
                .map(|body| crate::core::normalized_newlines(body).into_owned()),
            original_default_language: default_language.clone(),
            original_tags: draft
                .extensions
                .as_ref()
                .map(|value| value.tags.clone())
                .unwrap_or_default(),
        };
        if let Some(language) = &default_language {
            result
                .translations
                .insert(language.clone(), std::mem::take(&mut result.unqualified));
        }
        if let Some(extensions) = &draft.extensions {
            for (field, values) in [
                (MetadataLocalizedField::Name, &extensions.names),
                (MetadataLocalizedField::Author, &extensions.authors),
            ] {
                for (language, value) in values {
                    let target = result
                        .translations
                        .entry(normalize_language_tag(language)?)
                        .or_default()
                        .field_mut(field);
                    match target {
                        Some(previous) if !previous.is_empty() && !value.is_empty() => {
                            previous.push(' ');
                            previous.push_str(value);
                        }
                        Some(previous) => previous.push_str(value),
                        None => *target = Some(value.clone()),
                    }
                }
            }
        }
        if let Some(body) = &result.original_description {
            let document = DescriptionDocument::parse(body, default_language.as_deref());
            if default_language.is_some() {
                for (language, value) in document.values() {
                    result.translations.entry(language).or_default().description = Some(value);
                }
            } else {
                result.unqualified.description = Some(body.clone());
            }
        }
        Ok(result)
    }

    #[must_use]
    pub fn default_language(&self) -> Option<&str> {
        self.default_language.as_deref()
    }

    /// Default first, then canonical language codes in ascending order.
    #[must_use]
    pub fn languages(&self) -> Vec<&str> {
        self.default_language
            .iter()
            .map(String::as_str)
            .chain(
                self.translations
                    .keys()
                    .map(String::as_str)
                    .filter(|language| Some(*language) != self.default_language()),
            )
            .collect()
    }

    /// Exact-language lookup only. No region, default, or OS fallback is applied.
    pub fn text(
        &self,
        field: MetadataLocalizedField,
        language: Option<&str>,
    ) -> ScriptMetaKitResult<Option<&str>> {
        let value = match language {
            Some(language) => self.translations.get(&normalize_language_tag(language)?),
            None => Some(&self.unqualified),
        };
        Ok(value.and_then(|value| value.field(field).as_deref()))
    }

    pub fn add_language(&mut self, language: &str) -> ScriptMetaKitResult<()> {
        self.translations
            .entry(normalize_language_tag(language)?)
            .or_default();
        Ok(())
    }

    pub fn set_text(
        &mut self,
        field: MetadataLocalizedField,
        language: Option<&str>,
        value: Option<String>,
    ) -> ScriptMetaKitResult<()> {
        let target = match language {
            Some(language) => self
                .translations
                .get_mut(&normalize_language_tag(language)?)
                .ok_or_else(|| invalid("add the language before editing its text"))?,
            None if self.default_language.is_none() => &mut self.unqualified,
            None => {
                return Err(invalid(
                    "specify the language when editing localized metadata",
                ));
            }
        };
        *target.field_mut(field) = value;
        Ok(())
    }

    /// Assigning the first default explicitly labels existing unqualified text.
    /// Changing an existing default never reassigns its content to another language.
    pub fn set_default_language(&mut self, language: &str) -> ScriptMetaKitResult<()> {
        let language = normalize_language_tag(language)?;
        if self.default_language.is_none() {
            let mut target = self
                .translations
                .get(&language)
                .cloned()
                .unwrap_or_default();
            target.merge_unqualified(&self.unqualified)?;
            self.translations.insert(language.clone(), target);
            self.unqualified = LocalizedMetadataText::default();
        } else {
            self.translations.entry(language.clone()).or_default();
        }
        self.default_language = Some(language);
        Ok(())
    }

    /// Clears localization only when one language remains. Multiple translations
    /// must be removed explicitly, so this operation cannot silently discard them.
    pub fn clear_default_language(&mut self) -> ScriptMetaKitResult<()> {
        if self.translations.is_empty() {
            self.default_language = None;
            return Ok(());
        }
        if self.translations.len() != 1 {
            return Err(invalid(
                "remove other languages before clearing the default",
            ));
        }
        let language = self
            .translations
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| invalid("missing language"))?;
        self.remove_language(&language)
    }

    /// Removing the last language retains its content as ordinary, unqualified fields.
    /// Removing a default among other languages leaves the default unset until the caller chooses one.
    pub fn remove_language(&mut self, language: &str) -> ScriptMetaKitResult<()> {
        let language = normalize_language_tag(language)?;
        let Some(value) = self.translations.get(&language) else {
            return Ok(());
        };
        if self.translations.len() == 1 {
            let mut plain = self.unqualified.clone();
            plain.merge_unqualified(value)?;
            self.unqualified = plain;
            self.default_language = None;
        } else if self.default_language.as_deref() == Some(&language) {
            self.default_language = None;
        }
        self.translations.remove(&language);
        Ok(())
    }

    pub fn edit(&mut self, operation: MetadataLocalizationOperation) -> ScriptMetaKitResult<()> {
        match operation {
            MetadataLocalizationOperation::AddLanguage { language } => self.add_language(&language),
            MetadataLocalizationOperation::SetDefaultLanguage { language } => {
                self.set_default_language(&language)
            }
            MetadataLocalizationOperation::ClearDefaultLanguage => self.clear_default_language(),
            MetadataLocalizationOperation::RemoveLanguage { language } => {
                self.remove_language(&language)
            }
            MetadataLocalizationOperation::SetText {
                field,
                language,
                value,
            } => self.set_text(field, language.as_deref(), value),
        }
    }

    /// Produces a full save draft while retaining the caller's Tags and common fields.
    /// Save it through the existing conditional write/session API to retain unknown fields and backups.
    pub fn applying_to(
        &self,
        draft: &ScriptMetadataDraft,
    ) -> ScriptMetaKitResult<ScriptMetadataDraft> {
        for language in self.translations.keys() {
            if normalize_language_tag(language)? != *language {
                return Err(invalid("editor language keys must be canonical"));
            }
        }
        let plain = if self.translations.is_empty() {
            if self.default_language.is_some() {
                return Err(invalid("default language is absent from translations"));
            }
            &self.unqualified
        } else {
            let language = self.default_language.as_ref().ok_or_else(|| {
                invalid("Meta-Default-language is required for localized metadata")
            })?;
            if self.unqualified != LocalizedMetadataText::default() {
                return Err(invalid(
                    "assign unqualified text before saving localized metadata",
                ));
            }
            self.translations
                .get(language)
                .ok_or_else(|| invalid("default language is absent from translations"))?
        };
        let mut result = draft.clone();
        result.name = plain.name.clone();
        result.author = plain.author.clone();
        result.description = self.render_description();
        let mut extensions = MetadataEditExtensions {
            default_language: self.default_language.clone(),
            tags: draft
                .extensions
                .as_ref()
                .map(|value| value.tags.clone())
                .unwrap_or_else(|| self.original_tags.clone()),
            ..MetadataEditExtensions::default()
        };
        for (language, value) in &self.translations {
            if Some(language.as_str()) == self.default_language() {
                continue;
            }
            if let Some(name) = &value.name {
                extensions.names.insert(language.clone(), name.clone());
            }
            if let Some(author) = &value.author {
                extensions.authors.insert(language.clone(), author.clone());
            }
            // One empty scalar declaration keeps an otherwise completely empty language alive.
            if value.name.is_none() && value.author.is_none() && value.description.is_none() {
                extensions.names.insert(language.clone(), String::new());
            }
        }
        result.extensions = Some(extensions);
        crate::editkit::validate_script_metadata_draft(&result)?;
        Ok(result)
    }

    fn render_description(&self) -> Option<String> {
        if self.translations.is_empty() {
            return self.unqualified.description.clone();
        }
        let values: BTreeMap<&str, &str> = self
            .translations
            .iter()
            .filter_map(|(language, value)| {
                value
                    .description
                    .as_ref()
                    .map(|body| (language.as_str(), body.as_str()))
            })
            .collect();
        let original = self.original_description.as_deref().map(|body| {
            DescriptionDocument::parse(body, self.original_default_language.as_deref())
        });
        let previous = original
            .as_ref()
            .map(DescriptionDocument::values)
            .unwrap_or_default();
        if self.default_language == self.original_default_language
            && original.is_some()
            && previous.len() == values.len()
            && previous
                .iter()
                .all(|(language, body)| values.get(language.as_str()) == Some(&body.as_str()))
        {
            return self.original_description.clone();
        }
        let mut chunks = Vec::<DescriptionChunk>::new();
        let mut emitted = BTreeSet::new();
        if let Some(original) = &original {
            for part in &original.parts {
                let Some(value) = values.get(part.language.as_str()) else {
                    continue;
                };
                if original.localized
                    && previous.get(&part.language).map(String::as_str) == Some(*value)
                {
                    chunks.push(DescriptionChunk {
                        text: if part.labeled {
                            part.raw.to_owned()
                        } else {
                            format!("[{}]\n{}", part.language, part.raw)
                        },
                        has_separator: part.separator
                            || (part.body.is_empty() && part.raw.ends_with('\n')),
                        trim_last_separator: part.separator,
                    });
                    emitted.insert(part.language.clone());
                } else if emitted.insert(part.language.clone()) {
                    chunks.push(DescriptionChunk::edited(render_section(
                        &part.language,
                        value,
                    )));
                }
            }
        }
        // Newly created sections have deterministic default-first/code ordering.
        for language in self.languages() {
            if let Some(value) = values.get(language)
                && emitted.insert(language.to_owned())
            {
                chunks.push(DescriptionChunk::edited(render_section(language, value)));
            }
        }
        if chunks.is_empty() {
            return None;
        }
        let mut output = String::new();
        for (index, chunk) in chunks.iter().enumerate() {
            let last = index + 1 == chunks.len();
            if last && chunk.trim_last_separator {
                output.push_str(&chunk.text[..chunk.text.len() - 1]);
            } else {
                output.push_str(&chunk.text);
            }
            if !last && !chunk.has_separator {
                output.push('\n');
            }
        }
        Some(output)
    }
}

fn render_section(language: &str, body: &str) -> String {
    let mut escaped = String::with_capacity(body.len());
    for character in body.chars() {
        if matches!(character, '\\' | '[' | ']') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    format!("[{language}]\n{escaped}")
}

struct DescriptionPart<'a> {
    language: String,
    raw: &'a str,
    body: &'a str,
    labeled: bool,
    separator: bool,
}

// A header-only empty section already ends with its boundary newline. Keep
// that separate from a newline belonging to an edited body or an existing body.
struct DescriptionChunk {
    text: String,
    has_separator: bool,
    trim_last_separator: bool,
}
impl DescriptionChunk {
    fn edited(text: String) -> Self {
        Self {
            text,
            has_separator: false,
            trim_last_separator: false,
        }
    }
}

struct DescriptionDocument<'a> {
    parts: Vec<DescriptionPart<'a>>,
    localized: bool,
}

impl<'a> DescriptionDocument<'a> {
    fn parse(source: &'a str, default: Option<&str>) -> Self {
        let Some(default) = default else {
            return Self {
                parts: vec![],
                localized: false,
            };
        };
        let mut labels = Vec::new();
        let mut offset = 0;
        for line in source.split_inclusive('\n') {
            let trimmed = line.trim_start();
            if let Some((language, _)) = description_label(trimmed)
                && let Ok(language) = normalize_language_tag(language)
            {
                let end = trimmed.find(']').unwrap_or(0) + 1;
                let mut body_start = offset + line.len() - trimmed.len() + end;
                if source.as_bytes().get(body_start) == Some(&b'\n') {
                    body_start += 1;
                }
                labels.push((offset, body_start, language));
            }
            offset += line.len();
        }
        let localized = !labels.is_empty();
        let mut parts = Vec::new();
        if labels.first().is_none_or(|label| label.0 > 0) {
            let end = labels.first().map_or(source.len(), |label| label.0);
            let separator = localized && source[..end].ends_with('\n');
            parts.push(DescriptionPart {
                language: default.to_owned(),
                raw: &source[..end],
                body: &source[..end - usize::from(separator)],
                labeled: false,
                separator,
            });
        }
        for (index, (start, body_start, language)) in labels.iter().enumerate() {
            let end = labels.get(index + 1).map_or(source.len(), |label| label.0);
            let separator =
                index + 1 < labels.len() && end > *body_start && source[..end].ends_with('\n');
            parts.push(DescriptionPart {
                language: language.clone(),
                raw: &source[*start..end],
                body: &source[*body_start..end - usize::from(separator)],
                labeled: true,
                separator,
            });
        }
        Self { parts, localized }
    }

    fn values(&self) -> BTreeMap<String, String> {
        let mut values = BTreeMap::<String, String>::new();
        for part in &self.parts {
            let body = if self.localized {
                unescape_description(part.body)
            } else {
                part.body.to_owned()
            };
            let target = values.entry(part.language.clone()).or_default();
            if !target.is_empty() && !body.is_empty() {
                target.push_str("\n\n");
            }
            target.push_str(&body);
        }
        values
    }
}
