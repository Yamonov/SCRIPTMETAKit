// swift-tools-version: 6.1
import Foundation
import PackageDescription

let languageMode: SwiftLanguageMode =
    ProcessInfo.processInfo.environment["SMK_TEST_SWIFT_LANGUAGE_MODE"] == "5" ? .v5 : .v6

let package = Package(
    name: "ScriptMetaKitSwiftRegression",
    platforms: [.macOS(.v13)],
    dependencies: [.package(path: "../..")],
    targets: [
        .executableTarget(
            name: "SwiftAPIRegression",
            dependencies: [.product(name: "ScriptMetaKit", package: "SCRIPTMETAKit")]
        )
    ],
    swiftLanguageModes: [languageMode]
)
