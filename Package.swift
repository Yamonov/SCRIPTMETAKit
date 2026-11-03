// swift-tools-version: 6.1
import PackageDescription
import Foundation

let package = Package(
    name: "ScriptMetaKit",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .library(
            name: "ScriptMetaKit",
            targets: ["ScriptMetaKit"]
        )
    ],
    targets: [
        .binaryTarget(
            name: "ScriptMetaKitFFI",
            path: ProcessInfo.processInfo.environment["SCRIPT_META_KIT_FFI_PATH"] ?? "Artifacts/ScriptMetaKitFFI.xcframework"
        ),
        .target(
            name: "ScriptMetaKit",
            dependencies: ["ScriptMetaKitFFI"],
            resources: [
                .process("Resources")
            ]
        ),
        .testTarget(
            name: "ScriptMetaKitTests",
            dependencies: ["ScriptMetaKit"],
            path: "tests/ScriptMetaKitTests"
        )
    ],
    swiftLanguageModes: [.v5, .v6]
)
