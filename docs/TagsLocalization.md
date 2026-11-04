# Tags and metadata localization

For complete validated authoring examples, see [metadata authoring](MetadataAuthoring.md).

SCRIPTMETAKit 2.1.0.
See [field requirements and relationships](MetadataFieldRules.md) for required and optional items.
See [release validation](Release2.0Validation.md) for the current macOS Universal
package and verified architectures.

## Syntax

```text
SCRIPTMETA-BEGIN
Script-ID=com.example.export
Meta-Default-language=ja
Name=画像を書き出す
Name[en]=Export Images
Name[fr]=Exporter les images
Author=作者
Author[en]=Author
Tags=画像,image,
export,
Tags=image,print
Description-BEGIN
[ja]
選択した画像を書き出します。
[en]
Exports the selected images.
\[fr\] is a literal label here.
Description-END
SCRIPTMETA-END
```

Language identifiers are validated and canonicalized BCP 47 tags. Use `ja`, not
`jp`, and hyphens rather than underscores. The default language identifies the
unqualified Name/Author and the default Description section.

Name and Author translations use suffixed keys. Description keeps its original
outer markers and recognizes valid language labels at the beginning of a line.
With a valid `Meta-Default-language`, text before the first valid language label
belongs to that default language; the initial label may be omitted. For example,
with `Meta-Default-language=ja`, `日本語\n[en]\nEnglish` contains Japanese and
English sections. The default field can appear before or after Description.
Without a valid default language, the entire Description remains ordinary text,
including leading language-looking labels and literal backslashes.
A body containing no valid labels always remains plain default text. Ordinary
headings such as `[Note]` are text. Description outer markers retain their
canonical case.

Within localized Description text, only `\[`, `\]` and `\\` are unescaped,
once, after section boundaries have been determined, including the implicit
default section before the first label. Other backslash sequences remain literal.
Description text with no valid language labels is not unescaped.

Repeated valid content for the same language is joined in source order:
Name/Author use a space, Description uses a blank line. Identical content is
joined too; there is no content equality check. Diagnostics report the joining
without requiring a UI alert.

## Tags

Tags are common metadata, independent of display language. Commas separate values;
leading/trailing whitespace is removed and interior whitespace remains. A final
comma permits continuation, but another metadata field or block marker ends that
occurrence. All occurrences in one record are merged in first-appearance order,
with exact string deduplication. Empty values do not erase earlier occurrences.

Item-shaped or block-shaped values are reserved, including `Name=Example`,
`Name[en]=Example`, `client:print`, and `Description-END`. Inline item syntax and
outer SCRIPTMETA marker substrings are also reserved. Tags inside Description are
text. Invalid source tag values are omitted from normal results and diagnosed;
new writes reject them.

## Normal reads: explicit priorities, selected values only

Rust:

```rust
engine.set_preferred_languages(["ja", "en", "fr"])?;
let result = engine.scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))?;
```

Swift:

```swift
try await engine.setPreferredLanguages(["ja", "en", "fr"])
let result = try await engine.scan(folderURLs: [folder], checkUpdates: false)
let item = result.allItems.first
print(item?.name ?? "")
print(item?.presentation.nameLanguage ?? "unknown")
print(item?.tags ?? [])
```

Workspace configuration can set `preferredLanguages` initially, or its
`setPreferredLanguages(_:)` method can replace them. The engine setter returns
whether canonical priorities changed. Preferences survive a volatile engine reset.

Each field tries each supplied language, including its less-specific lookup
candidates, before using the metadata default. Thus an English name and a French
description can be returned together when the caller supplies `ja, en, fr`.
No configured languages means metadata defaults. Missing/empty values fall back;
unavailable Description/Author remains absent. Names in old/incomplete documents
can be displayed via existing filename fallback. Localized saves require
a nonempty unqualified Name and a valid default language; Tags alone require neither.

The consumer owns OS preference acquisition and change detection. The Kit does
not read OS languages, application localizations or environment locale values.
After changing priorities, request a new scan; old language projections are not
reused. Cache schema 4 includes the canonical language order and projection
version and metadata-read settings. Projection version 6 also records custom-field selection. Projection version 5 makes Description
without a valid default language ordinary text. Projection version 4 included the parser,
file-filter and inspection settings, in addition to correct partial-file decoding
and recognition of an implicit default Description section.
Old/context-mismatched metadata caches require a source rescan.
Schema 4 stores the authoritative candidate records without redundant derived
catalog item lists; compatible schema 3 caches remain readable.

`ScriptMetaItem.presentation` / Rust `MetadataPresentation` contains Tags,
actual selected languages and nonfatal diagnostics, without unselected text.
`ScanResult.preferred_languages` / Swift `preferredLanguages` identifies the
instruction used for the result.

## Editing

Use a fresh full editing draft/session, not a normal selected item.

```swift
let read = try await engine.readScriptMetadataDraft(fileURL: file)
var draft = read.draft
var metadata = draft.extensions ?? MetadataEditExtensions(defaultLanguage: "ja")
metadata.names["en"] = "Updated English name"
draft.extensions = metadata
try await engine.writeScriptMetadata(
    fileURL: file, draft: draft,
    expectedSourceFingerprint: read.sourceFingerprint
)
```

The draft's Description is the full raw body with every language and escape.
Extensions carry all localized names/authors and Tags. A nil extensions value
preserves extensions found in the original for legacy callers. An explicit
extensions value is authoritative; an empty Tags array clears all Tags fields.
Unrelated edits preserve tag fragments and every Description language.

Localized writes require Name and Meta-Default-language; Tags and legacy-only
writes retain their independent requirements. Empty localized Name/Author fields
are retained as language declarations but are not selected for display. Invalid new language identifiers, reserved
Tags, line breaks in scalar names/authors, and ending-marker injection are
rejected. Unclosed/repeated outer Description blocks require repair before an
edit, avoiding silent loss of their contents. Conditional writes reject external
source changes.

See [multilingual editing API](LocalizationEditingAPI.md) for safe default-language
changes, Description section editing, empty languages and returning the last
language to ordinary fields.

## C ABI

Existing SmkScriptItem and SmkScriptMetadataDraft layouts are unchanged.

- `smk_engine_set_preferred_languages` accepts an ordered UTF-8 slice array and
  reports whether the canonical priorities changed.
- `smk_scan_result_preferred_languages` returns the priorities used by a result.
- `smk_scan_result_metadata_presentations` returns parallel arrays for items,
  file items, file entries or candidate records. Its ranges address the same
  result's metadata tags and diagnostics arrays.
- `smk_edit_result_metadata_extensions` exposes full scalar translations/Tags.
- `smk_engine_write_script_metadata_file_if_unchanged_v2` accepts those extensions.

All result slices borrow the owning result until its existing free function is
called. See the C header for exact signatures and collection identifiers.

## Native test workflow

```sh
cargo test --workspace --all-features --locked --target aarch64-apple-darwin
cargo clippy --workspace --all-targets --all-features --locked --target aarch64-apple-darwin -- -D warnings
python3 script/build_arm64_test_package.py
```

The builder prints an isolated Swift package path with arm64-only dylib, headers,
SDK sources, tests and a test manifest. It leaves the released Universal artifacts
unchanged. Build/test this package on Apple Silicon. No application automation is
required by the localization tests.
