# Swift application API

Version: 2.0.0, macOS Swift package.

Requires Swift 6.1 / Xcode 16.3 or later; deployment target is macOS 13 or later.

Swift exposes application operations. Rust owns metadata rules, language
selection, validation, cache decisions and change routing. The application owns
OS language acquisition, UI selection and the timing of refreshes.

## Entry points

| Task | Swift API |
|---|---|
| Coordinated roots, disk cache and watch streams | `ScriptMetaKitWorkspace` |
| Direct scan, update, edit, backup and watch operations | `ScriptMetaKitEngine` |
| Complete engine configuration | `ScriptMetaKitConfiguration.defaults()`, Engine `configuration()` / Workspace `engineConfiguration()`, `applyConfiguration(_:)` |
| Parse selected metadata from source text | `ScriptMetaKitMetadata.parse(_:preferredLanguages:requireClosedBlock:)` |
| Read all translations for editing | `ScriptMetaKitMetadata.readDraft(from:fileName:)` or Engine `readScriptMetadataDraft(fileURL:)` |
| Render metadata or replace it in source text | `ScriptMetaKitMetadata.render(_:style:)`, `write(_:into:fileName:mode:)` |
| Parse distribution records / select a script | `ScriptMetaKitMetadata.parseDistribution(_:scriptID:)` |
| Validate tags / normalize tags, language tags and URLs | `ScriptMetaKitMetadata.isValidTag`, `normalizeTags`, `parseTagList`, `normalizeLanguageTag`, `normalizeURL` |
| Invalidate caches by scope | Engine / Workspace `invalidateCache(scope:)` |
| Supply externally observed filesystem changes | Engine / Workspace `notifyChangedPaths(_:overflowed:)`, then `refreshDirtyRoots(...)` |
| Pause and reload registered folders independently | Engine / Workspace `pauseWatching(rootIDs:)`, `resumeWatching(rootIDs:)`, `isWatchingPaused(rootID:)` |
| Inspect the linked native binary | `ScriptMetaKitRuntime.linkedRuntimeInfo()` |

The existing scan, edit-session, backup, password, version, update-check,
path-inspection and native-watch APIs remain available. Existing call sites do
not have to supply languages. Without preferences, metadata defaults apply.

Physical watcher plans, internal cache payload structures and standalone
distribution resolver internals stay below this API. File-based cache load/save
and existing update checks cover the application operations; arbitrary cache
`Data` persistence is not added.

## Configuration

```swift
let engine = ScriptMetaKitEngine()
var settings = try await engine.configuration()
settings.parser.preferredLanguages = ["ja", "en", "fr"]
settings.cache.idleLifetimeMillis = 60_000
settings.cache.persistentCache = false
settings.scanner.decompileCompiledOSADuringScan = false

let change = try await engine.applyConfiguration(settings)
if change.refreshRequired {
    let refreshed = try await engine.refreshDirtyRoots()
    // Publish refreshed results to the application model.
}
```

Obtain defaults from the linked native Kit rather than creating a second set of
defaults in the application. Settings are grouped into parser, scanner, watcher,
cache and update-check options, with application ID, cache namespace and supported
extensions at the top level. The parser prefix limit is authoritative; the legacy
Rust scanner copy is synchronized when applying settings. Optional timeouts use
`nil` to disable the timeout, which crosses the ABI as explicit JSON null.

Invalid settings leave the current configuration and caches unchanged. Language
tags and extensions are canonicalized by Rust. Read back `configuration()` when
the canonical spelling matters. Setting the same canonical configuration reports
`changed == false`.

`ScriptMetaKitConfigurationChange` reports:

- `metadataInvalidated`: the old presentation is unusable under the new parser,
  language, filtering or identity settings. Reload source files.
- `refreshRequired`: registered roots need a new scan. Execution limits and
  preflight changes set this flag while retaining last-good results for a partial
  or interrupted scan.
- `persistenceChanged`: global cache/persistence enablement changed.
- `watcherChanged`: watcher or scanner/routing options changed.

Runtime configuration survives `shutdown()` and Workspace `clearVolatileState()`.
Roots, scan results and active watchers do not survive those operations.
Existing individual setters use the same Rust configuration handling.

Disabling the global watcher, or selecting its disabled/manual policy, stops the
active native watcher. Workspace also ends its current watch stream. Re-enabling
settings permits a later explicit `startWatching` / `watchUpdates` call. Other
live watcher changes remain transactional: replacement failure retains the
previous engine and watcher.

A Workspace without a cache store disables persistent retention. Its normal
memory eviction therefore cannot leave data waiting for a disk save that will
never happen. Workspace skips automatic cache reads/saves while persistence is
disabled. Engine users may supply their own file-cache lifecycle through the
existing `loadCache` / `saveCache` calls.

## Metadata and editing

```swift
let selected = try ScriptMetaKitMetadata.parse(
    source, preferredLanguages: ["ja", "en", "fr"]
)
let read = try ScriptMetaKitMetadata.readDraft(from: source, fileName: "Example.jsx")
var draft = read.draft
draft.extensions?.names["en"] = "Updated English name"
let updatedSource = try ScriptMetaKitMetadata.write(
    draft, into: source, fileName: "Example.jsx", mode: .replaceOnly
)
```

Normal `ScriptMetadata` contains only the selected Name, Author and Description,
plus common Tags and selection diagnostics. Selection falls back independently
for each field. An editing draft retains all translated values and the complete
Description body. Rewriting source preserves unknown fields and unedited content.
The text APIs neither read/write files nor execute scripts. They are synchronous;
call them on an application worker task for large source text. File-edit sessions
continue to provide source-fingerprint conflict protection and backups.

