import XCTest
@testable import ScriptMetaKit

final class CustomMetadataTests: XCTestCase {
    private let source = """
    /*
    SCRIPTMETA-BEGIN
    Script-ID=example.swift.custom
    Meta-Default-language=ja
    Name=日本語
    Name[en]=English
    Tags=image,
    X-org.iwashi-Group=Production Tools
    X-ORG.IWASHI-GROUP=Production Tools
    X-org.iwashi-Order=0020
    X-org.iwashi-URL=https://example.com/my%20scripts?Name=English&Version=2.0
    X-org.iwashi-Empty=
    X-Custom=legacy
    Unknown-Key=keep
    Description-BEGIN
    [ja]説明
    [en]Description
    X-org.iwashi-Inside=body only
    Description-END
    SCRIPTMETA-END
    */
    """

    private func fixture() throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("ScriptMetaCustom-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try source.write(to: root.appendingPathComponent("test.jsx"), atomically: true, encoding: .utf8)
        return root
    }

    func testTextReadsSelectKeysIndependentlyOfLanguage() throws {
        let ordinary = try ScriptMetaKitMetadata.parse(source)
        XCTAssertTrue(ordinary.customFields.isEmpty)
        XCTAssertEqual(ordinary.tags, ["image"])
        let selected = try ScriptMetaKitMetadata.parse(source, preferredLanguages: ["en"], customMetadataKeys: ["x-ORG.IWASHI-GROUP"])
        XCTAssertEqual(selected.name, "English")
        XCTAssertEqual(selected.customFields.count, 1)
        XCTAssertEqual(selected.customValues(for: "X-org.iwashi-Group"), ["Production Tools", "Production Tools"])
        let all = try ScriptMetaKitMetadata.parse(source, customMetadataKeys: ["X-"])
        XCTAssertEqual(all.customFields.count, 4)
        XCTAssertEqual(all.customValues(for: "X-org.iwashi-Empty"), [""])
        XCTAssertEqual(all.customValues(for: "X-org.iwashi-Order"), ["0020"])
        XCTAssertNil(all.customValues(for: "X-org.iwashi-Inside"))
        XCTAssertTrue(all.presentation.diagnostics.contains { $0.field == "X-Custom" })
    }

    func testInputValidationReturnsStructuredIssues() throws {
        XCTAssertEqual(try ScriptMetaKitMetadata.normalizeCustomMetadataKey("x-ORG.iwashi-Group"), "X-org.iwashi-group")
        XCTAssertTrue(try ScriptMetaKitMetadata.validateCustomMetadataField(key: "X-org.iwashi-URL", value: "https://example.com/?Name=A&Version=2").isEmpty)
        let issues = try ScriptMetaKitMetadata.validateCustomMetadataField(key: "X-org.iwashi-Note", value: "\"Note Name=Injected\"")
        XCTAssertEqual(issues.first?.code, "custom_metadata_legacy_field")
        XCTAssertEqual(issues.first?.field, "X-org.iwashi-Note")
        XCTAssertFalse(try ScriptMetaKitMetadata.validateCustomMetadataField(key: "X-org..iwashi-Name", value: "value").isEmpty)
        XCTAssertThrowsError(try ScriptMetaKitMetadata.parse(source, customMetadataKeys: ["X-*"]))
    }

    func testHostCommentDelimitersAreRejectedWithoutRewritingValues() throws {
        var draft = ScriptMetadataDraft(scriptID: "example.comments")
        try draft.setCustomValues(["https://example.com/*/image"], for: "X-org.iwashi-URL")
        XCTAssertThrowsError(try ScriptMetaKitMetadata.render(draft, style: .javaScriptBlock))
        XCTAssertTrue(try ScriptMetaKitMetadata.render(draft).contains("https://example.com/*/image"))
        try draft.setCustomValues(["literal (* opening"], for: "X-org.iwashi-URL")
        XCTAssertThrowsError(try ScriptMetaKitMetadata.render(draft, style: .appleScriptBlock))
    }

    func testLegacyDraftJSONDefaultsToPreservingCustomFields() throws {
        let draft = try JSONDecoder().decode(ScriptMetadataDraft.self, from: Data("{\"script_id\":\"example.legacy\"}".utf8))
        XCTAssertTrue(draft.customFields.isEmpty)
    }

    func testManuallyConstructedCustomFieldsRemainCaseInsensitive() throws {
        let presentation = MetadataPresentation(customFields: ["x-ORG.IWASHI-Order": ["0020"]])
        XCTAssertEqual(presentation.customValues(for: "X-org.iwashi-order"), ["0020"])
        var draft = ScriptMetadataDraft(scriptID: "example.manual", customFields: [
            "x-ORG.IWASHI-Order": ["0020"],
            "X-org.iwashi-ORDER": ["old alias"],
            "X-org.iwashi-Group": ["keep"]
        ])
        try draft.setCustomValues(["0030"], for: "X-org.iwashi-Order")
        XCTAssertEqual(draft.customFields.count, 2)
        let written = try ScriptMetaKitMetadata.render(draft)
        let metadata = try ScriptMetaKitMetadata.parse(written, customMetadataKeys: ["X-"])
        XCTAssertEqual(metadata.customValues(for: "X-org.iwashi-Order"), ["0030"])
        XCTAssertEqual(metadata.customValues(for: "X-org.iwashi-Group"), ["keep"])
    }

