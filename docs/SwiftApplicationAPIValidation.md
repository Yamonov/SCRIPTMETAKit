# Swift application API — validation

This records the earlier development checkpoint. Current 2.0.0 release evidence
is in [Release2.0Validation.md](Release2.0Validation.md).

Date: 2026-11-03. Branch: `codex/tags-localization`.
Development base: 1.3.3. Target: `aarch64-apple-darwin`.

## Final results

| Check | Result |
|---|---|
| Rust/C ABI workspace, all features | **354 passed**, no failures |
| Swift SDK with the matching native arm64 XCFramework | **85 passed**, no failures or skips |
| Public Swift client without `@testable` | Release build and execution passed |
| Swift caller configuration | Swift 6.2, strict concurrency, MainActor default isolation, warnings as errors |
| C client compiled with the generated header | Passed with `-Wall -Wextra -Werror` |
| Header / native exports | All **130** public C functions exported |
| Swift C declarations | **123**, none unused |
| Rust Clippy, default and all features | Passed with warnings denied |
| Rust formatting and patch whitespace | Passed |
| Source manifest / copied SDK sources / dylib SHA-256 | Match |
| Protected fixtures, released Artifacts and unrelated integration plan | Unchanged; 28 files checked |
| Final full and focused review | No unresolved findings in the reviewed scope |

The seven C functions not bound directly in Swift are legacy variants covered by
the current bounded/v2 APIs and single-element `scan(folderURLs:)`. No necessary
application operation depends on these duplicate declarations.

## Regression coverage

The new tests cover atomic configuration validation, canonical languages and
extensions, explicit null timeouts, settings preserved across engine recreation,
cache read-context mismatch, release of pending data when persistence is disabled,
cache invalidation and dirty-root refresh, external changed paths, native watcher
disable/restart and preservation of persistent file-list provenance.

Text/file editing tests cover selected-only parsing, full-language editing,
unknown fields, URL/version input parity, comment delimiters, legacy inline-field
ambiguity, scalar line breaks, outer end markers, CR/LF/CRLF Description boundaries
and preservation of indentation/trailing whitespace. AppleScript tests compile
synthetic output and read synthetic compiled files; they do not execute scripts.

Read-failure regressions verify that unchanged filesystem identity does not make
a prior failed read permanent, and that unavailable/unknown compiled source never
becomes appendable merely through cache reuse. Update-check tests verify that
unused display/editing data does not cross the input ABI or replace cached display
metadata.

The existing tests for language fallback, repeated localized fields, Tags,
1,000-file selected-only retention, cross-view string/array sharing, file-reference
watching, cancellation, source conflicts, backup handling and persistent cache
merges continue to pass.

The original configuration reproducer now reports:

```text
disable_persistence_after_scan: cached_items=0, retained=false
disable_all_caches_after_scan: cached_items=0, retained=false
direct_language_change: preferences=["en"], name=Some("English name"), actual_language=Some("en")
```

## Real-corpus comparison

Compared with the preceding memory-fix checkpoint in eight contexts: unspecified
cold/cached, ja/en/fr, fr/en/ja, en, fr, es and return to unspecified.

- All **401 legacy metadata items** match, including every displayed field and
  localization/Tags data. All **437** whole-file parse results match.
- All **10 user-edited fixtures** match in every context. Their bytes are unchanged.
- The C ABI comparison also matches all 401 + 10 metadata items.
- One intentional candidate-state correction appears in the cached scan:
  `AppleScript/SetSameGridColors.scpt` changes from `appendable` / `true` to
  `unknown` / `false` for edit state / append permission. Its source had not been
  decompiled. Cold and cached scans now agree; the file itself is unchanged.
- Of the 450 files in the earlier corpus inventory, all 449 script/support files
  match their stored SHA-256. Only `.DS_Store` differs from that earlier inventory.

## Test package and evidence

Verified package:

```text
build/arm64-tests/localization-115871jn/
```

It contains the matching SDK source, public header, arm64-only XCFramework and
`test-manifest.json`. The native dylib is ad-hoc signed and signature-verified for
local tests. The committed Universal artifact is unchanged.

Evidence directory:

```text
build/swift-services-20261103-y9b613u5/
```

- `verification-final.json`: final test/API/hash summary.
- `comparison-final.json`: all-context metadata comparison and the one candidate change.
- `review-rust-all-final.log`, `review-swift-all-final.log`: full test results.
- `review-clippy-default.log`, `review-clippy-all.log`: warning-free static checks.
- `review-*-before.log`: failures that reproduced the review findings before fixes.
- `public-swift-client/`, `public-swift-build.log`, `public-swift-run.log`: the independent public API consumer.
- `c-services.log`, `config-repro.log`: C ownership/error smoke and the original configuration reproducer.

See [the review record](ImplementationReview-2026-11-03.md) for findings and fixes,
and [the API guide](SwiftApplicationAPI.md) for supported application operations.

No Intel/Windows build or real Scripta!/Adobe UI execution was performed. Runtime
RSS was not re-benchmarked in this pass; memory claims concern verified ownership,
retention and removed copies. No public release, commit or push was performed.
