import XCTest
@testable import ScriptMetaKit

final class TagsLocalizationTests: XCTestCase {
    private let source = """
    /*
    SCRIPTMETA-BEGIN
    Script-ID=com.example.swift.localization
    Meta-Default-language=de
    Name=German name
    Name[ja]=日本語名
    Name[en]=English name
    Name[fr]=Nom français
    Author=Author
    Tags=画像,image,
    export,
    Tags=image,print
    Description-BEGIN
    [de]
    German description.
    [fr]
    French description.
    Tags=literal-example
    Description-END
    SCRIPTMETA-END
    */
    """

    private func fixture() throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("ScriptMetaLanguages-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try source.write(to: root.appendingPathComponent("example.jsx"), atomically: true, encoding: .utf8)
        return root
    }

    func testConsumerSuppliedPriorityAndPresentationInEveryView() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let engine = ScriptMetaKitEngine()
        try await engine.setPreferredLanguages(["ja", "en", "fr"])
        let result = try await engine.scan(folderURLs: [root], checkUpdates: false)
        XCTAssertEqual(result.preferredLanguages, ["ja", "en", "fr"])
        let catalog = try XCTUnwrap(result.catalogSnapshot)
        let items = catalog.allItems + catalog.fileItems + catalog.candidateCache.records.compactMap(\.item)
            + result.flattenedFileEntries.compactMap(\.scriptMetaItem)
        XCTAssertEqual(items.count, 4)
        for item in items {
            XCTAssertEqual(item.name, "日本語名")
            XCTAssertEqual(item.presentation.nameLanguage, "ja")
            XCTAssertEqual(item.presentation.descriptionLanguage, "fr")
            XCTAssertEqual(item.itemDescription, "French description.\nTags=literal-example")
            XCTAssertEqual(item.tags, ["画像", "image", "export", "print"])
        }
        let encoded = try JSONEncoder().encode(result)
        let decoded = try JSONDecoder().decode(ScriptMetaScanResult.self, from: encoded)
        XCTAssertEqual(decoded.preferredLanguages, result.preferredLanguages)
        XCTAssertEqual(decoded.allItems.first?.tags, result.allItems.first?.tags)
    }

    func testLanguageChangeReloadsAndSurvivesVolatileReset() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let registration = ScriptMetaKitRoot(rootID: "language", url: root, cachePolicy: .memoryOnly)
        let workspace = ScriptMetaKitWorkspace(configuration: .init(preferredLanguages: ["ja", "en", "fr"]))
        let first = try await workspace.scanRoots([registration], replacingGroup: "test", rootIDs: [registration.rootID], mode: .fileListAndMetadata)
        XCTAssertEqual(first.allItems.first?.name, "日本語名")
        try await workspace.setPreferredLanguages(["en", "fr"])
        let second = try await workspace.scanRoots([registration], replacingGroup: "test", rootIDs: [registration.rootID], mode: .fileListAndMetadata)
        XCTAssertEqual(second.allItems.first?.name, "English name")
        await workspace.clearVolatileState()
        let third = try await workspace.scanRoots([registration], replacingGroup: "test", rootIDs: [registration.rootID], mode: .fileListAndMetadata)
        XCTAssertEqual(third.preferredLanguages, ["en", "fr"])
        XCTAssertEqual(third.allItems.first?.name, "English name")
    }

    func testRepeatedTextIsJoinedAndDiagnosticsCrossTheABI() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let repeated = source.replacingOccurrences(of: "Name[en]=English name", with: "Name[en]=Same\nName[EN]=Same")
            .replacingOccurrences(of: "[fr]\nFrench description.\nTags=literal-example", with: "[fr]Same\n[FR]Same")
        try repeated.write(to: root.appendingPathComponent("example.jsx"), atomically: true, encoding: .utf8)
        let engine = ScriptMetaKitEngine()
        try await engine.setPreferredLanguages(["en", "fr"])
        let result = try await engine.scan(folderURLs: [root], checkUpdates: false)
        let item = try XCTUnwrap(result.allItems.first)
        XCTAssertEqual(item.name, "Same Same")
        XCTAssertEqual(item.itemDescription, "Same\n\nSame")
        XCTAssertTrue(item.presentation.diagnostics.contains { $0.field == "Name" && $0.language == "en" && $0.line != nil })
        XCTAssertTrue(item.presentation.diagnostics.contains { $0.field == "Description" && $0.language == "fr" })
    }

    func testEditingPreservesAllLanguagesAndClearsEveryTagsOccurrence() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("example.jsx")
        let engine = ScriptMetaKitEngine()
        try await engine.setPreferredLanguages(["en", "fr"])
        let read = try await engine.readScriptMetadataDraft(fileURL: file)
        var draft = read.draft
        var extensions = try XCTUnwrap(draft.extensions)
        XCTAssertEqual(extensions.defaultLanguage, "de")
        XCTAssertEqual(extensions.names["ja"], "日本語名")
        extensions.names["en"] = "Edited English"
        extensions.tags = []
        draft.extensions = extensions
        _ = try await engine.writeScriptMetadata(fileURL: file, draft: draft, mode: .replaceOnly,
                                                expectedSourceFingerprint: read.sourceFingerprint)
        let reread = try await engine.readScriptMetadataDraft(fileURL: file)
        XCTAssertEqual(reread.draft.extensions?.names["fr"], "Nom français")
        XCTAssertEqual(reread.draft.extensions?.names["en"], "Edited English")
        XCTAssertEqual(reread.draft.extensions?.tags, [])
        XCTAssertTrue(reread.draft.itemDescription?.contains("German description.") == true)
        XCTAssertTrue(reread.draft.itemDescription?.contains("French description.") == true)
        XCTAssertTrue(reread.draft.itemDescription?.contains("Tags=literal-example") == true)
    }

    func testUnlabeledDescriptionPreambleUsesMetadataDefaultAcrossLanguageChanges() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("example.jsx")
        let implicit = source.replacingOccurrences(of: "[de]\nGerman description.", with: "German description.")
        try implicit.write(to: file, atomically: true, encoding: .utf8)
        let engine = ScriptMetaKitEngine()
        for (languages, expected, selected) in [
            ([String](), "German description.", "de"),
            (["ja", "en", "fr"], "French description.\nTags=literal-example", "fr"),
            (["de"], "German description.", "de")
        ] {
            try await engine.setPreferredLanguages(languages)
            let result = try await engine.scan(folderURLs: [root], checkUpdates: false)
            let item = try XCTUnwrap(result.allItems.first)
            XCTAssertEqual(item.itemDescription, expected)
            XCTAssertEqual(item.presentation.descriptionLanguage, selected)
            XCTAssertTrue(item.presentation.diagnostics.isEmpty)
        }
        let read = try await engine.readScriptMetadataDraft(fileURL: file)
        XCTAssertEqual(read.draft.itemDescription, "German description.\n[fr]\nFrench description.\nTags=literal-example")
    }
}
