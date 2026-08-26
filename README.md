# SCRIPTMETAKit 1.3.0

SCRIPTMETAKit is a Rust library and Swift package for parsing, editing, scanning, caching, and watching SCRIPTMETA-enabled script files.

The 1.0 release is intended for use by Scripta, ACEMenuPlus, and other consumer applications that need a reusable SCRIPTMETA engine across platforms.

Registered macOS aliases and symbolic links can be inspected through the shared Rust, C FFI, and Swift path-resolution API. `PathKind::WindowsShortcut` is reserved for compatibility; `.lnk` resolution is not implemented in the 1.x series and callers must treat it as unsupported.

## Package Version

- Rust crate: `scriptmetakit` `1.3.0`
- Rust FFI crate: `scriptmetakit_ffi` `1.3.0`
- Swift package product: `ScriptMetaKit`

## 1.3.0

- Adds opt-in root-priority reconciliation delivery while preserving the existing complete reconciliation behavior by default.
- Exposes reconciled and pending root identities so consumers can act on current watched roots without waiting for unrelated background roots.

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
