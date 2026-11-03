# Headless package integration fixture

Open `KitIntegration.xcworkspace` or build its `KitIntegration` scheme. The app
uses the root repository's `ScriptMetaKit` package, standard Frameworks runpath,
Swift 6 strict concurrency/MainActor default, ad-hoc signing and App Sandbox.

Execute `KitIntegration.app/Contents/MacOS/KitIntegration` directly. It creates
temporary synthetic JSX and distribution metadata, exercises public APIs, emits
JSON results and exits. It never displays a window or runs the script contents.
`--benchmark` selects 1,000 files and a 10-second idle sample instead of the small
smoke fixture.

Detailed commands and interpretation are in
[MacOSIntegration-2.0.md](../../docs/MacOSIntegration-2.0.md).
