import XCTest
@testable import ScriptMetaKit

final class LocalizationEditingTests: XCTestCase {
    private let source = """
    /*
    SCRIPTMETA-BEGIN
    Script-ID=swift.editor
    Name=日本語名
    Name[en]=English
    Author=作者
    Meta-Default-language=ja
    Tags=image,画像
    X-Custom=keep
    Description-BEGIN
      日本語

    [en]English body
    Description-END
    SCRIPTMETA-END
    */
    """

    func testDefaultSwitchAndEmptyLanguagesThroughPublicTextAPI() throws {
        let read = try ScriptMetaKitMetadata.readDraft(from: source)
        var editor = try ScriptMetaKitMetadata.localizationEditor(from: read)
        try editor.addLanguage("fr")
        try editor.setText(" \t", for: .author, language: "fr")
        try editor.setText("\n[de]literal\nC:\\temp\n\n", for: .description, language: "en")
        try editor.setDefaultLanguage("en")
        let written = try ScriptMetaKitMetadata.write(editor.applying(to: read.draft), into: source)
        let reopened = try ScriptMetaKitMetadata.localizationEditor(from: ScriptMetaKitMetadata.readDraft(from: written))
        XCTAssertEqual(reopened.languages, ["en", "fr", "ja"])
        XCTAssertEqual(try reopened.text(for: .name, language: "ja"), "日本語名")
        XCTAssertEqual(try reopened.text(for: .name, language: "en"), "English")
        XCTAssertNil(try reopened.text(for: .name, language: "fr-FR"))
        XCTAssertEqual(try reopened.text(for: .author, language: "fr"), " \t")
        XCTAssertEqual(try reopened.text(for: .description, language: "en"), "\n[de]literal\nC:\\temp\n\n")
        XCTAssertTrue(written.contains("X-Custom=keep"))
    }

    func testCompletelyEmptyLanguageIsPersistedWithoutDisplayFallbackInEditor() throws {
        let read = try ScriptMetaKitMetadata.readDraft(from: source)
        var editor = try ScriptMetaKitMetadata.localizationEditor(from: read)
        try editor.addLanguage("fr")
        let written = try ScriptMetaKitMetadata.write(editor.applying(to: read.draft), into: source)
        let reopened = try ScriptMetaKitMetadata.localizationEditor(from: ScriptMetaKitMetadata.readDraft(from: written))
        XCTAssertEqual(reopened.languages, ["ja", "en", "fr"])
        XCTAssertEqual(try reopened.text(for: .name, language: "fr"), "")
        XCTAssertNil(try reopened.text(for: .description, language: "fr"))
        XCTAssertEqual(try ScriptMetaKitMetadata.parse(written, preferredLanguages: ["fr", "en"]).name, "English")
    }

    func testRemovingDefaultRequiresExplicitChoiceAndFailedOperationDoesNotMutateEditor() throws {
        let read = try ScriptMetaKitMetadata.readDraft(from: source)
        var editor = try ScriptMetaKitMetadata.localizationEditor(from: read)
        XCTAssertThrowsError(try editor.clearDefaultLanguage())
        XCTAssertEqual(editor.defaultLanguage, "ja")
        try editor.removeLanguage("ja")
        XCTAssertNil(editor.defaultLanguage)
        XCTAssertThrowsError(try editor.applying(to: read.draft))
        try editor.setDefaultLanguage("en")
        XCTAssertEqual(try editor.applying(to: read.draft).name, "English")
    }

    func testLastLanguageReturnsToPlainFieldsWithTagsUnknownFieldsAndBackup() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("example.jsx")
        try source.write(to: file, atomically: true, encoding: .utf8)
        let engine = ScriptMetaKitEngine()
        let session = try await engine.beginScriptMetadataEditSession(fileURL: file)
        var editor = try ScriptMetaKitMetadata.localizationEditor(from: session.readResult)
        try editor.removeLanguage("en")
        try editor.removeLanguage("ja")
        XCTAssertTrue(editor.languages.isEmpty)
        XCTAssertEqual(try editor.text(for: .name), "日本語名")
        let draft = try editor.applying(to: session.draft)
        let saved = try await engine.commitScriptMetadataEditSession(session, draft: draft, backupRootURL: directory.appendingPathComponent("backups"))
        XCTAssertNotNil(saved.backup)
        let text = try String(contentsOf: file, encoding: .utf8)
        XCTAssertFalse(text.contains("Meta-Default-language"))
        XCTAssertTrue(text.contains("X-Custom=keep"))
        let reread = try await engine.readScriptMetadataDraft(fileURL: file)
        XCTAssertEqual(reread.draft.name, "日本語名")
        XCTAssertEqual(reread.draft.itemDescription, "  日本語\n")
        XCTAssertEqual(reread.draft.extensions?.tags, ["image", "画像"])
        try "\(text)\n// external".write(to: file, atomically: true, encoding: .utf8)
        do {
            _ = try await engine.commitScriptMetadataEditSession(session, draft: draft)
            XCTFail("A stale session must be rejected")
        } catch ScriptMetaKitError.sourceConflict { }
        XCTAssertTrue(try String(contentsOf: file, encoding: .utf8).hasSuffix("// external"))
    }

    func testTagsOnlyAndLegacyNilExtensionsRemainIndependent() throws {
        let text = "SCRIPTMETA-BEGIN\nScript-ID=tags\nTags=image\nSCRIPTMETA-END"
        let draft = try ScriptMetaKitMetadata.readDraft(from: text).draft
        let written = try ScriptMetaKitMetadata.write(draft, into: text)
        XCTAssertEqual(try ScriptMetaKitMetadata.parse(written).tags, ["image"])
        var legacy = try ScriptMetaKitMetadata.readDraft(from: source).draft
        legacy.extensions = nil
        let preserved = try ScriptMetaKitMetadata.write(legacy, into: source)
        XCTAssertTrue(preserved.contains("Name[en]=English"))
        XCTAssertTrue(preserved.contains("Meta-Default-language=ja"))
    }
    func testLastLanguageRetainsLiteralLabelLookingText() throws {
        let read = try ScriptMetaKitMetadata.readDraft(from: source)
        var editor = try ScriptMetaKitMetadata.localizationEditor(from: read)
        let body = "[en]使い方\nC:\\temp\n\n"
        try editor.setText(body, for: .description, language: "ja")
        try editor.removeLanguage("en")
        try editor.clearDefaultLanguage()
        let written = try ScriptMetaKitMetadata.write(editor.applying(to: read.draft), into: source)
        XCTAssertFalse(written.contains("Meta-Default-language"))
        XCTAssertEqual(try ScriptMetaKitMetadata.parse(written, preferredLanguages: ["en"]).itemDescription, body)
    }

}
