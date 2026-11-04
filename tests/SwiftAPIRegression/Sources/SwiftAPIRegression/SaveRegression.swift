import Foundation
import ScriptMetaKit

private struct RegressionFailure: Error, CustomStringConvertible {
    let description: String
}

private func require(_ condition: @autoclosure () -> Bool, _ message: String) throws {
    guard condition() else { throw RegressionFailure(description: message) }
}

@main
struct SaveRegression {
    static func main() async throws {
        let expectedVersion = CommandLine.arguments.dropFirst().first ?? "2.1.1"
        try require(ScriptMetaKitRuntime.packageVersion == expectedVersion, "Native package version mismatch")
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitSwiftABI-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }

        let file = directory.appendingPathComponent("Fixture.jsx")
        let backups = directory.appendingPathComponent("backups")
        let body = "var untouchedBody = 'keep';\n"
        let source = """
        /*
        SCRIPTMETA-BEGIN
        Script-ID=org.example.swift-abi
        Version=1.0.0
        Meta-Default-language=ja
        Name=日本語の名前
        Name[en]=English name
        Name[fr]=Nom français
        Author=作者
        Author[en]=Author
        Author[fr]=
        Tags=image, 画像
        X-org.example-Order=0010
        X-org.example-Order=0010
        X-org.example-Empty=
        X-org.example-URL=https://example.com/some%20path?a=1&b=2
        Unknown-Field=keep
        Description-BEGIN
        [ja]日本語の説明
        二行目です。
        [en]English description
        [fr]Description française
        Description-END
        SCRIPTMETA-END
        */

        """ + body
        try source.write(to: file, atomically: true, encoding: .utf8)
        let engine = ScriptMetaKitEngine()
        try await engine.setPreferredLanguages(["ja", "en", "fr"])
        try await engine.setCustomMetadataKeys(["X-"])
        let scan = try await engine.scan(folderURLs: [directory], checkUpdates: false)
        try require(scan.allItems.count == 1, "Typed scan result was not decoded")
        try require(scan.allItems.first?.customValues(for: "X-org.example-Order") == ["0010", "0010"],
                    "Typed custom-field ranges were not decoded")

        var session = try await engine.beginScriptMetadataEditSession(fileURL: file)
        var draft = session.draft
        draft.version = "2.0.0"
        try draft.setCustomValues(["0020", "0020"], for: "X-org.example-Order")
        _ = try await engine.commitScriptMetadataEditSession(session, draft: draft)
        try verifyPreservation(file, original: source, body: body, version: "2.0.0")

        session = try await engine.beginScriptMetadataEditSession(fileURL: file)
        draft = session.draft
        draft.version = "3.0.0"
        let saved = try await engine.commitScriptMetadataEditSession(session, draft: draft, backupRootURL: backups)
        let backup = try unwrap(saved.backup, "Saving with a backup did not return a record")
        let backupText = try String(contentsOfFile: backup.backupFilePath, encoding: .utf8)
        let backupMetadata = try ScriptMetaKitMetadata.parse(backupText)
        try require(backupMetadata.version == "2.0.0", "Backup does not contain the previous source")
        let generations = try await engine.scriptMetaBackupGenerations(fileURL: file, backupRootURL: backups)
        try require(!generations.isEmpty, "Backup generation records were not decoded")
        try verifyPreservation(file, original: source, body: body, version: "3.0.0")

        session = try await engine.beginScriptMetadataEditSession(fileURL: file)
        let external = try String(contentsOf: file, encoding: .utf8) + "// external change\n"
        try external.write(to: file, atomically: true, encoding: .utf8)
        draft = session.draft
        draft.version = "4.0.0"
        var conflictDetected = false
        do {
            _ = try await engine.commitScriptMetadataEditSession(session, draft: draft, backupRootURL: backups)
        } catch ScriptMetaKitError.sourceConflict {
            conflictDetected = true
        }
        try require(conflictDetected, "An external source change was not rejected")
        let afterConflict = try String(contentsOf: file, encoding: .utf8)
        try require(afterConflict == external, "Conflict handling overwrote the external change")

        let link = directory.appendingPathComponent("Reference.jsx")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: file)
        session = try await engine.beginScriptMetadataEditSession(fileURL: link)
        draft = session.draft
        draft.version = "5.0.0"
        _ = try await engine.commitScriptMetadataEditSession(session, draft: draft, backupRootURL: backups)
        _ = try FileManager.default.destinationOfSymbolicLink(atPath: link.path)
        let throughLink = try ScriptMetaKitMetadata.parse(String(contentsOf: link, encoding: .utf8))
        try require(throughLink.version == "5.0.0", "Saving through a symlink did not update the target")

        #if arch(arm64)
        let architecture = "arm64"
        #elseif arch(x86_64)
        let architecture = "x86_64"
        #else
        let architecture = "other"
        #endif
        #if swift(>=6)
        let swiftLanguageMode = "6"
        #else
        let swiftLanguageMode = "5"
        #endif
        let report: [String: String] = [
            "status": "passed", "architecture": architecture,
            "swiftLanguageMode": swiftLanguageMode,
            "version": ScriptMetaKitRuntime.packageVersion,
            "normalSave": "passed", "externalChangeDetection": "passed",
            "backupSave": "passed", "symlinkSave": "passed",
            "localizationsTagsCustomAndUnknownFields": "preserved"
        ]
        let data = try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])
        print(String(decoding: data, as: UTF8.self))
    }

    private static func unwrap<T>(_ value: T?, _ message: String) throws -> T {
        guard let value else { throw RegressionFailure(description: message) }
        return value
    }

    private static func verifyPreservation(_ file: URL, original: String, body: String, version: String) throws {
        let text = try String(contentsOf: file, encoding: .utf8)
        try require(text.contains("Unknown-Field=keep") && text.hasSuffix(body), "Unknown fields or script body changed")
        for language in ["ja", "en", "fr"] {
            let before = try ScriptMetaKitMetadata.parse(original, preferredLanguages: [language], customMetadataKeys: ["X-"])
            let after = try ScriptMetaKitMetadata.parse(text, preferredLanguages: [language], customMetadataKeys: ["X-"])
            try require(after.version == version, "Updated version was not preserved")
            try require(before.name == after.name && before.author == after.author && before.itemDescription == after.itemDescription,
                        "A localization changed: " + language)
            try require(before.tags == after.tags, "Tags changed")
            try require(after.customValues(for: "X-org.example-Order") == ["0020", "0020"], "Repeated custom values changed")
            for key in ["X-org.example-Empty", "X-org.example-URL"] {
                try require(before.customValues(for: key) == after.customValues(for: key), "A custom value changed: " + key)
            }
        }
        let beforeDraft = try ScriptMetaKitMetadata.readDraft(from: original)
        let afterDraft = try ScriptMetaKitMetadata.readDraft(from: text)
        try require(beforeDraft.draft.extensions == afterDraft.draft.extensions, "Full localization editing data changed")
    }
}
