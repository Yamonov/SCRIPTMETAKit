# Root watch pause/resume review

Date: 2026-11-03. Version: 2.0.0. Branch: `codex/tags-localization`.

## Findings and repairs

| Finding | Repair and evidence |
|---|---|
| Resume read its roots, then synthetic reconciliation scanned them again | Track the registered IDs whose fresh snapshots remain usable. Real notifications are processed independently. The native snapshot-identity regression fails before the repair and passes for complete and progressive delivery afterward |
| Progressive reconciliation reused a record after a same-size/same-timestamp change | Refresh the selected batch with its dirty scopes. Reintroducing the former scan call returns `One` instead of `Two`; the fixed path returns `Two` |
| A full resume probed headers in the file-list pass and parsed the files again in metadata | Defer header probing to the metadata pass whenever both are requested; metadata still forces a fresh read on resume |
| A completed watch result could be published after an intervening pause/resume | Validate generation and publish in one Workspace actor turn. A controlled publication barrier reproduces old `1.0` results after a `2.0` resume when the guard is disabled. Stale results do not consume sequence numbers |
| Cancellation was checked after creating a detached worker, allowing an early return during hand-off | Check before scheduling and always await an existing worker. Tests cover already-cancelled calls, cancellation at native hand-off, batch suspension, retry and concurrent disjoint operations |
| Root control cloned the engine's per-file indexes | Validate and prepare only the watch plan and suspension changes before committing. Allocation tracking shows constant cost with 10 and 1,000 cached files |
| Replacing a watcher waited for callbacks while holding the engine mutex | Retire the old watcher under the mutex, then unlock before joining it. A bounded callback-query regression fails before the repair and succeeds afterward. Free also stops callbacks before destroying engine state |

The new reconciliation bookkeeping contains root IDs only. It is pruned when
registrations change, cleared when monitoring stops, and not accumulated when
resume is used without an active watcher. It does not retain extra source trees.
Cached-read exemptions are discarded after topology changes, cache invalidation
or eviction, and cannot suppress a real change notification.

## Allocation measurement

The fixture scans 10 or 1,000 synthetic scripts, then measures heap allocations
made by `pause` on the calling thread. Native watching has not been started, so
OS watcher creation and worker-thread allocations are outside this measurement.

| Cached scripts | Before | After |
|---|---:|---:|
| 10 | 5,031 bytes | 344 bytes |
| 1,000 | 129,055 bytes | 344 bytes |

These are bytes allocated during the operation, not process RSS or retained memory.
The fixed 344-byte result was also verified in an optimized release build.

## Final verification

| Check | Result |
|---|---|
| Rust/C, all features, macOS arm64 | 378 passed |
| Swift 6 full suite, macOS arm64 | 99 passed, no skips |
| Swift 5 language-mode full suite, macOS arm64 | 99 passed, no skips |
| Xcode Release client, arm64 and x86_64 under Rosetta | Built and ran; pause/resume succeeded, no stderr or diagnostics |
| Universal XCFramework | Rebuilt with Developer ID signatures; 134 C exports, native linking and lifecycle checked for both CPUs |
| Formatting and all-target/all-feature Clippy | Passed with warnings denied |
| No-default-feature focused regression suites | Passed |
| Windows MSVC native-watch, all targets | Cross-target Clippy passed; Windows execution was not performed |

The artifact manifest matches the final source fingerprint and native-library
SHA-256. `build/watch-api-review-ky485zoc/verification.json` records the final
checks. No Scripta! application changes, commit or publication were performed.

## Additional checks and scope

The node-limit case was initially reported as a failure-propagation defect.
Closer inspection and the corrected regression established that the existing
caller already preserves failed/incomplete file-list status. No change was
retained in the metadata scanner for that claim. The new test verifies that the
root stays paused and a later retry reads all files.

Other regression cases cover multiple and overlapping registrations, duplicate
pause calls, a delayed publication for another root, all roots paused, native
replacement failure, cached-result publication, and post-resume invalidation.
Tests use temporary synthetic files and controlled synchronization points. They
do not operate Scripta! or Adobe applications.

Evidence is in `build/watch-api-review-ky485zoc/`. Reproduction logs distinguish
observed before/after failures from the cancellation control-flow review. The
API signatures and C ABI layouts remain compatible with the initial 2.0 addition.