Text and file editing use the same URL/version normalization. Empty strings clear
optional values; malformed values are rejected. Scalar line breaks, outer ending
markers and ambiguous inline legacy fields are rejected before saving. Comment
output uses the existing space separation for closing delimiters (`* /`, `* )`);
AppleScript opening delimiters in text are also separated as `( *` because its
block comments nest. Plain metadata rendering preserves those comment characters.

Distribution parsing follows the existing Kit format: multiple Script-ID records
inside one distribution block. Selection preserves the core's legacy single-record
fallback. A missing ID among multiple records returns unresolved metadata with a
note; it does not invent a version for that ID.

## Cache and externally observed changes

```swift
try await engine.notifyChangedPaths(changedFileURLs, overflowed: eventQueueOverflowed)
let result = try await engine.refreshDirtyRoots(mode: .fileListAndMetadata)
```

Notification only marks changes. It uses existing root refresh policies and
alias/reference routing. Explicit refresh processes the resulting dirty roots;
neither operation runs script contents. `overflowed: true` requests reconciliation
according to the configured overflow policy.

Results identify their supplied language order in `preferredLanguages`. A consumer
with an independently buffered UI or watch stream can use it to ignore a result
from an earlier language selection while publishing its refreshed result.

Engine `invalidateCache` clears the requested in-memory/pending scope, marks roots
for full refresh and preserves registered watcher topology. It does not delete
files. Workspace invalidation additionally removes its cache files containing
that scope and cancels queued saves that could restore invalidated data. A combined
`.all` cache file is removed when either constituent scope is invalidated.

Cache schema remains 4; metadata projection version is now 4. Cache validation
includes canonical language order and the metadata read context (prefix/closed
block rules, file filters, reference handling, compiled OSA inspection and supported
extensions). Old projection/context mismatches require a source reload. Runtime
time/node budgets do not make last-good metadata intrinsically invalid.

## Root-scoped watch suspension

```swift
try await workspace.pauseWatching(rootIDs: repositoryRootIDs)
// The application now updates its files. Keep execution and explicit source
// scans for these roots disabled until the update and reload have succeeded.
let result = try await workspace.resumeWatching(
    rootIDs: repositoryRootIDs, cacheScope: .all
)
if result.roots.allSatisfy({ $0.status == "ready" }) {
    // Publish result and make the application actions available again.
}
```

Both methods wait for an exclusive Kit operation. Once pause returns, native
events, watcher overflow and initial reconciliation cannot automatically read
the specified roots. The existing update stream remains open and other roots
continue watching. If all roots are paused, no filesystem roots are monitored;
the stream can still be resumed. Existing cached content is retained.

Resume arms the running native watcher before rereading the entire requested
file trees and metadata. It bypasses unchanged-record reuse, including records
with identical file size, modification time and identity. It returns the reload
result, performs no Meta-URL/network checks and optionally saves Workspace caches.
Missing, unreadable, cancelled or incomplete root results keep the entire
requested batch paused; retry explicitly after resolving the issue. Native
reconfiguration failure also retains suspension. Inspect the result before
enabling application execution.

The stream publishes already reloaded, resident snapshots without repeating their
source scan solely for reconciliation. Actual queued filesystem changes always
invalidate those roots, even if size and timestamps match. A changed watch topology
or evicted/invalid cache requires a fresh reconciliation. Progressive delivery
honors the same invalidation rules. Root control retains IDs and prepares the new
watch plan; it does not clone the catalog's per-file indexes.

Cancellation is checked before scheduling a native worker. Once scheduled, the
worker is joined before releasing Workspace serialization. Publication also checks
the watch-control generation: a read completed before a pause/resume cannot later
replace the new result. Native watcher retirement waits for callbacks outside the
engine mutex, allowing a C callback to query the engine while a watch is replaced.

Suspension is idempotent, not reference counted. Empty ID lists and unknown IDs
fail without changes. Duplicate IDs are accepted. Resume reapplies the configured
watch/refresh policies and does not start a globally stopped watcher. A disabled
or manual watch policy therefore remains disabled or manual after the reload.
`isWatchingPaused` reports the suspension state and returns false for unknown IDs.

The scope is a registered `rootID`, not an exclusive lock on a filesystem path.
If file-list and metadata registrations refer to the same repository, suspend
all of their IDs together. Another unpaused registration covering the same path
can still read it. Explicit scans and script execution remain the application's
responsibility. Already buffered application results must also respect its
update-in-progress state.

Suspension survives root-group updates for the same ID, configuration changes
and cache invalidation. Removing a registration clears its suspension. Shutdown
and `clearVolatileState` clear all roots and suspension state. Suspension is not
written to the persistent catalog. The C equivalents are
`smk_engine_pause_root_watching`, `smk_engine_resume_root_watching` (free the
returned scan result), and `smk_engine_is_root_watching_paused`.

## Ownership and packaging

The hot scan path retains its typed C ABI, interned native strings and Swift
storage sharing. The less frequent configuration/text operations use owned UTF-8
or JSON results. Swift borrows input bytes for the native call, makes one owned
byte copy for JSON decoding, and frees every returned native handle on success
and error. It does not convert JSON through an intermediate Swift String.

C clients must free every non-null text-service `SmkEditResult`, including error
responses, with `smk_edit_result_free`. Existing C struct layouts and functions
remain available. Swift source must be linked with its matching native
binary. `linkedRuntimeInfo()` reports the loaded
binary's version, architecture and enabled native-watch/HTTP features.

Build an isolated Apple Silicon SDK with `script/build_arm64_test_package.py`.
See [2.0 release validation](Release2.0Validation.md) and
[macOS integration](MacOSIntegration-2.0.md) for the current package and results.
