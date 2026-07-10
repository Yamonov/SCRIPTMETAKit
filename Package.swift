// swift-tools-version: 5.9
import PackageDescription

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
            path: "Artifacts/ScriptMetaKitFFI.xcframework"
        ),
        .target(
            name: "ScriptMetaKit",
            dependencies: ["ScriptMetaKitFFI"]
        ),
        .testTarget(
            name: "ScriptMetaKitTests",
            dependencies: ["ScriptMetaKit"],
            path: "tests/ScriptMetaKitTests"
        )
    ]
)