    func testLastLanguageRemovalPreservesCustomFieldsAndUnknownLines() throws {
        let read = try ScriptMetaKitMetadata.readDraft(from: source)
        var editor = try ScriptMetaKitMetadata.localizationEditor(from: read)
        try editor.removeLanguage("en")
        try editor.removeLanguage("ja")
        let written = try ScriptMetaKitMetadata.write(editor.applying(to: read.draft), into: source)
        let parsed = try ScriptMetaKitMetadata.parse(written, customMetadataKeys: ["X-"])
        XCTAssertNil(parsed.presentation.defaultLanguage)
        XCTAssertEqual(parsed.customValues(for: "X-org.iwashi-Order"), ["0020"])
        XCTAssertEqual(parsed.tags, ["image"])
        XCTAssertTrue(written.contains("X-Custom=legacy"))
        XCTAssertTrue(written.contains("Unknown-Key=keep"))
    }

    func testEngineReadsAndWritesCustomFieldsThroughTheNativeABI() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("test.jsx")
        let engine = ScriptMetaKitEngine()
        let changed = try await engine.setCustomMetadataKeys(["X-"])
        XCTAssertTrue(changed)
        let result = try await engine.scan(folderURLs: [root], checkUpdates: false)
        XCTAssertEqual(result.allItems.first?.customValues(for: "X-org.iwashi-Order"), ["0020"])
        let session = try await engine.beginScriptMetadataEditSession(fileURL: file)
        XCTAssertEqual(session.draft.customFields["X-org.iwashi-order"], ["0020"])
        var draft = session.draft
        draft.name = "変更"
        try draft.setCustomValues(["0030"], for: "X-org.iwashi-Order")
        try draft.setCustomValues([], for: "X-org.iwashi-Empty")
        _ = try await engine.commitScriptMetadataEditSession(session, draft: draft, backupRootURL: root.appendingPathComponent("backups"))
        let written = try String(contentsOf: file, encoding: .utf8)
        let reopened = try ScriptMetaKitMetadata.parse(written, customMetadataKeys: ["X-"])
        XCTAssertEqual(reopened.customValues(for: "X-org.iwashi-Order"), ["0030"])
        XCTAssertNil(reopened.customValues(for: "X-org.iwashi-Empty"))
        XCTAssertEqual(reopened.customValues(for: "X-org.iwashi-Group"), ["Production Tools", "Production Tools"])
        XCTAssertTrue(written.contains("X-Custom=legacy"))
        XCTAssertTrue(written.contains("Unknown-Key=keep"))
        XCTAssertTrue(written.contains("Name[en]=English"))
    }

    func testUnsafeNativeSaveLeavesTheOriginalFileAndBackupDirectoryUntouched() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("test.jsx")
        let backups = root.appendingPathComponent("backups")
        let engine = ScriptMetaKitEngine()
        let session = try await engine.beginScriptMetadataEditSession(fileURL: file)
        var draft = session.draft
        draft.customFields["X-org.iwashi-Note"] = ["Note Name=Injected"]
        do {
            _ = try await engine.commitScriptMetadataEditSession(session, draft: draft, backupRootURL: backups)
            XCTFail("Unsafe custom metadata must fail before writing")
        } catch {
            XCTAssertTrue(String(describing: error).contains("custom_metadata_legacy_field"))
        }
        XCTAssertEqual(try String(contentsOf: file, encoding: .utf8), source)
        XCTAssertFalse(FileManager.default.fileExists(atPath: backups.path))
    }

    func testScanRangesRemainAlignedAcrossNestedAndMetadataFreeEntries() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        try "// ordinary JavaScript".write(to: root.appendingPathComponent("a.jsx"), atomically: true, encoding: .utf8)
        let nested = root.appendingPathComponent("nested")
        try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
        try source.replacingOccurrences(of: "example.swift.custom", with: "example.swift.nested").write(to: nested.appendingPathComponent("c.jsx"), atomically: true, encoding: .utf8)
        let engine = ScriptMetaKitEngine()
        try await engine.setCustomMetadataKeys(["X-"])
        let scan = try await engine.scan(folderURLs: [root], checkUpdates: false)
        let items = scan.allItems.filter { $0.hasScriptMeta }
        XCTAssertEqual(items.count, 2)
        XCTAssertEqual(items.map { $0.customValues(for: "X-org.iwashi-Empty") }, [[""], [""]])
        let changed = try await engine.setCustomMetadataKeys(["X-org.iwashi-Order"])
        XCTAssertTrue(changed)
        let equivalent = try await engine.setCustomMetadataKeys(["x-ORG.IWASHI-order", "X-org.iwashi-Order"])
        XCTAssertFalse(equivalent)
        let narrowed = try await engine.scan(folderURLs: [root], checkUpdates: false)
        XCTAssertTrue(narrowed.allItems.filter { $0.hasScriptMeta }.allSatisfy { $0.customFields.count == 1 })
    }
}
