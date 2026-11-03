# SCRIPTMETAKit 2.0.0 — macOS release validation

Date: 2026-11-03. Branch: `codex/tags-localization`.
Scope: macOS 2.0.0 release validation. The review records below precede publication;
the release tag and GitHub Release identify the published package.

The subsequent registered-root pause/resume addition and its current verification
are recorded in [RootWatchPauseValidation.md](RootWatchPauseValidation.md).
The full-suite matrix below records the base release review before that addition;
the current arm64 totals are 370 Rust/C tests and 94 Swift tests per language mode.

The later [watch API review](RootWatchPauseReview-2026-11-03.md) supersedes those
totals: 378 Rust/C tests and 99 Swift tests per language mode, with rebuilt
Universal artifacts and both Xcode client architectures checked.

## Result

The macOS source, Swift package and Universal native artifact are version 2.0.0.
Full and focused reviews were repeated after fixes. No unresolved defect remains
among the findings reproduced in this review. This statement is limited to the
verified environments and behaviors below; it is not a claim about untested OSs,
hardware or consumer application UI.

Existing Swift calls and C struct layouts are retained. C ABI generation remains
1. The binary now exports 134 public C functions, including all 105 functions from
the original 1.3.3 header. Swift has 127 C bindings with no unused declarations;
the remaining seven C functions are legacy variants covered by the newer calls.

## Release-pass findings and repairs

| Finding | Repair / evidence |
|---|---|
| Workspace destruction left a consumer waiting on an open stream | Finish the session and cancel delayed saves in deinit. Reproduced a timeout before the fix; lifecycle regression passes afterward |
| Drain delays multiplied UInt64 milliseconds into nanoseconds | Use Duration; the maximum-millisecond cancellation probe and public-API regression complete safely |
| macOS Dispatch I/O requested the entire file despite the configured source limit | Pass the limit plus one detection byte to the native producer. A 64 MiB file with a stalled consumer queues only the requested 64 KiB + 1 bytes in the regression |
| Cache revision checks converted full metadata and file trees to Swift | Return revision-only JSON and keep cache adoption/provenance bookkeeping in one serialized operation |
| Source-load followers woke every 50 ms while waiting | Use completion/cancellation notifications, with lock ordering that prevents a lost wakeup |
| HTTP response retention ignored its configuration | Make the setting effective and public in Swift. Default true preserves the earlier actual revalidation behavior; false or disabled memory cache prevents cross-operation retention |
| Clearing a shared HTTP cache could affect a rollback candidate's original engine | Detach shared state on clear; clear unique state without allocation or locking |
| Expected cache incompatibility was reported as a load failure | Introduce `cache_incompatible` / C status 7. Workspace quietly discards incompatible derived caches; corrupt data and I/O errors still diagnose |
| Language fallback allocated a candidate vector for each field | Use a borrowed iterator; extension/script/base fallback remains covered |
| CI omitted newly added test suites | Run all workspace tests and add the headless Xcode consumer. Install both macOS Rust target libraries |
| Artifact checks sampled only a few C exports | Check every declared public function for both CPU slices, actual runtime version/features, install name, system dependencies and deployment target |
| Source provenance did not cover integration fixtures or frame file boundaries | Fingerprint format 2 covers test fixtures/CI with length-framed paths and per-file SHA-256 values |
| Rebuilding did not recreate the enclosing XCFramework signature | Sign and verify the native image and enclosing bundle; the local candidate preserves the previous release's Developer ID publisher |

The earlier tags/localization and Swift API findings are documented in
[ImplementationReview-2026-11-03.md](ImplementationReview-2026-11-03.md).

## Test matrix

| Verification | Result |
|---|---|
| Rust/C ABI, all features, arm64 | **361 passed** |
| Rust/C ABI, all features, x86_64 under Rosetta | **361 passed** |
| Swift 6 package tests, arm64 | **89 passed**, no skips |
| Swift 5 language-mode package tests, arm64 | **89 passed**, no skips |
| Swift package XCTest bundle, x86_64 under Rosetta | **89 passed** |
| Xcode Release app, public API, strict concurrency/MainActor default | Built and ran on arm64 and x86_64 |
| Xcode app sandbox, resource embedding and native dylib loading | Passed; smoke runs emitted no stderr or diagnostics |
| Original 1.3.3 C header against the new library | Linked and scanned the 10 fixtures on both CPUs with identical output |
| Clippy, formatting and license resources | Passed |
| Windows MSVC target, default and native-watch configurations | Cross-target Clippy passed with warnings denied; no native Windows execution |

