# SCRIPTMETAKit 1.0

SCRIPTMETAKit is a Rust library and Swift package for parsing, editing, scanning, caching, and watching SCRIPTMETA-enabled script files.

The 1.0 release is intended for use by Scripta, ACEMenuPlus, and other consumer applications that need a reusable SCRIPTMETA engine across platforms.

Registered macOS aliases and symbolic links can be inspected through the shared Rust, C FFI, and Swift path-resolution API. `PathKind::WindowsShortcut` is reserved for compatibility; `.lnk` resolution is not implemented in the 1.x series and callers must treat it as unsupported.

## Package Version

- Rust crate: `scriptmetakit` `1.1.1`
- Rust FFI crate: `scriptmetakit_ffi` `1.1.1`
- Swift package product: `ScriptMetaKit`

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

The Swift package bundles `THIRD_PARTY_LICENSES.txt` and exposes it through
`ScriptMetaKitRuntime.acknowledgementsText`. The generated document contains
the SCRIPTMETAKit license and the licenses of Rust crates used by the macOS
release targets.

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
