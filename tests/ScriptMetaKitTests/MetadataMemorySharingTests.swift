import XCTest
@testable import ScriptMetaKit

final class MetadataMemorySharingTests: XCTestCase {
    private let tags = ["shared_category_first_long_label", "shared_category_second_long_label", "shared_category_third_long_label"]

    private func fixture(repeatedName: Bool = false) throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("ScriptMetaSharing-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        for index in 0..<2 {
            let repeated = repeatedName ? "Name[en]=Same\nName[en]=Same\n" : ""
            let source = """
            /*
            SCRIPTMETA-BEGIN
            Script-ID=com.example.sharing.\(index)
            Meta-Default-language=ja
            Name=Sharing test \(index)
            \(repeated)Tags=\(tags.joined(separator: ","))
            Description-BEGIN
            [ja]JA_\(index)_\(String(repeating: "a", count: 2048))
            [en]EN_\(index)_\(String(repeating: "b", count: 2048))
            Description-END
            SCRIPTMETA-END
            */
            """
            try source.write(to: root.appendingPathComponent("\(index).jsx"), atomically: true, encoding: .utf8)
        }
        return root
    }

    private func references(_ result: ScriptMetaScanResult) throws -> [ScriptMetaItem] {
        let catalog = try XCTUnwrap(result.catalogSnapshot)
        return catalog.allItems + catalog.fileItems
            + catalog.candidateCache.records.compactMap(\.item)
            + result.flattenedFileEntries.compactMap(\.scriptMetaItem)
    }

    private func stringStorage(_ value: String) -> UInt {
        value.utf8.withContiguousStorageIfAvailable { UInt(bitPattern: $0.baseAddress!) }!
    }

    private func arrayStorage<T>(_ value: [T]) -> UInt {
        value.withUnsafeBufferPointer { UInt(bitPattern: $0.baseAddress!) }
    }

    func testStringsAndTagArraysAreSharedAcrossResultCollections() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let engine = ScriptMetaKitEngine()
        try await engine.setPreferredLanguages(["ja", "en", "fr"])
        let result = try await engine.scan(folderURLs: [root], checkUpdates: false)
        let items = try references(result)
        XCTAssertEqual(items.count, 8)
        for group in Dictionary(grouping: items, by: \.filePath).values {
            XCTAssertEqual(Set(group.map { stringStorage($0.itemDescription!) }).count, 1)
            XCTAssertEqual(Set(group.map { arrayStorage($0.tags) }).count, 1)
            XCTAssertTrue(group.allSatisfy { $0.tags == tags })
        }
        XCTAssertEqual(Set(items.flatMap(\.tags).map(stringStorage)).count, tags.count)
        withExtendedLifetime(result) {}
    }

    func testSharedValuesRemainIndependentAcrossChangesAndResultLifetimes() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let engine = ScriptMetaKitEngine()
        try await engine.setPreferredLanguages(["ja"])
        let first = try await engine.scan(folderURLs: [root], checkUpdates: false)
        let original = try XCTUnwrap(first.allItems.first)
        let originalDescription = try XCTUnwrap(original.itemDescription)
        var changed = original
        var presentation = changed.presentation
        presentation.tags.append("only-this-value")
        changed.presentation = presentation
        XCTAssertEqual(original.tags, tags)
        XCTAssertNotEqual(changed.tags, original.tags)
        try await engine.setPreferredLanguages(["en"])
        let second = try await engine.scan(folderURLs: [root], checkUpdates: false)
        XCTAssertTrue(second.allItems.allSatisfy { $0.itemDescription?.hasPrefix("EN_") == true })
        await engine.shutdown()
        XCTAssertEqual(first.allItems.first?.itemDescription, originalDescription)
        XCTAssertTrue(originalDescription.hasPrefix("JA_"))
    }

    func testDiagnosticArraysAreSharedWithinEachMetadataItem() async throws {
        let root = try fixture(repeatedName: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let engine = ScriptMetaKitEngine()
        try await engine.setPreferredLanguages(["en"])
        let result = try await engine.scan(folderURLs: [root], checkUpdates: false)
        let items = try references(result)
        for group in Dictionary(grouping: items, by: \.filePath).values {
            XCTAssertTrue(group.allSatisfy { !$0.presentation.diagnostics.isEmpty })
            XCTAssertEqual(Set(group.map { arrayStorage($0.presentation.diagnostics) }).count, 1)
        }
        withExtendedLifetime(result) {}
    }
}
