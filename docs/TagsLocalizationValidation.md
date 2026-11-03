# Tags/localization validation — Apple Silicon

Date: 2026-11-03. Branch: `codex/tags-localization`.

This is a development build based on 1.3.3, not a published release.

## Results

| Check | Result |
|---|---|
| Rust/C ABI workspace, all features, `aarch64-apple-darwin` | 333 tests passed |
| Swift SDK, native arm64, isolated local XCFramework | 70 tests passed, no skips |
| C client compiled against the generated public header | Passed |
| Rust formatting and Clippy with warnings denied | Passed |
| Generated dependency acknowledgements | Current |
| Dylib architecture and ad-hoc code signature | arm64 only; signature verified |

Rust totals include 117 core unit tests, 13 directory-reference tests, 116 package
tests, 9 memory/cache tests, 5 partial-decoding tests, 8 reference-watch tests, 23 feature tests,
9 FFI unit tests, 31 existing FFI smoke tests, and 2 new FFI localization tests.
Windows-only tests were not run. The subsequent legacy-corpus decoding fix and
the rebuilt test SDK are documented in [PrefixDecodingValidation.md](PrefixDecodingValidation.md).
The subsequent Swift/C memory and cache improvements are documented in
[MemoryEfficiencyValidation.md](MemoryEfficiencyValidation.md).

## Covered behavior

- Explicit `ja → en → fr → metadata default` selection for each field.
- Correct default behavior with no preferences and no OS-language acquisition.
- Duplicate valid localized values join even when identical.
- Name/Author scalar translations, internal Description labels, bracket escaping,
  ordinary nonlanguage headings, and legacy unlabeled Description.
- Repeated Tags merge and deduplicate; Description examples stay text.
- Language changes invalidate projections and reject context-mismatched caches.
- Equivalent canonical priorities report no change; invalid priorities preserve
  the existing configuration.
- 1,000 scanned files and exported cache data contain no unselected name or
  description sentinel text. This checks retention in data structures, not RSS.
- Full editing preserves other languages; tag clearing removes every Tags field.
- New-feature saves require Name/default language; legacy writes remain supported.
- Unclosed Description and ending-marker/line injection are rejected on write.
- Rust, C and Swift selected-data provenance and full editing extensions agree.
- Workspace preferences survive volatile state reset.
- With a valid metadata default, unlabeled initial Description text belongs to
  that language; later labels switch sections normally. Default-field ordering,
  CR/LF/CRLF, empty initial text, invalid/missing defaults, escapes, repeated
  default sections, selected-only caches and full editing preservation are tested.

## Implicit default Description — actual fixtures

The user-modified `01-basic-tags.jsx` and `02-tag-whitespace.jsx` were scanned
without changing their contents. Their `Meta-Default-language=ja` assigns the
initial unlabeled Description text to Japanese.

| Preferences | Fixture 01 | Fixture 02 |
|---|---|---|
| Unspecified, or `ja/en/fr` | Japanese paragraph only | Japanese paragraph only |
| `en` | English paragraph only | `this is the test.` |
| `fr` | French paragraph only | Japanese default paragraph |
| `fr/en/ja` | French paragraph only | `this is the test.` |

All ten fixtures scan without errors or diagnostics across eight scan contexts.
The other eight fixtures and all Tags are unchanged. The existing 401-item
legacy corpus and its 437 whole-file parsing results also remain unchanged.
At this checkpoint, cache projection version became 3. The subsequent Swift API
work advances it to 4 and records metadata-read settings; see
[SwiftApplicationAPIValidation.md](SwiftApplicationAPIValidation.md) for current
results. The package version is still the 1.3.3 development base.

Inline labels are supported: `[ja]日本語です\n[fr]bonjor` returns `日本語です`
for Japanese and `bonjor` for French. This exact example was also scanned.

Test SDK verified at this checkpoint:
`build/arm64-tests/localization-n9ov3_o4/ScriptMetaKitFFI.xcframework`.
Local results are in `build/implicit-description-20261102-3g5ym5nz/`, including
`fixtures-output.json`, `fixture-selection.json`, `inline-label-output.json`,
the C ABI output, test logs and `verification.json`.

## Consumer sample

`examples/localized_scan.rs` supplies `["ja", "en", "fr"]` itself. It does not
query OS settings, execute scripts, or request remote updates. Its four fixtures
select Japanese, English, French, and finally German metadata defaults.

```sh
cargo run --locked --release --target aarch64-apple-darwin --example localized_scan
```

The JSON output includes the priority list, chosen text, actual language per
field, Tags and diagnostics. A folder argument can be passed after `--`.

## Test SDK packaging

`script/build_arm64_test_package.py` creates a separate arm64 XCFramework and a
SwiftPM test package under `build/arm64-tests/`. The committed Universal artifact
is unchanged. The test dylib is ad-hoc signed for local use; no app/notarized public
release was created.

No Intel or Windows build/test was performed. No Scripta!/Adobe UI was operated.
The AppleScript-related existing tests compile/decompile synthetic fixtures;
they do not execute their script contents.
