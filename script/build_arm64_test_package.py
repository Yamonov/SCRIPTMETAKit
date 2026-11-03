#!/usr/bin/env python3
"""Build an isolated arm64-only SDK/test package without replacing release artifacts."""
from pathlib import Path
import hashlib
import json
import platform
import plistlib
import shutil
import subprocess
import tempfile

from build_support import build_environment, source_tree_sha256

ROOT = Path(__file__).resolve().parents[1]


def run(*arguments, **kwargs):
    return subprocess.run(arguments, check=True, **kwargs)


def main():
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise SystemExit("Run this test build on Apple Silicon macOS.")
    environment = build_environment(ROOT)
    run("cargo", "build", "--locked", "--release", "--target", "aarch64-apple-darwin",
        "-p", "scriptmetakit_ffi", "--features", "blocking-http,native-watch", cwd=ROOT, env=environment)

    output_root = ROOT / "build" / "arm64-tests"
    output_root.mkdir(parents=True, exist_ok=True)
    package = Path(tempfile.mkdtemp(prefix="localization-", dir=output_root))
    framework = package / "ScriptMetaKitFFI.xcframework"
    library = framework / "macos-arm64"
    headers = library / "Headers"
    headers.mkdir(parents=True)
    dylib = library / "libscriptmetakit_ffi.dylib"
    shutil.copy2(ROOT / "target/aarch64-apple-darwin/release/libscriptmetakit_ffi.dylib", dylib)
    shutil.copy2(ROOT / "scriptmetakit_ffi/include/scriptmetakit_ffi.h", headers)
    (headers / "module.modulemap").write_text('module ScriptMetaKitFFI {\n  header "scriptmetakit_ffi.h"\n  export *\n}\n')
    run("install_name_tool", "-id", "@rpath/libscriptmetakit_ffi.dylib", str(dylib))
    run("strip", "-S", "-x", str(dylib))
    run("codesign", "--force", "--sign", "-", str(dylib))
    architectures = subprocess.check_output(["lipo", "-archs", str(dylib)], text=True).strip().split()
    if architectures != ["arm64"]:
        raise SystemExit(f"Unexpected architectures: {architectures}")
    run("codesign", "--verify", "--strict", str(dylib))
    with (framework / "Info.plist").open("wb") as stream:
        plistlib.dump({"CFBundlePackageType": "XFWK", "XCFrameworkFormatVersion": "1.0",
                      "AvailableLibraries": [{"LibraryIdentifier": "macos-arm64", "LibraryPath": dylib.name,
                                               "HeadersPath": "Headers", "SupportedArchitectures": ["arm64"],
                                               "SupportedPlatform": "macos"}]}, stream)
    shutil.copytree(ROOT / "Sources", package / "Sources")
    shutil.copytree(ROOT / "tests/ScriptMetaKitTests", package / "tests/ScriptMetaKitTests")
    (package / "Package.swift").write_text('''// swift-tools-version: 6.1
import PackageDescription
let package = Package(name: "ScriptMetaKit", platforms: [.macOS(.v13)],
    products: [.library(name: "ScriptMetaKit", targets: ["ScriptMetaKit"])],
    targets: [
        .binaryTarget(name: "ScriptMetaKitFFI", path: "ScriptMetaKitFFI.xcframework"),
        .target(name: "ScriptMetaKit", dependencies: ["ScriptMetaKitFFI"], resources: [.process("Resources")]),
        .testTarget(name: "ScriptMetaKitTests", dependencies: ["ScriptMetaKit"], path: "tests/ScriptMetaKitTests")
    ], swiftLanguageModes: [.v5, .v6])
''')
    manifest = {
        "testingOnly": True, "architectures": architectures, "target": "aarch64-apple-darwin",
        "features": ["blocking-http", "native-watch"],
        "gitRevision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "sourceTreeSHA256": source_tree_sha256(ROOT),
        "sourceHashFormatVersion": 2,
        "sourceHashScope": "build inputs, source, tests and fixtures, resources and CI; excludes documentation",
        "dylibSHA256": hashlib.sha256(dylib.read_bytes()).hexdigest(),
    }
    (package / "test-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"ARM64_TEST_PACKAGE={package}")


if __name__ == "__main__":
    main()
