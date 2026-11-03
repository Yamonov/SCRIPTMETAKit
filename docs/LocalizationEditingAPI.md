# Multilingual editing API (2.0.1)

These additive APIs edit Name, Author and Description without applying display
fallbacks. Existing C structures and the `extensions = nil` preservation contract
remain compatible. Tags are optional, independent common metadata: they do not
require Name or Meta-Default-language.

## Swift

```swift
let session = try await engine.beginScriptMetadataEditSession(fileURL: file)
var editor = try ScriptMetaKitMetadata.localizationEditor(from: session.readResult)

try editor.addLanguage("fr")
try editor.setText("Nom français", for: .name, language: "fr")
try editor.setText("Description française", for: .description, language: "fr")
try editor.setDefaultLanguage("en")

var draft = session.draft
draft.version = "1.1"
draft = try editor.applying(to: draft)
try await engine.commitScriptMetadataEditSession(
    session, draft: draft, backupRootURL: backups
)
```

`languages` lists the default first and other canonical BCP 47 codes in ascending
order. `translations` contains `LocalizedMetadataText` values; `unqualified`
contains ordinary text whose language has not been specified. Reading a legacy
document never inserts `ja` or obtains an OS locale.

`text(for:language:)` is an exact-language lookup. If only `fr` exists, looking up
`fr-FR` returns nil. Missing, empty and whitespace-only values are not replaced
with another translation. `setText` edits an already-added language; omit its
language argument only when editing unqualified ordinary fields. These are value
types; setting text does not parse/rewrite the Description or serialize the full
document on each keystroke. Syntax processing happens in the shared Rust core
when the editor is opened or converted to a save draft.

`setDefaultLanguage` keeps every language's content associated with that language.
For a legacy document, the first explicitly chosen default labels its existing
ordinary text. If conflicting translated and unqualified values already exist,
the operation fails without dropping either. Switching an established default
uses the new language's actual values, including missing values. An empty default
Name must be filled before saving localized metadata; other languages are never
used to fill that editing field automatically.

`removeLanguage` removes that language's Name, Author and Description. If it was
the default and others remain, `defaultLanguage` becomes nil: explicitly choose a
new default before saving. No replacement is guessed. Removing the last language
instead moves its content to `unqualified`, clears all language qualifiers and
removes Meta-Default-language. `clearDefaultLanguage` performs the same conversion
when only one language remains; with several languages it fails to prevent
silent loss of the other translations.

```swift
try editor.removeLanguage("en")
// If ja is now the last language:
try editor.removeLanguage("ja")
let ordinaryDraft = try editor.applying(to: session.draft)
```

`applying(to:)` owns the three localized fields and preserves the supplied draft's
common fields and Tags. If its extensions are nil, the editor retains the Tags
from its original draft; an explicit Tags array is authoritative. The resulting
extensions are explicit, so a language removal cannot be mistaken for the legacy
nil-means-preserve request. Save through the existing session to retain unknown
fields from the original file, detect external changes and create a backup.

## Description and empty languages

Description parsing requires a valid Meta-Default-language. Without it, the whole
Description is ordinary text, even if it begins with `[en]` or contains literal
backslashes. This lets the last language return to the old unqualified form
without inventing another metadata key. Valid localized documents keep their
existing display matching and fallback rules. Projection version 5 invalidates
older cached presentations whose interpretation may differ.

The editor retains the original Description separately from editable values for
this session only. Unedited sections preserve indentation, blank lines, escapes,
inline labels and repeated same-language sections. Editing a language replaces
its sections together; other sections retain their raw content. The Kit escapes
literal brackets and backslashes in newly edited localized text and unescapes
them exactly once when returning to ordinary text. No-change edits leave the raw
Description untouched. Line endings are restored by the existing file writer.

An added language with all fields absent is saved as an empty `Name[fr]=`
declaration. Empty Name/Author declarations and empty Description sections survive
editing reads, while normal display still ignores empty translations. The editor
has no UI-specific 10-language or 40-candidate limit. There is no configurable
translation-order API; new sections use deterministic default-first/code order,
and existing unedited Description sections retain their source order.

## Rust and C

Rust exposes `MetadataLocalizationEditor::from_draft`, `text`, `add_language`,
`set_text`, `set_default_language`, `remove_language`, `clear_default_language`
and `applying_to`. The same structural operations are available as a serializable
`MetadataLocalizationOperation` enum.

The C ABI adds three owned JSON text services:

- `smk_metadata_localization_read(draft_json, out_result)` creates an editor.
- `smk_metadata_localization_edit(editor_json, operation_json, out_result)` returns
  a changed editor. A failed operation leaves the caller's editor untouched.
- `smk_metadata_localization_apply(editor_json, draft_json, out_result)` returns a
  validated save draft, suitable for the existing conditional file write API.

Operation examples:

```json
{"operation":"add_language","language":"fr"}
{"operation":"set_text","field":"description","language":"fr","value":"Texte"}
{"operation":"set_default_language","language":"en"}
{"operation":"remove_language","language":"fr"}
{"operation":"clear_default_language"}
```

Read result text with `smk_edit_result_text` and release every returned handle with
`smk_edit_result_free`, including error results. None of these three services
performs file I/O. Source fingerprints, encoding preservation, backup creation
and atomic file replacement stay in the existing save implementation. The
editing model is never added to the resident catalog or normal scan results.

Field requirements and relationships: [メタ項目の必須条件と関係](MetadataFieldRules.md).
