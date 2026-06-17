# SCRIPTMETAKit 1.0

SCRIPTMETAKit is a Rust library and Swift package for parsing, editing, scanning, caching, and watching SCRIPTMETA-enabled script files.

The 1.0 release is intended for use by Scripta, ACEMenuPlus, and other consumer applications that need a reusable SCRIPTMETA engine across platforms.

## Package Version

- Rust crate: `scriptmetakit` `1.0.5`
- Rust FFI crate: `scriptmetakit_ffi` `1.0.5`
- Swift package product: `ScriptMetaKit`

## 1.0.5

- Fixes update diagnostics for multi-script `SCRIPTMETA-DIST` blocks when the requested `Script-ID` is missing.
- The resolver now reports the requested missing `Script-ID` instead of reusing another entry's `Script-ID`.

## License

SCRIPTMETAKit is licensed under the Apache License, Version 2.0.

See [LICENSE](LICENSE) and [NOTICE](NOTICE).

## Names

Project names and marks are not licensed as part of the source-code license.

See [TRADEMARKS.md](TRADEMARKS.md).
