# Registered-root watch suspension — 2.0.0

Date: 2026-11-03. Branch: `codex/tags-localization`.
Status: implemented locally; not committed or published.

This records the initial implementation pass. The subsequent fixes and current
378 Rust/C / 99 Swift test results are documented in
[RootWatchPauseReview-2026-11-03.md](RootWatchPauseReview-2026-11-03.md).

## API contract

Swift Engine and Workspace expose `pauseWatching(rootIDs:)`,
`resumeWatching(rootIDs:)` and `isWatchingPaused(rootID:)`. Workspace resume also
accepts an optional persistent-cache scope. Rust owns the suspension state and
source reload; C exposes matching operations without changing existing ABI layouts.

- Pausing synchronizes with the current Kit operation and retains cached content.
  Native monitoring excludes paused roots where the physical topology permits it;
  logical event routing excludes them even when an unpaused root shares a watch.
- Event routing, overflow refresh and progressive reconciliation do not scan the
  paused registrations. Other registrations and the current stream remain active.
- Resume arms monitoring before reading the requested roots, then performs a full
  file-list and metadata read. Matching size, timestamp and file identity do not
  permit metadata reuse. No network update checks are made.
- Missing, unreadable, cancelled or incomplete root results leave every requested
  root paused. A later explicit resume retries the read. Failed native setup does
  not silently release the suspension.
- Pause is idempotent. Unknown/empty ID lists fail without partial changes.
  Removal of a root clears its suspension; re-registering the same live ID and
  applying configuration preserve it. Shutdown clears roots and suspension.
- Explicit scans and script execution remain application responsibilities.
  Multiple registrations covering an updating repository must be paused together.
  Buffered application results must respect the application's update state.

Usage and lifecycle details are in [SwiftApplicationAPI.md](SwiftApplicationAPI.md).

## Verification

| Check | Result |
|---|---|
| All Rust/C suites, macOS arm64, all features | 370 passed |
| Added Rust core regressions | 6 passed |
| Added C regressions, including actual native watching | 3 passed |
| Swift 6 full suite, macOS arm64 | 94 passed, no skips |
| Swift 5 language-mode full suite, macOS arm64 | 94 passed, no skips |
| Added Swift public-API and stream regressions | 5 passed in both language modes |
| Xcode Release integration app, arm64 and x86_64 under Rosetta | Public pause/query/resume exercised through embedded Universal dylib; sandboxed, no windows or script execution; no stderr or diagnostics |
| Native artifact | 134 C exports, native linking and version/lifecycle checks for arm64 and x86_64; Developer ID signatures verified |
| Rust formatting and all-target/all-feature Clippy | Passed, warnings denied |
| Rust no-default-feature boundary | Passed |
| Windows MSVC, native-watch, all targets | Cross-target check and Clippy passed; native Windows execution not performed |

The tests cover same-size/same-timestamp metadata changes, tree additions and
deletions, suspension after queued changes, watcher overflow, missing roots,
batch failure/retry, cancellation, invalid IDs, root/configuration changes,
continued stream delivery for other roots, all roots paused, and progressive
reconciliation with shared physical watches. The existing rejection of an empty
watch configuration without suspended registrations remains covered.

An all-feature Windows cross-check from macOS requires a Windows C toolchain for
the existing `ring` HTTP dependency; that configuration was unavailable here.
The changed watcher/core/FFI and test code was cross-checked with `native-watch`
and no default features. Windows CI remains responsible for HTTP-enabled builds
and actual Windows watcher behavior.

The headless Xcode fixture is `tests/integration/KitIntegration.xcworkspace`.
It reports `root_pause_resume: true` after changing synthetic metadata while its
root is paused, verifying the returned fresh content, then shutting down cleanly.
The fixture displays no UI and does not operate Scripta! or Adobe applications.

Evidence for this pass is under `build/root-watch-pause-sqe48tj9/`, including the
full Rust and Swift logs, Xcode build/runtime output, cross-target checks and
artifact verification. The Scripta! application itself was not modified.
