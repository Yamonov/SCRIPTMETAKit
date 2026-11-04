# SCRIPTMETAKit 2.1.1

SCRIPTMETAKit is a Rust library and Swift package for parsing, editing, scanning, caching, and watching SCRIPTMETA-enabled script files.

Scripta, ACEMenuPlus and other consumer applications share the same metadata rules
through the Rust core, C ABI and macOS Swift package.

For script authors and AI assistants, start with the [metadata authoring guide](docs/MetadataAuthoring.md). It contains complete validated examples and the required rules for Tags, localization and custom fields.

Registered macOS aliases, Windows Shell links (`.lnk`), and symbolic links can be inspected through the shared Rust, C FFI, and Swift path-resolution API. OS-specific resolution and native notifications are compiled only on their corresponding platforms.

## 2.1.1 macOS and Windows package

Version 2.1.1 fixes Swift-to-C calling conventions on x86_64 by importing the native
C module and types. See [Swift ABI regression validation](docs/Release2.1.1Validation.md).

Version 2.1.0 adds selected author-defined custom metadata fields, shared validation
and key-scoped editing through Rust, C and Swift. It provides the macOS Swift package
and Universal XCFramework, plus a Windows x64 DLL. Common `Tags`, BCP 47 metadata
localization, exact-language editing and per-root watch suspension are available
through the same shared core.

The Swift package requires Swift 6.1 / Xcode 16.3 or later and targets macOS 13 or
later. Swift 5 and Swift 6 language modes are supported. The macOS binary contains
arm64 and x86_64 slices. The Windows DLL uses the same C ABI and shared Rust core.

Use `https://github.com/Yamonov/SCRIPTMETAKit.git` and version **2.1.1** in Xcode.
An existing dependency rule starting at 1.3.3 and allowing updates up to the next
major version excludes 2.0.0; change its minimum version to 2.1.1 when upgrading.

- Consumers supply priorities, such as `ja`, `en`, `fr`. The Kit never obtains OS/UI languages.
- Normal results retain selected Name, Author and Description only, plus Tags and selection diagnostics.
- Changing canonical preferences invalidates language-dependent caches. Scan again to reload source files.
- Full editing drafts preserve all translations; Description remains the complete raw multilingual body.
- Repeated valid localized text is concatenated, including identical content. Tags instead merge with exact deduplication.
- The C ABI additions leave legacy public struct layouts unchanged. Cache schema 4 / projection 6 identifies the language, custom-field selection and metadata-read context.

See [multilingual editing APIs](docs/LocalizationEditingAPI.md) and
[field requirements and relationships](docs/MetadataFieldRules.md).
See [Tags and localization](docs/TagsLocalization.md) for syntax and APIs.
See [2.1.0 review and validation](docs/Release2.1Validation.md) for release verification.
See [Swift application API](docs/SwiftApplicationAPI.md) for configuration, text
parsing/editing, validation, cache invalidation and external change notifications.
See [macOS package integration](docs/MacOSIntegration-2.0.md) and
[2.0 release validation](docs/Release2.0Validation.md) for embedding requirements,
the reproducible Xcode consumer and the final verification scope.

Apple Silicon-only sample:

```sh
cargo run --locked --target aarch64-apple-darwin --example localized_scan
```

The sample passes `ja`, `en`, `fr` directly and reads only the bundled fixture folder
(or an explicitly supplied folder argument). It does not execute scripts or check remote updates.

For the Swift SDK, build an isolated native test package without replacing the
committed Universal XCFramework:

```sh
python3 script/build_arm64_test_package.py
```

Use the printed `ARM64_TEST_PACKAGE` path for SwiftPM build/test. Consumers can
also set `SCRIPT_META_KIT_FFI_PATH` to a local development XCFramework path when
using the main package manifest. For an application dependency, use the main
package with its matching Universal artifact. Rebuild it with
`script/build_xcframework.sh` after changing Rust or C code.

## Package Version

- Rust crate: `scriptmetakit` `2.1.1`
- Rust FFI crate: `scriptmetakit_ffi` `2.1.1`
- Swift package product: `ScriptMetaKit`
- C ABI generation: `1` (existing layouts/calls retained; additions are additive)