The installed Xcode's SwiftPM testing helper is arm64-only. Its `--arch x86_64`
invocation built the x86_64 tests but could not load them in the helper. The actual
x86_64 XCTest bundle was subsequently run with the x86_64 XCTest agent under
Rosetta, and all 89 tests passed. A preceding XcodeBuildMCP test-products error was
also a test-runner issue, not a Kit failure.

## Xcode integration

The reproducible project is
`tests/integration/KitIntegration.xcworkspace`; instructions are in
[MacOSIntegration-2.0.md](MacOSIntegration-2.0.md).

The initial generated minimal host project omitted the normal Frameworks runpath:
linking succeeded, but dyld could not locate the embedded library. The final
fixture explicitly keeps `@executable_path/../Frameworks`, does not re-strip the
already stripped/signed library, and disables the obsolete user-header search
setting. It builds without the initial host-configuration warnings. The existing
Scripta project was inspected read-only and already contains these standard settings.

Both the library and app bundle signatures were verified. The integration app is
an ad-hoc-signed test executable, not a distributable/notarized Scripta application.
No real Scripta or Adobe application was launched or operated.

## Runtime measurements

Environment: Apple M1 Ultra, 128 GiB RAM, macOS 26.6.2, Apple Swift 6.4.2,
Rust 1.94.0. Three independent arm64 Release runs used 1,000 synthetic JSX files
with four language sections and 4 KiB of text per section. Files were newly
created, so filesystem caches were warm; “initial” means no Kit cache. All runs
used the same public SDK through the sandboxed Xcode application.

| Measurement | Median | Range |
|---|---:|---:|
| Initial scan and persistent save | 0.393 s | 0.366–0.400 s |
| Repeat scan and persistent save | 0.434 s | 0.429–0.460 s |
| Restore from persistent cache | 0.074 s | 0.072–0.074 s |
| Language change, source reload and save | 0.397 s | 0.382–0.420 s |
| External change to delivered update | 0.722 s | 0.715–0.727 s |
| Shutdown | 1.17 ms | 1.14–1.18 ms |
| Idle CPU, percent of one core | 0.0033% | 0.0027–0.0045% |
| Idle watch updates | 0 | 0 |
| Interrupt wakeups during ~10 seconds idle | 3 | 3–4 |
| Idle process footprint | 42.9 MiB | 40.8–53.7 MiB |

The idle measurement includes its own wakeup timer and OS runtime activity. The
library's idle watch path uses notification waits rather than periodic polling.
Change-delivery time includes the default coalescing delay and refreshing the
affected root. Repeat-scan figures include durable cache merge/save work and are
not a measurement of a memory-only lookup.

Median idle RSS was 116.5 MiB and peak RSS 117.7 MiB. These are whole-process
measurements, including the host, runtime and allocator; they are not Kit-only
live allocation counts. RSS did not return to its initial value immediately on
shutdown. Ownership/reference-count regressions separately verify metadata and
pending-data release; allocator-retained pages must not be called a proven leak
or claimed to have been returned to the OS. This pass did not run Instruments.

## Existing scripts and fixtures

Compared with the last reviewed development implementation in eight language/read
contexts:

- **401 legacy metadata items** and all **437** whole-file parse results match.
- All **10 user-edited fixtures** match, including Tags and selection provenance.
- Candidate records also match, including the corrected unknown state for the
  compiled script that had not been decompiled.
- The 17 protected fixture/example/integration-plan files checked against the
  start-of-task hashes are unchanged.

## Artifact and evidence

Current consumer package: repository `Package.swift` plus
`Artifacts/ScriptMetaKitFFI.xcframework`.

The Universal library and enclosing XCFramework are Developer ID signed with the
same publisher as 1.3.3, with timestamps. Both arm64/x86_64 slices are present.
Its install name is `@rpath/libscriptmetakit_ffi.dylib`; all linked dependencies
are system libraries/frameworks. Its deployment target does not exceed macOS 13.
The manifest's source and dylib hashes match the reviewed working tree/artifact.
The manifest records a dirty source state because this work has not been committed.

Local evidence directory:

```text
build/release-2.0-review-20261103-zuk6f9r4/
```

Key evidence: `verification.json`, `corpus-comparison.json`, `benchmark-summary.json`,
`rust-arm64-final.log`, `rust-x86_64-final.log`, `swift-package-six.log`,
`swift-package-five.log`, `swift-x86_64-xctest.log`, the Xcode build/smoke logs,
and `universal-build-signed.log`. `before-source-and-artifacts.tar.gz` preserves
the complete pre-review source and artifact state.

Native Windows runtime/DLL integration, a physical Intel Mac, an actual macOS 13
machine, physical drive/network-share removal and real consumer UI behavior were
not executed in this pass. Windows release validation remains necessary before
claiming a fully validated cross-platform 2.0 release. No commit, push, tag or
public release was performed.
