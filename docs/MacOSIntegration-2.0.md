# SCRIPTMETAKit 2.1 — macOS application integration

Package version **2.1.0**. C ABI generation remains **1**; existing C layouts and
Swift entry points are retained. The SDK is distributed as Swift source plus a
matching Universal native XCFramework.

## Requirements

- macOS deployment target **13.0 or later**.
- **Swift 6.1 / Xcode 16.3 or later**. Swift 5 and Swift 6 language modes are supported.
- `Artifacts/ScriptMetaKitFFI.xcframework` from the same source/version as the SDK.

The minimum compiler is explicit because the SDK uses type-level `nonisolated`,
introduced in Swift 6.1. This requirement is distinct from the macOS deployment
target. See [Swift 6.1's official release notes](https://www.swift.org/blog/swift-6.1-released/).

## Add the package

Add this repository as an Xcode package dependency, then add the `ScriptMetaKit`
product to the app target. For local release-candidate testing, use the repository
directory containing `Package.swift`.

```swift
import ScriptMetaKit

let workspace = ScriptMetaKitWorkspace(configuration: .init(
    preferredLanguages: ["ja", "en", "fr"]
))
```

The ordered languages above are test inputs. In a consumer application, the app
obtains and supplies its OS/user preferences. Kit never reads OS language settings.
Existing callers may omit the language argument and use the metadata default.

SwiftPM/Xcode links the Rust dylib and embeds it in the application bundle. Keep
the normal macOS app runpath when overriding build settings with custom xcconfig:

```text
LD_RUNPATH_SEARCH_PATHS = $(inherited) @executable_path/../Frameworks
COPY_PHASE_STRIP = NO
ALWAYS_SEARCH_USER_PATHS = NO
```

`COPY_PHASE_STRIP=NO` avoids attempting to strip the already stripped/signed native
library again. App signing signs the embedded copy using the app's signing
configuration. The native library's install name is
`@rpath/libscriptmetakit_ffi.dylib`; no machine-specific library path is required.

A successful link is insufficient: verify the final `.app` contains the dylib in
`Contents/Frameworks`, includes `ScriptMetaKit_ScriptMetaKit.bundle` resources, has
the Frameworks runpath and passes code-signature verification. The test project
below exercises the actual loader and resource access.

## App/Kit responsibilities

- The app owns UI presentation, root selection, security-scoped access/bookmarks,
  OS language acquisition and the decision to execute a script.
- Kit owns parsing, validation, per-field language fallback, file/reference
  inspection, metadata editing, cache rules, update resolution and watch routing.
- Normal results retain selected metadata only. Editing starts from a fresh full
  draft/session so other translations are preserved.
- Language/read-setting changes require a new scan. `preferredLanguages` on each
  result identifies its selection context; independently buffered UI work should
  not replace a newer selection with an older result.
- Watch updates use native notifications. `drainWatchChanges` is an explicit,
  bounded drain operation, not a background polling service.
- `shutdown()` or `clearVolatileState()` explicitly ends sessions. Releasing a
  Workspace also finishes its stream and cancels pending delayed saves.

## Cache and resource policy

A Workspace without a persistent cache store does not retain data waiting for a
disk save. A Workspace with a store uses revision-only queries for housekeeping;
it does not convert complete script descriptions and file trees just to compare
their revisions. Cache adoption and provenance bookkeeping finish within one
operation.

`updateCheck.cacheNetworkResponses` controls retention of bounded HTTP validators
and extracted bodies between update operations. Its default is **true**, matching
the earlier implementation's actual conditional-revalidation behavior. Set it to
false to avoid that retention. Disabling global memory caching also disables this
cross-operation HTTP retention; sharing one source within a single update batch
remains enabled.

Idle cache expiry is evaluated when the engine is accessed. It does not install a
background timer. Configure memory caching or explicitly end/release the workspace
when the app needs to release its state immediately. Caller-retained results remain
owned by the caller, independently of the engine.

Schema, language and read-context incompatibility is an expected cache miss. C
cache-load APIs return `SMK_STATUS_CACHE_INCOMPATIBLE` (**7**); the Swift Engine
preserves its existing `operationFailed(status, message)` error shape. Workspace
silently discards the incompatible derived file and allows source reload. Corrupt
data and actual I/O failures still produce optional diagnostics; cancellation
neither deletes cache files nor emits a cache-failure diagnostic.

## Reproducible Xcode consumer

`tests/integration/KitIntegration.xcworkspace` contains a minimal sandboxed
application that imports only the public SDK API. It has no window or UI tests.
Its executable validates native version/features, resource embedding, language
selection, cache restoration, update checks, watch delivery and shutdown.

```sh
xcodebuildmcp macos build \
  --workspace-path "$PWD/tests/integration/KitIntegration.xcworkspace" \
  --scheme KitIntegration --configuration Release --arch arm64 \
  --derived-data-path "$PWD/build/kit-integration" --prefer-xcodebuild true

codesign --verify --deep --strict \
  build/kit-integration/Build/Products/Release/KitIntegration.app

build/kit-integration/Build/Products/Release/KitIntegration.app/Contents/MacOS/KitIntegration
```

Add `--benchmark` to the executable invocation for 1,000 synthetic scripts with
four language sections, 4 KiB per section, and a 10-second idle observation.
Fixtures are created in the test app's temporary directory and removed afterward.
The script contents are never executed. The benchmark reports process CPU, RSS,
footprint and wakeups; these are whole-process measurements, not a claim that all
resident bytes belong to Kit.

Build with `--arch x86_64` and a separate derived-data directory to test the Intel
consumer. On Apple Silicon, run that executable through `arch -x86_64` when
Rosetta is available. This establishes compatibility, not native Intel performance.

## Release artifacts and Windows

Run `script/build_xcframework.sh` after Rust/C changes. It stages both architectures,
verifies every public C export, Mach-O layout, install name, system dependencies,
deployment target, native version/capabilities and available-CPU execution, then
replaces the artifact. The manifest records source state and source/native-binary hashes.
Builds default to ad-hoc signing for CI/local development. For distribution, set
`SCRIPTMETAKIT_SIGNING_IDENTITY` to the intended Developer ID identity; the script
signs and verifies both the native dylib and the enclosing XCFramework. The 2.0.0
candidate was signed with the same Developer ID publisher as 1.3.3.

Source fingerprint format 2 includes test fixtures/CI and uses framed names plus
individual file hashes; documentation is excluded.

Windows uses `script/build_windows.ps1 -Release` with `x86_64-pc-windows-msvc` and
the same `blocking-http,native-watch` features. Windows app wrappers consume the C
ABI; the macOS Swift package contains a macOS binary. Native Windows DLL execution
and application integration remain a separate validation step.