## 2.1.1

- Imports all Swift FFI functions and data structures from the C module.
- Fixes x86_64 metadata-save crashes caused by using the Swift ABI for C calls.
- Preserves the public Swift API, C ABI generation 1 and metadata format.
- Tests public Swift save APIs on arm64 and x86_64 in Debug and Release.

## 2.1.0

- Adds author-defined `X-creator-field=value` metadata with exact-key selection or `X-` for all custom fields.
- Retains selected custom values only and includes that selection in cache compatibility checks.
- Exposes shared input validation, diagnostics and key-scoped editing through Rust, C and Swift.
- Preserves empty/repeated values, unknown fields and every unedited localization during edits.
- Keeps public C layouts compatible and adds eight C functions.
- Preserves Windows DACLs and their inheritance settings during metadata file replacement.

Start with the [metadata authoring guide](docs/MetadataAuthoring.md) to write or edit
metadata, including with an AI assistant. Its complete JSX and AppleScript examples
are checked by the Kit's parser and save APIs in Japanese, English and French.
See [custom metadata fields](docs/CustomMetadataFields.md) for the complete syntax and APIs.

## 2.0.1

- Adds a shared multilingual editor to Rust, C and Swift; editing never applies display fallback.
- Safely changes the default language and returns the last language to ordinary fields, preserving Tags and unknown fields.
- Retains empty languages, whitespace and untouched Description sections.
- Makes Tags independent of Name/default-language requirements. Description without a valid default remains ordinary text.
- Keeps legacy nil-extensions preservation, conditional saves and backups.

## 2.0.0

- Adds Tags and explicit BCP 47 metadata language selection, with full-language editing preservation.
- Exposes safe configuration, metadata text tools, validation, cache invalidation and change notifications to Swift.
- Retains selected metadata only during normal operation and shares immutable data between result views.
- Uses lightweight revision queries for Workspace cache bookkeeping instead of converting complete file trees and metadata.
- Finishes watch streams and cancels delayed saves when a Workspace is released. Idle watching is notification-driven.
- Bounds native macOS metadata-source read-ahead by the configured source-byte limit.
- Uses notification-driven source-load cancellation, including Windows builds without HTTP support.
- Makes `cacheNetworkResponses` effective; its default is true to preserve prior conditional HTTP revalidation. Disabling it or global memory caching prevents response retention between checks.
- Treats incompatible version/language/read-context caches as expected misses. C cache loads return `SMK_STATUS_CACHE_INCOMPATIBLE`; Workspace discards those derived caches without a failure diagnostic.
- Adds `pauseWatching(rootIDs:)`, `resumeWatching(rootIDs:)` and `isWatchingPaused(rootID:)` to Engine and Workspace. Resume rereads changed roots before returning and keeps incomplete batches paused.
- Prevents stale watch-result publication across pause/resume, honors dirty scopes during progressive reconciliation, and avoids duplicate source reads and catalog-index copies during root control.
- Joins retired native watchers after releasing the engine mutex, allowing notification callbacks to query the C API safely.
- Validates all public C exports, both macOS slices, install names, deployment targets and runtime version/capabilities before installing an artifact.

## 1.3.3

- Fixes Intel linking of the macOS Universal dylib by disabling Rust's implicit debuginfo strip and retaining Apple's final symbol reduction.
- Validates LINKEDIT string-pool alignment and real native linking for both arm64 and x86_64 in the final XCFramework; runs engine create/free on every architecture available on the build host.
- Checks the committed binary in CI before rebuilding it, so a locally generated invalid slice cannot pass through native-architecture-only Swift tests.
- Keeps the Rust implementation, public API, ABI, and cache format unchanged from 1.3.2.

## 1.3.2

