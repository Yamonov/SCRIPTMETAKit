# Partial file decoding fix — Apple Silicon

Date: 2026-11-02. Branch: `codex/tags-localization`.

## Problem and change

A valid UTF-8 script could lose its metadata when the 128 KiB scan prefix ended
inside a multibyte character. The whole prefix was treated as a complete document;
its failed UTF-8 validation could trigger the existing UTF-16 fallback, corrupting
the metadata markers even when the complete block was near the start of the file.

The shared text decoder now distinguishes a partial read from a complete file.
For partial reads, the existing `encoding_rs` decoder receives `last=false` and
returns only complete characters. An incomplete final character is buffered by
the decoder instead of disqualifying the encoding. Malformed sequences inside
the supplied prefix still disqualify that encoding.

This path is used by metadata scans and text-file editing previews. UTF-8,
UTF-16 LE/BE with and without BOM, EUC-JP and Shift_JIS are covered. Partial
BOM-less UTF-16 can have an odd byte count. The existing complete-file decoding
policy, read-size limits and editing write path are preserved.

No dependency was added or updated. `encoding_rs` remains at 0.8.35.
Package version remains 1.3.3 on the development branch.

Metadata projection version advances from 1 to 2 so that persisted results made
with the old decoder must be reloaded from source. Cache schema remains 3.

## Validation

| Check | Result |
|---|---|
| Rust/C ABI workspace, all features, `aarch64-apple-darwin` | 315 passed |
| Swift SDK against rebuilt arm64 XCFramework | 66 passed, 0 failed, 0 skipped |
| C client linked against rebuilt arm64 dylib | 401 actual-corpus items match Rust |
| Clippy, warnings denied | Passed |
| `cargo fmt --all --check`, `git diff --check` | Passed |
| Cargo manifest and lockfile compared with pre-fix state | Unchanged |
| Committed Universal artifacts | Unchanged |

Eleven additional test cases cover UTF-8 boundaries for 2-, 3- and 4-byte
characters, UTF-16 code-unit and surrogate boundaries, Japanese legacy encoding
boundaries, genuine malformed input, complete-file EOF, the default scan limit,
editing previews, an unclosed metadata block outside the limit, and cache
invalidation. Tests use synthetic temporary files and do not execute scripts.

Commands run from the Kit root:

```sh
cargo test --workspace --all-features --locked --offline --target aarch64-apple-darwin -- --test-threads=4 --quiet
cargo clippy --workspace --all-targets --all-features --locked --offline --target aarch64-apple-darwin -- -D warnings
cargo fmt --all --check
python3 script/build_arm64_test_package.py
xcodebuildmcp swift-package test --package-path build/arm64-tests/localization-_75ml1c1 --configuration debug
```

## Actual legacy corpus

Root: `local legacy JSX corpus`

| Result | Before this fix | After this fix |
|---|---:|---:|
| Files in the source folder | 450 | 450 |
| Supported script candidates | 438 | 438 |
| Metadata returned by normal scan | 387 | 401 |
| Previously missed metadata | 14 | 0 |
| Returned file issues | 0 | 0 |

All 14 formerly missed scripts are now marked as having editable metadata, rather
than as candidates for appending a new block. The existing 387 items are exactly
unchanged from the pre-fix tags/localization implementation. Direct whole-file
parsing of all 437 text scripts also remains unchanged.

The 401 metadata items agree across the initial scan, cached scan, language order
`ja/en/fr`, order `fr/en/ja`, `es`, and a return to unspecified preferences.
The native C ABI scan with `ja/en/fr` returns the same 401 items and common fields.

All 450 input files retain their SHA-256, size and modification time. Scripts
were not executed or edited. No Scripta!/Adobe UI was operated. This validation
does not claim real-app UI, Intel or Windows runtime coverage.

The separate pre-existing `SetSameGridColors.scpt` cache issue remains: without
decompilation its edit state changes from `unknown` to `appendable` on a cached
scan. It is outside this text-decoding fix.

## Local evidence and test SDK

- Fixed scan output: `build/prefix-decoding-20261102-68n8r71i/fixed-output.json`
- Native C API output: `build/prefix-decoding-20261102-68n8r71i/fixed-ffi-output.json`
- Verification summary: `build/prefix-decoding-20261102-68n8r71i/verification.json`
- Rust test log: `build/prefix-decoding-20261102-68n8r71i/rust-tests.log`
- Swift test log: `build/prefix-decoding-20261102-68n8r71i/swift-tests.log`
- Rebuilt arm64 XCFramework: `build/arm64-tests/localization-_75ml1c1/ScriptMetaKitFFI.xcframework`

The test dylib is ad-hoc signed for local testing. This work does not replace the
committed Universal SDK or create a public release. No commit or push was made.
