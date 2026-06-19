# SCRIPTMETAKit 1.0

SCRIPTMETAKit is a Rust library and Swift package for parsing, editing, scanning, caching, and watching SCRIPTMETA-enabled script files.

The 1.0 release is intended for use by Scripta, ACEMenuPlus, and other consumer applications that need a reusable SCRIPTMETA engine across platforms.

## Package Version

- Rust crate: `scriptmetakit` `1.0.8`
- Rust FFI crate: `scriptmetakit_ffi` `1.0.8`
- Swift package product: `ScriptMetaKit`

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

## License

SCRIPTMETAKit is licensed under the Apache License, Version 2.0.

See [LICENSE](LICENSE) and [NOTICE](NOTICE).

## Names

Project names and marks are not licensed as part of the source-code license.

See [TRADEMARKS.md](TRADEMARKS.md).