- Parses escaped line endings in SCRIPTMETA-DIST blocks embedded in JSON, including note article metadata.
- Preserves literal backslash pairs and unknown escapes while decoding supported HTML and line-ending escapes in one pass.
- Adds regression coverage for body-over-head selection, streamed chunk boundaries, and public distribution resolution.
- Resolves Windows Shell links to filesystem targets without launching or repairing them, including file and folder references.
- Watches file-reference targets through their parent directories on macOS and Windows. Shared watches are deduplicated, and unrelated sibling files do not invalidate a reference's registered root.
- Tracks reference sources separately from targets, detects retargeting, and retains the last known watch target while a reference is temporarily broken.
- Starts Windows watches after notification requests are registered, and rereads notified metadata even when file size and modification time are unchanged.
- Adds a standalone Windows DLL build script and native reference-watch tests for both platforms.
- Retains the existing C/Swift API, ABI, and metadata-source selection policy.

This release consolidates the previously local Windows reference support into the common 1.3.2 release. The version follows the project's chosen numbering; the additional Windows capability would ordinarily be a minor release under strict SemVer. The published Rust `ScannerOptions` fields remain unchanged.

### Platform builds

Build the Universal macOS XCFramework on macOS:

```sh
./script/build_xcframework.sh
```

Build the Windows x64 DLL on Windows:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File ./script/build_windows.ps1 -Release
```

Both builds use the same source, version, and fixed `blocking-http,native-watch` features. See [WindowsReferences.md](WindowsReferences.md) for Shell-link behavior.

## 1.3.1

- Expands every directory alias or symbolic-link reference within a registered root while stopping actual ancestor cycles.
- Preserves each reference's display path and ancestor context during partial refreshes, including references to descendants of a changed directory.
- Updates and removes all affected logical references, instead of stopping after the first physical-path match.
- Keeps metadata inspection deduplicated by physical directory and preserves the registered display-root path during metadata-only refreshes.
- Reconciles older cached trees containing false cycle markers or physical child paths under an alias before they become current again.
- Retains the existing scan limits, C/Swift API, and persistent-cache format.

## 1.3.0

- Adds opt-in root-priority reconciliation delivery while preserving the existing complete reconciliation behavior by default.
- Swift watch updates expose reconciled and pending root identities so consumers can act on current watched roots without waiting for unrelated background roots.

## 1.2.2

- Watches the resolved physical targets of macOS alias folders, including targets that are not registered as separate roots.
- Reuses an existing physical watcher when an alias target is already registered, while routing additions, modifications, and removals to both the direct root and every alias-owning root.
- Reconfigures a live native watcher when aliases are added, removed, or retargeted, and restores alias-target topology from persistent file-list cache data before watching starts.
- Preserves alias-target monitoring for empty folders and after file-list memory eviction, without exposing hidden empty folders in the file-list result.
- Adds deterministic Rust and C FFI coverage for two-root routing, cache restoration, retargeting, watcher callbacks, and dynamically discovered targets.

## 1.2.1

- Propagates `CancellationError` from persistent cache loads without reporting `cache_load_failed` or deleting the cache file.
- Keeps the existing warning and cache removal behavior for corrupt or invalid persistent caches.
- Adds deterministic cancellation, cache-preservation, recovery, and corrupt-cache regression coverage.

## 1.2.0

- Adds root-state and file-list-content revisions with stable content identity across no-op reconciliation.
- Exposes current root state, last-good content, freshness, completeness, and cache provenance through public Swift file-list state APIs.
- Adds atomic visible-root activation with watcher rollback, multi-root cached-state recovery, sequenced watch updates, and watch-session generation protection.
- Preserves `children == nil` versus a valid empty list across the C FFI without changing legacy snapshot struct layouts.
- Retains last-good content for missing roots and identifies initial, restart, and overflow reconciliation results.
- Merges persistent cache data by root and scope so memory eviction and idle expiry do not remove unresident durable entries.
- Makes live root and watcher reconfiguration transactional: a replacement watcher is started before the new engine state is committed.
- Enforces the configured cache byte limit before atomic replacement, preserving the previous valid cache after an oversized save failure.
- Adds stateless root preflight, effective root scan priority, integrated single-pass density checks, and Windows watcher reconciliation after a directory handle is restored.

### File-list API contracts

The main Swift entry points are:

```swift
public func activateFileListRoot(
    _ rootID: String?,
    cacheScope: ScriptMetaCacheScope? = .fileList
) async throws -> ScriptMetaKitFileListState?

