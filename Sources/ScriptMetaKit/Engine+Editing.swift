import Foundation

public extension ScriptMetaKitEngine {
    func beginScriptMetadataEditSession(
        fileURL: URL
    ) async throws -> ScriptMetadataEditSession {
        let readResult = try await readScriptMetadataDraft(fileURL: fileURL)
        return ScriptMetadataEditSession(fileURL: fileURL, readResult: readResult)
    }

    func commitScriptMetadataEditSession(
        _ session: ScriptMetadataEditSession,
        draft: ScriptMetadataDraft,
        mode: ScriptMetaWriteMode = .insertOrReplace,
        backupRootURL: URL? = nil
    ) async throws -> ScriptMetadataFileWriteResult {
        try await writeScriptMetadata(
            fileURL: session.fileURL,
            draft: draft,
            mode: mode,
            backupRootURL: backupRootURL,
            expectedSourceFingerprint: session.sourceFingerprint
        )
    }

    func writeScriptMetadataUnconditionally(
        fileURL: URL,
        draft: ScriptMetadataDraft,
        mode: ScriptMetaWriteMode = .insertOrReplace,
        backupRootURL: URL? = nil
    ) async throws -> ScriptMetadataFileWriteResult {
        try await writeScriptMetadata(
            fileURL: fileURL,
            draft: draft,
            mode: mode,
            backupRootURL: backupRootURL,
            expectedSourceFingerprint: nil
        )
    }
}