public func cachedFileListStates(
    rootIDs: [String],
    cacheScope: ScriptMetaCacheScope? = .fileList
) async throws -> [String: ScriptMetaKitFileListState]

public func watchUpdates(
    roots: [ScriptMetaKitRoot],
    replacingGroup groupID: String,
    cacheScope: ScriptMetaCacheScope? = nil,
    dirtyOnly: Bool = false
) async throws -> ScriptMetaKitWatchUpdateSequence

public func preflightRoot(
    _ root: ScriptMetaKitRoot
) async throws -> ScriptMetaScanResult
```

`preflightRoot(_:)` validates a candidate root without registering it, populating the metadata catalog, retaining a file-list tree, or changing a watcher plan. File-list density checks are performed during the real scan traversal, so registered scans do not walk the tree a second time.

`ScriptMetaKitFileListState` separates current root state from last-good content. `freshness` reports filesystem verification, `completeness` reports truncation, and `source` reports memory or persistent provenance. A content revision is reusable only within the same non-empty workspace epoch.

Watch updates start at sequence 1 for each stream ID. A slow consumer can detect a dropped buffered result from a sequence gap, then recover all current registered states with `cachedFileListStates(rootIDs:)`. `watchChanges(...)` remains as a source-compatible adapter over the same native watcher and update pump.

`watchUpdates(...)` keeps complete reconciliation delivery as its default. Consumers with latency-sensitive roots may opt in to `.progressiveByRootPriority`; each update then reports cumulative `reconciledRootIDs` and remaining `pendingRootIDs`, and the cycle finishes with a reconciliation update whose `coversAllWatchedRoots` value is `true`. Reconciliation coverage does not replace each root's `freshness` and `completeness` checks.

Compatibility enum cases remain available but no longer create distinct behavior:

- `ScriptMetaCacheScope.root` uses the same durable file and FFI scope as `.fileList`; the old `ScriptMetaKitRootCache.cache` filename remains readable for migration.
- `ScriptMetaRefreshPolicy.onFileEventDeferred` is normalized to `.onFileEvent`.
- `ScriptMetaRootPriority.userInitiated` scans first, the selected `.visibleWhenSelected` root scans next, and other visible/background roots follow while result ordering remains registration-stable.
- `ScriptMetaCachePolicy.persistentCatalogOnly` contributes metadata to durable catalog export without retaining a resident catalog solely for that policy.

Persistent cache reads and writes accept 1 through 64 MiB. Invalid limits produce a nonfatal Workspace diagnostic, and an oversized save never deletes or replaces the previous valid file.

## 1.1.3

- Describes SCRIPTMETAKit's purpose, author, and supported platforms in `ScriptMetaKitRuntime.acknowledgementsSummaryText`.
- Keeps the complete license texts and notices unchanged in `ScriptMetaKitRuntime.acknowledgementsText`.

## 1.1.2

- Adds `ScriptMetaKitRuntime.acknowledgementsSummaryText` for concise About-screen component listings.
- Keeps complete license texts and notices available through `ScriptMetaKitRuntime.acknowledgementsText`.
- Generates and verifies both acknowledgement resources from the macOS release dependency graph.

## 1.1.1

- Bundles the macOS Rust dependency acknowledgements as a Swift Package resource.
- Exposes the generated text through `ScriptMetaKitRuntime.acknowledgementsText`.
- Verifies in CI that `THIRD_PARTY_LICENSES.txt` matches `Cargo.lock`.

## 1.1.0

- Keeps scan and update cancellation in one operation scope, including cancellation requested immediately before native work begins.
- Shares one filesystem traversal between file-list and metadata collectors for combined full scans and dirty refreshes.
- Defers duplicate script-header probes to the metadata collector during combined full scans.
- Reuses successful and terminal-failure results for each metadata URL within one batch update check, so every URL has one shared retry budget regardless of how many scripts reference it.
- Retries transient source failures at most twice after the initial request, using cancellation-aware 500 ms and 1,500 ms backoff delays by default.
- Cancels an active HTTP request without waiting for its request timeout, while preserving the existing synchronous FFI API.
- Uses macOS Dispatch I/O for local metadata sources so cancellation and resource deadlines can interrupt a stalled file or network-volume read without abandoning worker threads.
- Preserves the previous file-list subtree and dirty state after nested directory I/O failures.
- Adds conditional metadata writes that reject stale source fingerprints instead of overwriting external edits.
- Removes Swift's polling wait, exposes typed status projections, and returns directory-state snapshots through the public Swift API.
- Strengthens cache and metadata replacement durability on macOS by syncing the containing directory.
- Treats idle public cancellation as a no-op and binds hand-off cancellation to one reserved operation.
- Preserves the last good catalog and file-list state across simulated transient permission and I/O failures.
- Bounds metadata sources to 4 MiB by default and classifies HTTP, transport, and file failures before retrying.
- Honors bounded `Retry-After` hints and singleflights requests that converge on the same downstream `Latest-URL`.
- Revalidates repeated HTTP metadata checks with bounded `ETag`/`Last-Modified` state and reuses the cached body after `304 Not Modified`.
- Uses continuous bounded worker queues for root scans and update groups instead of chunk barriers.
- Keeps watcher queues bounded, avoids restart when the normalized plan is unchanged, and schedules one reconcile after a required restart.
- Uses notification-driven HTTP and Dispatch I/O cancellation without active 5 ms polling.
- Compacts persistent cache JSON, debounces watcher-driven saves, and skips identical writes.
- Exposes `ScriptMetaKitOperationalPolicy` presets, `ScriptMetaKitWatchSequence`, nonfatal workspace diagnostics, explicit termination policy, and safe edit sessions.
- Splits Swift editing, FFI operational policy, and Resolver retry logic into focused source files.

## 1.0.9

- Makes FFI cancellation, Swift Task cancellation, and Workspace compound operations safe under concurrency.
- Preserves the previous catalog and dirty state when a refresh is cancelled, times out, overflows, or encounters an unreadable subtree.
- Preserves executable permissions, ACLs, and extended attributes during metadata edits, and makes backup reset transactional.
- Unifies parser, marker, JSDoc, symlink, alias, UTF-16, and scan-status behavior across Rust, C FFI, and Swift.
- Adds a public registered-path resolution API and reproducible, staged XCFramework generation with a release manifest.

## 1.0.8

- Cleans up native watcher event filtering so the Rust workspace passes strict Clippy checks with warnings denied.

## 1.0.7

- Adds `ScriptMetaKitWorkspace.clearVolatileState()` for ending a consumer UI session while preserving persistent cache files.
- The new API clears in-memory workspace state and resets the engine configuration so the next operation starts from the persistent cache path normally.

## 1.0.6

- Removes the periodic 0.25 second wake from the macOS FSEvents watcher while idle.
- The watcher now blocks on the Core Foundation run loop and wakes only for file events or shutdown.

## 1.0.5

- Fixes update diagnostics for multi-script `SCRIPTMETA-DIST` blocks when the requested `Script-ID` is missing.
- The resolver now reports the requested missing `Script-ID` instead of reusing another entry's `Script-ID`.

## Third-Party Licenses

The Swift package provides two generated acknowledgement resources:

- `ScriptMetaKitRuntime.acknowledgementsSummaryText` is a concise component
  index for an application's About screen. It contains package names, versions,
  SPDX license expressions, and upstream repository links.
- `ScriptMetaKitRuntime.acknowledgementsText` contains the complete
  SCRIPTMETAKit license plus the license texts and notices of Rust crates used
  by the macOS release targets. Applications should make this complete text
  available from a license-details action.

Regenerate the document after changing `Cargo.lock`:

```sh
cargo install --locked --features cli --version 0.9.1 cargo-about
./script/generate_third_party_licenses.sh
```

Verify that the committed resource is current:

```sh
./script/generate_third_party_licenses.sh --check
```

## License

SCRIPTMETAKit is licensed under the Apache License, Version 2.0.

See [LICENSE](LICENSE) and [NOTICE](NOTICE).

## Names

Project names and marks are not licensed as part of the source-code license.

See [TRADEMARKS.md](TRADEMARKS.md).
