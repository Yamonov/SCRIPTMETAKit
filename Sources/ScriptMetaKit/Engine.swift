import Foundation

public nonisolated enum ScriptMetaKitError: LocalizedError {
    case engineCreationFailed(Int32)
    case operationFailed(Int32, String)

    public var errorDescription: String? {
        switch self {
        case .engineCreationFailed(let status):
            "SCRIPTMETAKit engineを作成できませんでした。status=\(status)"
        case .operationFailed(_, let message):
            message.isEmpty ? "SCRIPTMETAKit FFI呼び出しに失敗しました。" : message
        }
    }
}

public nonisolated enum ScriptMetaVersionOrdering: Int32, Sendable, Codable {
    case less = -1
    case equal = 0
    case greater = 1

    public var comparisonResult: ComparisonResult {
        switch self {
        case .less:
            .orderedAscending
        case .equal:
            .orderedSame
        case .greater:
            .orderedDescending
        }
    }
}

public nonisolated final class ScriptMetaKitEngine: @unchecked Sendable {
    private let engineBox = ScriptMetaKitFFIEngineBox()
    private static let operationPriority: TaskPriority = .utility

    public init() {}

    public static func normalizeVersionString(_ value: String) throws -> String? {
        try normalizeVersionStringViaFFI(value)
    }

    public static func validateVersionString(_ value: String) throws -> Bool {
        try validateVersionStringViaFFI(value)
    }

    public static func compareVersions(_ lhs: String, _ rhs: String) throws -> ScriptMetaVersionOrdering {
        try compareVersionsViaFFI(lhs, rhs)
    }

    public static func validateEditPasswordSHA256Format(_ value: String) throws -> Bool {
        try validateEditPasswordSHA256FormatViaFFI(value)
    }

    public static func renderDistributionMetadata(records: [DistributionMetadataDraft]) throws -> String {
        try ScriptMetaKitFFIEngineBox().renderDistributionMetadata(records: records)
    }

    public static func validateScriptIDUniqueness(
        in items: [ScriptIdUniquenessItem]
    ) throws -> ScriptIdUniquenessReport {
        try validateScriptIDUniquenessViaFFI(in: items)
    }

    public static func validateScriptIDUniqueness(in items: [ScriptMetaItem]) throws -> ScriptIdUniquenessReport {
        try validateScriptIDUniqueness(
            in: items.map {
                ScriptIdUniquenessItem(
                    itemID: $0.filePath,
                    filePath: $0.filePath,
                    scriptID: $0.scriptID
                )
            }
        )
    }

    public func scan(folderURL: URL, checkUpdates: Bool) async throws -> ScriptMetaScanResult {
        try await scan(folderURLs: [folderURL], checkUpdates: checkUpdates)
    }

    public func scan(
        folderURLs: [URL],
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> ScriptMetaScanResult {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.scan(folderURLs: folderURLs, checkUpdates: checkUpdates, onProgress: onProgress)
        }.value
    }

    public func checkUpdate(
        item: ScriptMetaItem,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> UpdateCheckResult {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.checkUpdate(item: item, onProgress: onProgress)
        }.value
    }

    public func checkUpdates(
        items: [ScriptMetaItem],
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> UpdateCheckResult {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.checkUpdates(items: items, onProgress: onProgress)
        }.value
    }

    public func setRoots(_ roots: [ScriptMetaKitRoot]) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.setRoots(roots)
        }.value
    }

    public func replaceRootGroup(_ roots: [ScriptMetaKitRoot], groupID: String) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.replaceRootGroup(roots, groupID: groupID)
        }.value
    }

    public func insertRootsIntoGroup(_ roots: [ScriptMetaKitRoot], groupID: String) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.insertRootsIntoGroup(roots, groupID: groupID)
        }.value
    }

    public func setVisibleRoot(_ rootID: String?) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.setVisibleRoot(rootID)
        }.value
    }

    public func clearVisibleRoot() async throws {
        try await setVisibleRoot(nil)
    }

    public func cancelCurrentOperation() {
        engineBox.cancelCurrentOperation()
    }

    public func shutdown() async {
        await Task.detached(priority: Self.operationPriority) { [engineBox] in
            engineBox.shutdown()
        }.value
    }

    public func scanRegisteredRoots(
        mode: ScriptMetaScanMode = .fileListAndMetadata,
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> ScriptMetaScanResult {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.scanRegisteredRoots(mode: mode, checkUpdates: checkUpdates, onProgress: onProgress)
        }.value
    }

    public func scanRoots(
        rootIDs: [String],
        mode: ScriptMetaScanMode = .fileListAndMetadata,
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> ScriptMetaScanResult {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.scanRoots(rootIDs: rootIDs, mode: mode, checkUpdates: checkUpdates, onProgress: onProgress)
        }.value
    }

    public func cachedRoots(
        rootIDs: [String],
        mode: ScriptMetaScanMode = .fileListAndMetadata
    ) async throws -> ScriptMetaScanResult {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.cachedRoots(rootIDs: rootIDs, mode: mode)
        }.value
    }

    public func scanRoot(
        rootID: String,
        mode: ScriptMetaScanMode = .fileListAndMetadata,
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> ScriptMetaScanResult {
        try await scanRoots(rootIDs: [rootID], mode: mode, checkUpdates: checkUpdates, onProgress: onProgress)
    }

    public func startWatching(onChange: @escaping @Sendable () -> Void) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.startWatching(onChange: onChange)
        }.value
    }

    public func startWatching(folderURLs: [URL], onChange: @escaping @Sendable () -> Void) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.startWatching(folderURLs: folderURLs, onChange: onChange)
        }.value
    }

    public func stopWatching() async {
        await Task.detached(priority: Self.operationPriority) { [engineBox] in
            engineBox.stopWatching()
        }.value
    }

    public func setResolveMacOSAlias(_ enabled: Bool) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.setResolveMacOSAlias(enabled)
        }.value
    }

    public func setDecompileCompiledOSADuringScan(_ enabled: Bool) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.setDecompileCompiledOSADuringScan(enabled)
        }.value
    }

    public func setNativeEventLatencyMillis(_ latencyMillis: UInt64) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.setNativeEventLatencyMillis(latencyMillis)
        }.value
    }

    public func setRootPreflightOptions(_ options: ScriptMetaRootPreflightOptions) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.setRootPreflightOptions(options)
        }.value
    }

    public func loadCache(from fileURL: URL) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.loadCache(from: fileURL)
        }.value
    }

    public func saveCache(to fileURL: URL, scope: ScriptMetaCacheScope = .all) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.saveCache(to: fileURL, scope: scope)
        }.value
    }

    public func writeScriptMetadata(
        fileURL: URL,
        draft: ScriptMetadataDraft,
        mode: ScriptMetaWriteMode = .insertOrReplace,
        backupRootURL: URL? = nil
    ) async throws -> ScriptMetadataFileWriteResult {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.writeScriptMetadata(
                fileURL: fileURL,
                draft: draft,
                mode: mode,
                backupRootURL: backupRootURL
            )
        }.value
    }

    public func readScriptMetadataDraft(fileURL: URL) async throws -> ScriptMetadataEditReadResult {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.readScriptMetadataDraft(fileURL: fileURL)
        }.value
    }

    public func readScriptMetadataEditPreview(
        fileURL: URL,
        maxBytes: Int = 8 * 1024
    ) async throws -> ScriptMetadataEditPreviewResult {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.readScriptMetadataEditPreview(fileURL: fileURL, maxBytes: maxBytes)
        }.value
    }

    public func renderDistributionMetadata(records: [DistributionMetadataDraft]) async throws -> String {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.renderDistributionMetadata(records: records)
        }.value
    }

    public func generateEditPasswordSHA256(password: String) async throws -> String {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.generateEditPasswordSHA256(password: password)
        }.value
    }

    public func verifyEditPasswordSHA256(password: String, storedValue: String) async throws -> Bool {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.verifyEditPasswordSHA256(password: password, storedValue: storedValue)
        }.value
    }

    public func scriptMetaBackupGenerations(
        fileURL: URL,
        backupRootURL: URL
    ) async throws -> [ScriptMetaBackupGeneration] {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.scriptMetaBackupGenerations(fileURL: fileURL, backupRootURL: backupRootURL)
        }.value
    }

    public func createScriptMetaBackup(
        fileURL: URL,
        backupRootURL: URL,
        reason: ScriptMetaBackupReason = .beforeSave
    ) async throws -> ScriptMetaBackupRecord {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.createScriptMetaBackup(fileURL: fileURL, backupRootURL: backupRootURL, reason: reason)
        }.value
    }

    public func restoreScriptMetaBackup(
        fileURL: URL,
        backupRootURL: URL,
        generationID: String
    ) async throws -> ScriptMetaBackupRecord {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.restoreScriptMetaBackup(
                fileURL: fileURL,
                backupRootURL: backupRootURL,
                generationID: generationID
            )
        }.value
    }

    public func clearScriptMetaBackups(fileURL: URL, backupRootURL: URL) async throws {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.clearScriptMetaBackups(fileURL: fileURL, backupRootURL: backupRootURL)
        }.value
    }

    public func resetScriptMetaBackupsWithCurrentAsInitial(
        fileURL: URL,
        backupRootURL: URL
    ) async throws -> ScriptMetaBackupRecord {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.resetScriptMetaBackupsWithCurrentAsInitial(
                fileURL: fileURL,
                backupRootURL: backupRootURL
            )
        }.value
    }

    public func validateScriptIDUniqueness(in items: [ScriptMetaItem]) throws -> ScriptIdUniquenessReport {
        try Self.validateScriptIDUniqueness(in: items)
    }

    public func pollWatchChanges(dirtyOnly: Bool = false) async throws -> ScriptMetaScanResult? {
        try await Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.pollWatchChanges(dirtyOnly: dirtyOnly)
        }.value
    }
}

private nonisolated let smkStatusOK: Int32 = 0

private nonisolated struct SmkUtf8Slice {
    var ptr: UnsafePointer<UInt8>?
    var len: Int

    init(ptr: UnsafePointer<UInt8>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkRootRegistration {
    var rootID: SmkUtf8Slice
    var path: SmkUtf8Slice
    var displayName: SmkUtf8Slice
    var purpose: UInt32
    var watchPolicy: UInt32
    var cachePolicy: UInt32
    var refreshPolicy: UInt32
    var priority: UInt32
}

private nonisolated struct SmkRegisteredRootSignature {
    var rootID: SmkUtf8Slice
    var path: SmkUtf8Slice
}

private nonisolated struct SmkCatalogInfo {
    var hasCatalog: UInt8
    var sourceRevision: SmkUtf8Slice
    var candidateCacheSchemaVersion: UInt32
    var candidateCacheBuiltAt: UInt64

    init(
        hasCatalog: UInt8 = 0,
        sourceRevision: SmkUtf8Slice = SmkUtf8Slice(),
        candidateCacheSchemaVersion: UInt32 = 0,
        candidateCacheBuiltAt: UInt64 = 0
    ) {
        self.hasCatalog = hasCatalog
        self.sourceRevision = sourceRevision
        self.candidateCacheSchemaVersion = candidateCacheSchemaVersion
        self.candidateCacheBuiltAt = candidateCacheBuiltAt
    }
}

private nonisolated struct SmkRootSnapshot {
    var rootID: SmkUtf8Slice
    var path: SmkUtf8Slice
    var status: SmkUtf8Slice
    var isDirty: UInt8
    var hasLastLoadedAt: UInt8
    var lastLoadedAt: UInt64
    var hasLastEventAt: UInt8
    var lastEventAt: UInt64
    var itemCount: Int
    var errorCode: SmkUtf8Slice
    var errorMessage: SmkUtf8Slice
}

private nonisolated struct SmkFileIdentity {
    var stableID: SmkUtf8Slice
    var volumeID: SmkUtf8Slice
    var fileID: SmkUtf8Slice
    var hasFileSize: UInt8
    var fileSize: UInt64
    var hasContentModifiedAt: UInt8
    var contentModifiedAt: UInt64
}

private nonisolated struct SmkFileEntry {
    var displayPath: SmkUtf8Slice
    var resolvedPath: SmkUtf8Slice
    var pathKind: SmkUtf8Slice
    var resolutionStatus: SmkUtf8Slice
    var resolutionMessage: SmkUtf8Slice
    var isDirectory: UInt8
    var hasFileSize: UInt8
    var fileSize: UInt64
    var hasContentModifiedAt: UInt8
    var contentModifiedAt: UInt64
    var hasIdentity: UInt8
    var identity: SmkFileIdentity
    var runtimeKind: SmkUtf8Slice
    var shebang: SmkUtf8Slice
    var hasScriptMeta: UInt8
    var hasScriptMetaEditPassword: UInt8
    var isFileLocked: UInt8
    var isReadOnly: UInt8
    var canEditScriptMeta: UInt8
    var canAppendScriptMeta: UInt8
    var scriptMetaEditState: SmkUtf8Slice
    var hasScriptMetaItem: UInt8
    var scriptMetaItem: SmkScriptItem
    var firstChildIndex: Int
    var childCount: Int
}

private nonisolated struct SmkFileListSnapshot {
    var rootIndex: Int
    var firstChildIndex: Int
    var childCount: Int
    var truncated: UInt8
}

private nonisolated struct SmkScriptItem {
    var rootID: SmkUtf8Slice
    var filePath: SmkUtf8Slice
    var identityPath: SmkUtf8Slice
    var runtimeKind: SmkUtf8Slice
    var shebang: SmkUtf8Slice
    var scriptID: SmkUtf8Slice
    var version: SmkUtf8Slice
    var name: SmkUtf8Slice
    var description: SmkUtf8Slice
    var targetApp: SmkUtf8Slice
    var minTargetVersion: SmkUtf8Slice
    var metaURL: SmkUtf8Slice
    var author: SmkUtf8Slice
    var releaseDate: SmkUtf8Slice
    var editPasswordSHA256: SmkUtf8Slice
    var hasScriptMeta: UInt8
    var hasScriptMetaEditPassword: UInt8
    var isFileLocked: UInt8
    var isReadOnly: UInt8
    var canEditScriptMeta: UInt8
    var canAppendScriptMeta: UInt8
    var scriptMetaEditState: SmkUtf8Slice
}

private nonisolated struct SmkCandidateRecord {
    var rootID: SmkUtf8Slice
    var rootPath: SmkUtf8Slice
    var filePath: SmkUtf8Slice
    var identityPath: SmkUtf8Slice
    var pathKind: SmkUtf8Slice
    var resolutionStatus: SmkUtf8Slice
    var resolutionMessage: SmkUtf8Slice
    var runtimeKind: SmkUtf8Slice
    var shebang: SmkUtf8Slice
    var hasScriptMeta: UInt8
    var hasScriptMetaEditPassword: UInt8
    var isFileLocked: UInt8
    var isReadOnly: UInt8
    var canEditScriptMeta: UInt8
    var canAppendScriptMeta: UInt8
    var scriptMetaEditState: SmkUtf8Slice
    var hasFileSize: UInt8
    var fileSize: UInt64
    var hasContentModifiedAt: UInt8
    var contentModifiedAt: UInt64
    var hasItem: UInt8
    var item: SmkScriptItem
}

private nonisolated struct SmkUpdateCheckInfo {
    var hasUpdateCheck: UInt8
    var checkedAt: UInt64

    init(hasUpdateCheck: UInt8 = 0, checkedAt: UInt64 = 0) {
        self.hasUpdateCheck = hasUpdateCheck
        self.checkedAt = checkedAt
    }
}

private nonisolated struct SmkUpdateStatusEntry {
    var itemID: SmkUtf8Slice
    var status: SmkUtf8Slice
}

private nonisolated struct SmkDistributionResolutionEntry {
    var itemID: SmkUtf8Slice
    var latestVersion: SmkUtf8Slice
    var latestPageURL: SmkUtf8Slice
    var finalPageURL: SmkUtf8Slice
    var firstLatestURLHistoryIndex: Int
    var latestURLHistoryCount: Int
    var checkedAt: UInt64
    var isUnresolved: UInt8
    var note: SmkUtf8Slice
    var hasRedirectCount: UInt8
    var redirectCount: UInt32
}

private nonisolated struct SmkUpdateFailureEntry {
    var itemID: SmkUtf8Slice
    var code: SmkUtf8Slice
    var message: SmkUtf8Slice
    var filePath: SmkUtf8Slice
    var scriptID: SmkUtf8Slice
    var currentVersion: SmkUtf8Slice
    var metaURL: SmkUtf8Slice
    var sourceURL: SmkUtf8Slice
    var checkedAt: UInt64
}

private nonisolated struct SmkUpdateErrorEntry {
    var itemID: SmkUtf8Slice
    var message: SmkUtf8Slice
}

private nonisolated struct SmkUpdateProgress {
    var completedItems: Int
    var totalItems: Int
    var itemID: SmkUtf8Slice
    var scriptID: SmkUtf8Slice
    var phase: SmkUtf8Slice
    var message: SmkUtf8Slice
}

private nonisolated struct SmkScanChangeInfo {
    var hasChangeSummary: UInt8
    var addedCount: Int
    var removedCount: Int
    var modifiedCount: Int

    init(hasChangeSummary: UInt8 = 0, addedCount: Int = 0, removedCount: Int = 0, modifiedCount: Int = 0) {
        self.hasChangeSummary = hasChangeSummary
        self.addedCount = addedCount
        self.removedCount = removedCount
        self.modifiedCount = modifiedCount
    }
}

private nonisolated struct SmkOperationInfo {
    var status: SmkUtf8Slice
    var totalUnits: Int
    var completedUnits: Int
    var failedUnits: Int
    var cancelled: UInt8
    var timedOut: UInt8
    var reasonCode: SmkUtf8Slice
    var message: SmkUtf8Slice

    init(
        status: SmkUtf8Slice = SmkUtf8Slice(),
        totalUnits: Int = 0,
        completedUnits: Int = 0,
        failedUnits: Int = 0,
        cancelled: UInt8 = 0,
        timedOut: UInt8 = 0,
        reasonCode: SmkUtf8Slice = SmkUtf8Slice(),
        message: SmkUtf8Slice = SmkUtf8Slice()
    ) {
        self.status = status
        self.totalUnits = totalUnits
        self.completedUnits = completedUnits
        self.failedUnits = failedUnits
        self.cancelled = cancelled
        self.timedOut = timedOut
        self.reasonCode = reasonCode
        self.message = message
    }
}

private nonisolated struct SmkFileIssue {
    var hasRootID: UInt8
    var rootID: SmkUtf8Slice
    var path: SmkUtf8Slice
    var code: SmkUtf8Slice
    var message: SmkUtf8Slice
    var pathKind: SmkUtf8Slice
    var resolutionStatus: SmkUtf8Slice
    var isDirectory: UInt8
}

private nonisolated struct SmkFileEntryChange {
    var rootID: SmkUtf8Slice
    var kind: SmkUtf8Slice
    var displayPath: SmkUtf8Slice
    var resolvedPath: SmkUtf8Slice
    var pathKind: SmkUtf8Slice
    var resolutionStatus: SmkUtf8Slice
    var resolutionMessage: SmkUtf8Slice
    var isDirectory: UInt8
    var hasFileSize: UInt8
    var fileSize: UInt64
    var hasContentModifiedAt: UInt8
    var contentModifiedAt: UInt64
    var hasIdentity: UInt8
    var identity: SmkFileIdentity
    var runtimeKind: SmkUtf8Slice
    var shebang: SmkUtf8Slice
    var hasScriptMeta: UInt8
    var hasScriptMetaEditPassword: UInt8
    var isFileLocked: UInt8
    var isReadOnly: UInt8
    var canEditScriptMeta: UInt8
    var canAppendScriptMeta: UInt8
    var scriptMetaEditState: SmkUtf8Slice
}

private nonisolated struct SmkWatchChangeInfo {
    var hasWatchChange: UInt8
    var overflowed: UInt8
    var pathCount: Int
    var affectedRootCount: Int
    var eventCount: Int
    var ignoredPathCount: Int
    var renameCandidateCount: Int
    var rescanTargetCount: Int

    init(
        hasWatchChange: UInt8 = 0,
        overflowed: UInt8 = 0,
        pathCount: Int = 0,
        affectedRootCount: Int = 0,
        eventCount: Int = 0,
        ignoredPathCount: Int = 0,
        renameCandidateCount: Int = 0,
        rescanTargetCount: Int = 0
    ) {
        self.hasWatchChange = hasWatchChange
        self.overflowed = overflowed
        self.pathCount = pathCount
        self.affectedRootCount = affectedRootCount
        self.eventCount = eventCount
        self.ignoredPathCount = ignoredPathCount
        self.renameCandidateCount = renameCandidateCount
        self.rescanTargetCount = rescanTargetCount
    }
}

private nonisolated struct SmkWatchPathEvent {
    var rootID: SmkUtf8Slice
    var path: SmkUtf8Slice
    var kind: SmkUtf8Slice
    var isDirectory: UInt8
    var rescanDirectory: SmkUtf8Slice
}

private nonisolated struct SmkIgnoredWatchPath {
    var hasRootID: UInt8
    var rootID: SmkUtf8Slice
    var path: SmkUtf8Slice
    var reason: SmkUtf8Slice
}

private nonisolated struct SmkWatchRenameCandidate {
    var rootID: SmkUtf8Slice
    var oldPath: SmkUtf8Slice
    var newPath: SmkUtf8Slice
    var confidence: SmkUtf8Slice
}

private nonisolated struct SmkWatchRescanTarget {
    var rootID: SmkUtf8Slice
    var path: SmkUtf8Slice
    var reason: SmkUtf8Slice
}

private nonisolated struct SmkRootSnapshotSlice {
    var ptr: UnsafePointer<SmkRootSnapshot>?
    var len: Int

    init(ptr: UnsafePointer<SmkRootSnapshot>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkRegisteredRootSignatureSlice {
    var ptr: UnsafePointer<SmkRegisteredRootSignature>?
    var len: Int

    init(ptr: UnsafePointer<SmkRegisteredRootSignature>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkFileListSnapshotSlice {
    var ptr: UnsafePointer<SmkFileListSnapshot>?
    var len: Int

    init(ptr: UnsafePointer<SmkFileListSnapshot>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkFileEntrySlice {
    var ptr: UnsafePointer<SmkFileEntry>?
    var len: Int

    init(ptr: UnsafePointer<SmkFileEntry>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkScriptItemSlice {
    var ptr: UnsafePointer<SmkScriptItem>?
    var len: Int

    init(ptr: UnsafePointer<SmkScriptItem>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkScriptIdUniquenessItem {
    var itemID: SmkUtf8Slice
    var filePath: SmkUtf8Slice
    var scriptID: SmkUtf8Slice

    init(
        itemID: SmkUtf8Slice = SmkUtf8Slice(),
        filePath: SmkUtf8Slice = SmkUtf8Slice(),
        scriptID: SmkUtf8Slice = SmkUtf8Slice()
    ) {
        self.itemID = itemID
        self.filePath = filePath
        self.scriptID = scriptID
    }
}

private nonisolated struct SmkScriptIdUniquenessReport {
    var totalItems: Int
    var uniqueScriptIDs: Int
    var duplicateCount: Int

    init(totalItems: Int = 0, uniqueScriptIDs: Int = 0, duplicateCount: Int = 0) {
        self.totalItems = totalItems
        self.uniqueScriptIDs = uniqueScriptIDs
        self.duplicateCount = duplicateCount
    }
}

private nonisolated struct SmkScriptIdDuplicate {
    var scriptID: SmkUtf8Slice
    var firstItemIDIndex: Int
    var itemIDCount: Int
    var firstFilePathIndex: Int
    var filePathCount: Int

    init(
        scriptID: SmkUtf8Slice = SmkUtf8Slice(),
        firstItemIDIndex: Int = 0,
        itemIDCount: Int = 0,
        firstFilePathIndex: Int = 0,
        filePathCount: Int = 0
    ) {
        self.scriptID = scriptID
        self.firstItemIDIndex = firstItemIDIndex
        self.itemIDCount = itemIDCount
        self.firstFilePathIndex = firstFilePathIndex
        self.filePathCount = filePathCount
    }
}

private nonisolated struct SmkScriptIdDuplicateSlice {
    var ptr: UnsafePointer<SmkScriptIdDuplicate>?
    var len: Int

    init(ptr: UnsafePointer<SmkScriptIdDuplicate>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkCandidateRecordSlice {
    var ptr: UnsafePointer<SmkCandidateRecord>?
    var len: Int

    init(ptr: UnsafePointer<SmkCandidateRecord>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkUpdateStatusEntrySlice {
    var ptr: UnsafePointer<SmkUpdateStatusEntry>?
    var len: Int

    init(ptr: UnsafePointer<SmkUpdateStatusEntry>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkDistributionResolutionEntrySlice {
    var ptr: UnsafePointer<SmkDistributionResolutionEntry>?
    var len: Int

    init(ptr: UnsafePointer<SmkDistributionResolutionEntry>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkUpdateFailureEntrySlice {
    var ptr: UnsafePointer<SmkUpdateFailureEntry>?
    var len: Int

    init(ptr: UnsafePointer<SmkUpdateFailureEntry>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkUpdateErrorEntrySlice {
    var ptr: UnsafePointer<SmkUpdateErrorEntry>?
    var len: Int

    init(ptr: UnsafePointer<SmkUpdateErrorEntry>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkUtf8SliceSlice {
    var ptr: UnsafePointer<SmkUtf8Slice>?
    var len: Int

    init(ptr: UnsafePointer<SmkUtf8Slice>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkFileEntryChangeSlice {
    var ptr: UnsafePointer<SmkFileEntryChange>?
    var len: Int

    init(ptr: UnsafePointer<SmkFileEntryChange>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkFileIssueSlice {
    var ptr: UnsafePointer<SmkFileIssue>?
    var len: Int

    init(ptr: UnsafePointer<SmkFileIssue>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkWatchPathEventSlice {
    var ptr: UnsafePointer<SmkWatchPathEvent>?
    var len: Int

    init(ptr: UnsafePointer<SmkWatchPathEvent>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkIgnoredWatchPathSlice {
    var ptr: UnsafePointer<SmkIgnoredWatchPath>?
    var len: Int

    init(ptr: UnsafePointer<SmkIgnoredWatchPath>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkWatchRenameCandidateSlice {
    var ptr: UnsafePointer<SmkWatchRenameCandidate>?
    var len: Int

    init(ptr: UnsafePointer<SmkWatchRenameCandidate>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkWatchRescanTargetSlice {
    var ptr: UnsafePointer<SmkWatchRescanTarget>?
    var len: Int

    init(ptr: UnsafePointer<SmkWatchRescanTarget>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkScriptMetadataDraft {
    var scriptID: SmkUtf8Slice
    var version: SmkUtf8Slice
    var description: SmkUtf8Slice
    var targetApp: SmkUtf8Slice
    var minTargetVersion: SmkUtf8Slice
    var metaURL: SmkUtf8Slice
    var name: SmkUtf8Slice
    var author: SmkUtf8Slice
    var releaseDate: SmkUtf8Slice
    var editPasswordSHA256: SmkUtf8Slice

    init(
        scriptID: SmkUtf8Slice = SmkUtf8Slice(),
        version: SmkUtf8Slice = SmkUtf8Slice(),
        description: SmkUtf8Slice = SmkUtf8Slice(),
        targetApp: SmkUtf8Slice = SmkUtf8Slice(),
        minTargetVersion: SmkUtf8Slice = SmkUtf8Slice(),
        metaURL: SmkUtf8Slice = SmkUtf8Slice(),
        name: SmkUtf8Slice = SmkUtf8Slice(),
        author: SmkUtf8Slice = SmkUtf8Slice(),
        releaseDate: SmkUtf8Slice = SmkUtf8Slice(),
        editPasswordSHA256: SmkUtf8Slice = SmkUtf8Slice()
    ) {
        self.scriptID = scriptID
        self.version = version
        self.description = description
        self.targetApp = targetApp
        self.minTargetVersion = minTargetVersion
        self.metaURL = metaURL
        self.name = name
        self.author = author
        self.releaseDate = releaseDate
        self.editPasswordSHA256 = editPasswordSHA256
    }
}

private nonisolated struct SmkScriptMetadataWriteRequest {
    var filePath: SmkUtf8Slice
    var backupRootPath: SmkUtf8Slice
    var writeMode: UInt32
    var draft: SmkScriptMetadataDraft
}

private nonisolated struct SmkScriptMetadataEditReadResult {
    var filePath: SmkUtf8Slice
    var draft: SmkScriptMetadataDraft
    var commentStyle: SmkUtf8Slice
    var lineEnding: SmkUtf8Slice
    var hasExistingBlock: UInt8
    var existingBlockText: SmkUtf8Slice
    var sourceFingerprint: SmkUtf8Slice

    init(
        filePath: SmkUtf8Slice = SmkUtf8Slice(),
        draft: SmkScriptMetadataDraft = SmkScriptMetadataDraft(),
        commentStyle: SmkUtf8Slice = SmkUtf8Slice(),
        lineEnding: SmkUtf8Slice = SmkUtf8Slice(),
        hasExistingBlock: UInt8 = 0,
        existingBlockText: SmkUtf8Slice = SmkUtf8Slice(),
        sourceFingerprint: SmkUtf8Slice = SmkUtf8Slice()
    ) {
        self.filePath = filePath
        self.draft = draft
        self.commentStyle = commentStyle
        self.lineEnding = lineEnding
        self.hasExistingBlock = hasExistingBlock
        self.existingBlockText = existingBlockText
        self.sourceFingerprint = sourceFingerprint
    }
}

private nonisolated struct SmkScriptMetadataEditPreviewResult {
    var filePath: SmkUtf8Slice
    var previewText: SmkUtf8Slice
    var previewByteCount: Int
    var fileSize: UInt64
    var hasFileSize: UInt8
    var commentStyle: SmkUtf8Slice
    var lineEnding: SmkUtf8Slice
    var hasScriptmetaMarkerInPreview: UInt8
    var isTruncated: UInt8
    var requiresFullRead: UInt8
    var fileStateFingerprint: SmkUtf8Slice

    init(
        filePath: SmkUtf8Slice = SmkUtf8Slice(),
        previewText: SmkUtf8Slice = SmkUtf8Slice(),
        previewByteCount: Int = 0,
        fileSize: UInt64 = 0,
        hasFileSize: UInt8 = 0,
        commentStyle: SmkUtf8Slice = SmkUtf8Slice(),
        lineEnding: SmkUtf8Slice = SmkUtf8Slice(),
        hasScriptmetaMarkerInPreview: UInt8 = 0,
        isTruncated: UInt8 = 0,
        requiresFullRead: UInt8 = 0,
        fileStateFingerprint: SmkUtf8Slice = SmkUtf8Slice()
    ) {
        self.filePath = filePath
        self.previewText = previewText
        self.previewByteCount = previewByteCount
        self.fileSize = fileSize
        self.hasFileSize = hasFileSize
        self.commentStyle = commentStyle
        self.lineEnding = lineEnding
        self.hasScriptmetaMarkerInPreview = hasScriptmetaMarkerInPreview
        self.isTruncated = isTruncated
        self.requiresFullRead = requiresFullRead
        self.fileStateFingerprint = fileStateFingerprint
    }
}

private nonisolated struct SmkDistributionMetadataDraft {
    var scriptID: SmkUtf8Slice
    var version: SmkUtf8Slice
    var latestURL: SmkUtf8Slice
    var latestPageURL: SmkUtf8Slice
}

private nonisolated struct SmkScriptMetaBackupRecord {
    var id: SmkUtf8Slice
    var createdAtMillis: UInt64
    var backupFileName: SmkUtf8Slice
    var backupFilePath: SmkUtf8Slice
    var fileSize: UInt64
    var reason: SmkUtf8Slice

    init(
        id: SmkUtf8Slice = SmkUtf8Slice(),
        createdAtMillis: UInt64 = 0,
        backupFileName: SmkUtf8Slice = SmkUtf8Slice(),
        backupFilePath: SmkUtf8Slice = SmkUtf8Slice(),
        fileSize: UInt64 = 0,
        reason: SmkUtf8Slice = SmkUtf8Slice()
    ) {
        self.id = id
        self.createdAtMillis = createdAtMillis
        self.backupFileName = backupFileName
        self.backupFilePath = backupFilePath
        self.fileSize = fileSize
        self.reason = reason
    }
}

private nonisolated struct SmkScriptMetadataFileWriteResult {
    var filePath: SmkUtf8Slice
    var operation: SmkUtf8Slice
    var hasBackup: UInt8
    var backup: SmkScriptMetaBackupRecord

    init(
        filePath: SmkUtf8Slice = SmkUtf8Slice(),
        operation: SmkUtf8Slice = SmkUtf8Slice(),
        hasBackup: UInt8 = 0,
        backup: SmkScriptMetaBackupRecord = SmkScriptMetaBackupRecord()
    ) {
        self.filePath = filePath
        self.operation = operation
        self.hasBackup = hasBackup
        self.backup = backup
    }
}

private nonisolated struct SmkScriptMetaBackupGeneration {
    var id: SmkUtf8Slice
    var sequenceNumber: Int
    var createdAtMillis: UInt64
    var filePath: SmkUtf8Slice
    var fileSize: UInt64
    var reason: SmkUtf8Slice
    var isCurrentFile: UInt8
}

private nonisolated struct SmkScriptMetaBackupGenerationSlice {
    var ptr: UnsafePointer<SmkScriptMetaBackupGeneration>?
    var len: Int

    init(ptr: UnsafePointer<SmkScriptMetaBackupGeneration>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

@_silgen_name("smk_engine_create_default")
private nonisolated func smk_engine_create_default(_ outEngine: UnsafeMutablePointer<OpaquePointer?>) -> Int32

@_silgen_name("smk_engine_free")
private nonisolated func smk_engine_free(_ engine: OpaquePointer?)

@_silgen_name("smk_engine_last_error")
private nonisolated func smk_engine_last_error(_ engine: OpaquePointer?, _ outMessage: UnsafeMutablePointer<SmkUtf8Slice>) -> Int32

@_silgen_name("smk_engine_set_resolve_macos_alias")
private nonisolated func smk_engine_set_resolve_macos_alias(_ engine: OpaquePointer?, _ enabled: UInt8) -> Int32

@_silgen_name("smk_engine_set_decompile_compiled_osa_during_scan")
private nonisolated func smk_engine_set_decompile_compiled_osa_during_scan(_ engine: OpaquePointer?, _ enabled: UInt8) -> Int32

@_silgen_name("smk_engine_set_native_event_latency_millis")
private nonisolated func smk_engine_set_native_event_latency_millis(_ engine: OpaquePointer?, _ latencyMillis: UInt64) -> Int32

@_silgen_name("smk_engine_set_root_preflight_options")
private nonisolated func smk_engine_set_root_preflight_options(
    _ engine: OpaquePointer?,
    _ rejectTrashRoots: UInt8,
    _ rejectRestrictedRoots: UInt8,
    _ rejectLowScriptDensityLargeRoots: UInt8,
    _ maxScannedItems: Int,
    _ maxDurationMillis: UInt64,
    _ minScannedFileCountForLargeRoot: Int,
    _ minScriptRatioDenominator: Int,
    _ minScannedItemsForTimeLimit: Int
) -> Int32

@_silgen_name("smk_engine_cancel_current_operation")
private nonisolated func smk_engine_cancel_current_operation(_ engine: OpaquePointer?) -> Int32

@_silgen_name("smk_engine_set_roots")
private nonisolated func smk_engine_set_roots(
    _ engine: OpaquePointer?,
    _ roots: UnsafePointer<SmkRootRegistration>?,
    _ rootCount: Int
) -> Int32

@_silgen_name("smk_engine_replace_root_group")
private nonisolated func smk_engine_replace_root_group(
    _ engine: OpaquePointer?,
    _ groupID: SmkUtf8Slice,
    _ roots: UnsafePointer<SmkRootRegistration>?,
    _ rootCount: Int
) -> Int32

@_silgen_name("smk_engine_insert_roots_into_group")
private nonisolated func smk_engine_insert_roots_into_group(
    _ engine: OpaquePointer?,
    _ groupID: SmkUtf8Slice,
    _ roots: UnsafePointer<SmkRootRegistration>?,
    _ rootCount: Int
) -> Int32

@_silgen_name("smk_engine_set_visible_root")
private nonisolated func smk_engine_set_visible_root(
    _ engine: OpaquePointer?,
    _ rootID: SmkUtf8Slice,
    _ hasRootID: UInt8
) -> Int32

@_silgen_name("smk_engine_scan_folders")
private nonisolated func smk_engine_scan_folders(
    _ engine: OpaquePointer?,
    _ paths: UnsafePointer<SmkUtf8Slice>?,
    _ pathCount: Int,
    _ checkUpdates: UInt8,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

private typealias SmkUpdateProgressCallback = @convention(c) (
    UnsafeRawPointer?,
    UnsafeMutableRawPointer?
) -> Void

private typealias SmkWatchNotificationCallback = @convention(c) (
    UnsafeMutableRawPointer?
) -> Void

@_silgen_name("smk_engine_scan_folders_with_progress")
private nonisolated func smk_engine_scan_folders_with_progress(
    _ engine: OpaquePointer?,
    _ paths: UnsafePointer<SmkUtf8Slice>?,
    _ pathCount: Int,
    _ checkUpdates: UInt8,
    _ progressCallback: SmkUpdateProgressCallback?,
    _ progressContext: UnsafeMutableRawPointer?,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_scan_registered_roots")
private nonisolated func smk_engine_scan_registered_roots(
    _ engine: OpaquePointer?,
    _ scanMode: UInt32,
    _ checkUpdates: UInt8,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_scan_roots")
private nonisolated func smk_engine_scan_roots(
    _ engine: OpaquePointer?,
    _ rootIDs: UnsafePointer<SmkUtf8Slice>?,
    _ rootIDCount: Int,
    _ scanMode: UInt32,
    _ checkUpdates: UInt8,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_cached_roots")
private nonisolated func smk_engine_cached_roots(
    _ engine: OpaquePointer?,
    _ rootIDs: UnsafePointer<SmkUtf8Slice>?,
    _ rootIDCount: Int,
    _ scanMode: UInt32,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_scan_registered_roots_with_progress")
private nonisolated func smk_engine_scan_registered_roots_with_progress(
    _ engine: OpaquePointer?,
    _ scanMode: UInt32,
    _ checkUpdates: UInt8,
    _ progressCallback: SmkUpdateProgressCallback?,
    _ progressContext: UnsafeMutableRawPointer?,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_scan_roots_with_progress")
private nonisolated func smk_engine_scan_roots_with_progress(
    _ engine: OpaquePointer?,
    _ rootIDs: UnsafePointer<SmkUtf8Slice>?,
    _ rootIDCount: Int,
    _ scanMode: UInt32,
    _ checkUpdates: UInt8,
    _ progressCallback: SmkUpdateProgressCallback?,
    _ progressContext: UnsafeMutableRawPointer?,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_check_update_item")
private nonisolated func smk_engine_check_update_item(
    _ engine: OpaquePointer?,
    _ item: UnsafePointer<SmkScriptItem>,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_check_update_item_with_progress")
private nonisolated func smk_engine_check_update_item_with_progress(
    _ engine: OpaquePointer?,
    _ item: UnsafePointer<SmkScriptItem>,
    _ progressCallback: SmkUpdateProgressCallback?,
    _ progressContext: UnsafeMutableRawPointer?,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_check_updates_for_items")
private nonisolated func smk_engine_check_updates_for_items(
    _ engine: OpaquePointer?,
    _ items: UnsafePointer<SmkScriptItem>?,
    _ itemCount: Int,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_check_updates_for_items_with_progress")
private nonisolated func smk_engine_check_updates_for_items_with_progress(
    _ engine: OpaquePointer?,
    _ items: UnsafePointer<SmkScriptItem>?,
    _ itemCount: Int,
    _ progressCallback: SmkUpdateProgressCallback?,
    _ progressContext: UnsafeMutableRawPointer?,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_normalize_version_string")
private nonisolated func smk_normalize_version_string(
    _ value: SmkUtf8Slice,
    _ outHasVersion: UnsafeMutablePointer<UInt8>,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_validate_version_string")
private nonisolated func smk_validate_version_string(
    _ value: SmkUtf8Slice,
    _ outIsValid: UnsafeMutablePointer<UInt8>
) -> Int32

@_silgen_name("smk_compare_versions")
private nonisolated func smk_compare_versions(
    _ lhs: SmkUtf8Slice,
    _ rhs: SmkUtf8Slice,
    _ outOrdering: UnsafeMutablePointer<Int32>
) -> Int32

@_silgen_name("smk_validate_edit_password_sha256_format")
private nonisolated func smk_validate_edit_password_sha256_format(
    _ value: SmkUtf8Slice,
    _ outIsValid: UnsafeMutablePointer<UInt8>
) -> Int32

@_silgen_name("smk_validate_script_id_uniqueness")
private nonisolated func smk_validate_script_id_uniqueness(
    _ items: UnsafePointer<SmkScriptIdUniquenessItem>?,
    _ itemCount: Int,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_script_id_uniqueness_result_report")
private nonisolated func smk_script_id_uniqueness_result_report(
    _ result: OpaquePointer?,
    _ outReport: UnsafeMutablePointer<SmkScriptIdUniquenessReport>
) -> Int32

@_silgen_name("smk_script_id_uniqueness_result_duplicates")
private nonisolated func smk_script_id_uniqueness_result_duplicates(
    _ result: OpaquePointer?,
    _ outDuplicates: UnsafeMutablePointer<SmkScriptIdDuplicateSlice>
) -> Int32

@_silgen_name("smk_script_id_uniqueness_result_item_ids")
private nonisolated func smk_script_id_uniqueness_result_item_ids(
    _ result: OpaquePointer?,
    _ outItemIDs: UnsafeMutablePointer<SmkUtf8SliceSlice>
) -> Int32

@_silgen_name("smk_script_id_uniqueness_result_file_paths")
private nonisolated func smk_script_id_uniqueness_result_file_paths(
    _ result: OpaquePointer?,
    _ outFilePaths: UnsafeMutablePointer<SmkUtf8SliceSlice>
) -> Int32

@_silgen_name("smk_script_id_uniqueness_result_free")
private nonisolated func smk_script_id_uniqueness_result_free(_ result: OpaquePointer?)

@_silgen_name("smk_engine_load_cache_file")
private nonisolated func smk_engine_load_cache_file(
    _ engine: OpaquePointer?,
    _ cachePath: SmkUtf8Slice
) -> Int32

@_silgen_name("smk_engine_save_cache_file")
private nonisolated func smk_engine_save_cache_file(
    _ engine: OpaquePointer?,
    _ scope: UInt32,
    _ cachePath: SmkUtf8Slice
) -> Int32

@_silgen_name("smk_engine_start_watching")
private nonisolated func smk_engine_start_watching(_ engine: OpaquePointer?) -> Int32

@_silgen_name("smk_engine_start_watching_with_callback")
private nonisolated func smk_engine_start_watching_with_callback(
    _ engine: OpaquePointer?,
    _ callback: SmkWatchNotificationCallback?,
    _ context: UnsafeMutableRawPointer?
) -> Int32

@_silgen_name("smk_engine_stop_watching")
private nonisolated func smk_engine_stop_watching(_ engine: OpaquePointer?) -> Int32

@_silgen_name("smk_engine_poll_watcher_scan")
private nonisolated func smk_engine_poll_watcher_scan(
    _ engine: OpaquePointer?,
    _ outChanged: UnsafeMutablePointer<UInt8>,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_poll_watcher_scan_dirty_only")
private nonisolated func smk_engine_poll_watcher_scan_dirty_only(
    _ engine: OpaquePointer?,
    _ outChanged: UnsafeMutablePointer<UInt8>,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_scan_result_roots")
private nonisolated func smk_scan_result_roots(_ result: OpaquePointer?, _ outRoots: UnsafeMutablePointer<SmkRootSnapshotSlice>) -> Int32

@_silgen_name("smk_scan_result_catalog_info")
private nonisolated func smk_scan_result_catalog_info(_ result: OpaquePointer?, _ outInfo: UnsafeMutablePointer<SmkCatalogInfo>) -> Int32

@_silgen_name("smk_scan_result_registered_root_signatures")
private nonisolated func smk_scan_result_registered_root_signatures(
    _ result: OpaquePointer?,
    _ outRoots: UnsafeMutablePointer<SmkRegisteredRootSignatureSlice>
) -> Int32

@_silgen_name("smk_scan_result_file_lists")
private nonisolated func smk_scan_result_file_lists(_ result: OpaquePointer?, _ outFileLists: UnsafeMutablePointer<SmkFileListSnapshotSlice>) -> Int32

@_silgen_name("smk_scan_result_file_entries")
private nonisolated func smk_scan_result_file_entries(_ result: OpaquePointer?, _ outFileEntries: UnsafeMutablePointer<SmkFileEntrySlice>) -> Int32

@_silgen_name("smk_scan_result_items")
private nonisolated func smk_scan_result_items(_ result: OpaquePointer?, _ outItems: UnsafeMutablePointer<SmkScriptItemSlice>) -> Int32

@_silgen_name("smk_scan_result_file_items")
private nonisolated func smk_scan_result_file_items(_ result: OpaquePointer?, _ outItems: UnsafeMutablePointer<SmkScriptItemSlice>) -> Int32

@_silgen_name("smk_scan_result_candidate_records")
private nonisolated func smk_scan_result_candidate_records(
    _ result: OpaquePointer?,
    _ outRecords: UnsafeMutablePointer<SmkCandidateRecordSlice>
) -> Int32

@_silgen_name("smk_scan_result_update_info")
private nonisolated func smk_scan_result_update_info(_ result: OpaquePointer?, _ outInfo: UnsafeMutablePointer<SmkUpdateCheckInfo>) -> Int32

@_silgen_name("smk_scan_result_update_statuses")
private nonisolated func smk_scan_result_update_statuses(_ result: OpaquePointer?, _ outStatuses: UnsafeMutablePointer<SmkUpdateStatusEntrySlice>) -> Int32

@_silgen_name("smk_scan_result_update_resolutions")
private nonisolated func smk_scan_result_update_resolutions(_ result: OpaquePointer?, _ outResolutions: UnsafeMutablePointer<SmkDistributionResolutionEntrySlice>) -> Int32

@_silgen_name("smk_scan_result_update_failures")
private nonisolated func smk_scan_result_update_failures(_ result: OpaquePointer?, _ outFailures: UnsafeMutablePointer<SmkUpdateFailureEntrySlice>) -> Int32

@_silgen_name("smk_scan_result_update_errors")
private nonisolated func smk_scan_result_update_errors(_ result: OpaquePointer?, _ outErrors: UnsafeMutablePointer<SmkUpdateErrorEntrySlice>) -> Int32

@_silgen_name("smk_scan_result_latest_url_history_urls")
private nonisolated func smk_scan_result_latest_url_history_urls(_ result: OpaquePointer?, _ outURLs: UnsafeMutablePointer<SmkUtf8SliceSlice>) -> Int32

@_silgen_name("smk_scan_result_change_info")
private nonisolated func smk_scan_result_change_info(_ result: OpaquePointer?, _ outInfo: UnsafeMutablePointer<SmkScanChangeInfo>) -> Int32

@_silgen_name("smk_scan_result_file_entry_changes")
private nonisolated func smk_scan_result_file_entry_changes(_ result: OpaquePointer?, _ outChanges: UnsafeMutablePointer<SmkFileEntryChangeSlice>) -> Int32

@_silgen_name("smk_scan_result_operation_info")
private nonisolated func smk_scan_result_operation_info(_ result: OpaquePointer?, _ outInfo: UnsafeMutablePointer<SmkOperationInfo>) -> Int32

@_silgen_name("smk_scan_result_file_issues")
private nonisolated func smk_scan_result_file_issues(_ result: OpaquePointer?, _ outIssues: UnsafeMutablePointer<SmkFileIssueSlice>) -> Int32

@_silgen_name("smk_scan_result_watch_change_info")
private nonisolated func smk_scan_result_watch_change_info(_ result: OpaquePointer?, _ outInfo: UnsafeMutablePointer<SmkWatchChangeInfo>) -> Int32

@_silgen_name("smk_scan_result_watch_events")
private nonisolated func smk_scan_result_watch_events(_ result: OpaquePointer?, _ outEvents: UnsafeMutablePointer<SmkWatchPathEventSlice>) -> Int32

@_silgen_name("smk_scan_result_ignored_watch_paths")
private nonisolated func smk_scan_result_ignored_watch_paths(_ result: OpaquePointer?, _ outPaths: UnsafeMutablePointer<SmkIgnoredWatchPathSlice>) -> Int32

@_silgen_name("smk_scan_result_watch_rename_candidates")
private nonisolated func smk_scan_result_watch_rename_candidates(_ result: OpaquePointer?, _ outCandidates: UnsafeMutablePointer<SmkWatchRenameCandidateSlice>) -> Int32

@_silgen_name("smk_scan_result_watch_rescan_targets")
private nonisolated func smk_scan_result_watch_rescan_targets(_ result: OpaquePointer?, _ outTargets: UnsafeMutablePointer<SmkWatchRescanTargetSlice>) -> Int32

@_silgen_name("smk_scan_result_free")
private nonisolated func smk_scan_result_free(_ result: OpaquePointer?)

@_silgen_name("smk_engine_write_script_metadata_file")
private nonisolated func smk_engine_write_script_metadata_file(
    _ engine: OpaquePointer?,
    _ request: UnsafePointer<SmkScriptMetadataWriteRequest>?,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_read_script_metadata_draft_file")
private nonisolated func smk_engine_read_script_metadata_draft_file(
    _ engine: OpaquePointer?,
    _ filePath: SmkUtf8Slice,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_read_script_metadata_edit_preview_file")
private nonisolated func smk_engine_read_script_metadata_edit_preview_file(
    _ engine: OpaquePointer?,
    _ filePath: SmkUtf8Slice,
    _ maxBytes: Int,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_render_distribution_metadata")
private nonisolated func smk_engine_render_distribution_metadata(
    _ engine: OpaquePointer?,
    _ records: UnsafePointer<SmkDistributionMetadataDraft>?,
    _ recordCount: Int,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_generate_edit_password_sha256")
private nonisolated func smk_engine_generate_edit_password_sha256(
    _ engine: OpaquePointer?,
    _ password: SmkUtf8Slice,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_verify_edit_password_sha256")
private nonisolated func smk_engine_verify_edit_password_sha256(
    _ engine: OpaquePointer?,
    _ password: SmkUtf8Slice,
    _ storedValue: SmkUtf8Slice,
    _ outIsMatch: UnsafeMutablePointer<UInt8>
) -> Int32

@_silgen_name("smk_engine_scriptmeta_backup_generations")
private nonisolated func smk_engine_scriptmeta_backup_generations(
    _ engine: OpaquePointer?,
    _ filePath: SmkUtf8Slice,
    _ backupRootPath: SmkUtf8Slice,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_create_scriptmeta_backup")
private nonisolated func smk_engine_create_scriptmeta_backup(
    _ engine: OpaquePointer?,
    _ filePath: SmkUtf8Slice,
    _ backupRootPath: SmkUtf8Slice,
    _ reason: UInt32,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_restore_scriptmeta_backup")
private nonisolated func smk_engine_restore_scriptmeta_backup(
    _ engine: OpaquePointer?,
    _ filePath: SmkUtf8Slice,
    _ backupRootPath: SmkUtf8Slice,
    _ generationID: SmkUtf8Slice,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_engine_clear_scriptmeta_backups")
private nonisolated func smk_engine_clear_scriptmeta_backups(
    _ engine: OpaquePointer?,
    _ filePath: SmkUtf8Slice,
    _ backupRootPath: SmkUtf8Slice
) -> Int32

@_silgen_name("smk_engine_reset_scriptmeta_backups_with_current_as_initial")
private nonisolated func smk_engine_reset_scriptmeta_backups_with_current_as_initial(
    _ engine: OpaquePointer?,
    _ filePath: SmkUtf8Slice,
    _ backupRootPath: SmkUtf8Slice,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
) -> Int32

@_silgen_name("smk_edit_result_text")
private nonisolated func smk_edit_result_text(
    _ result: OpaquePointer?,
    _ outText: UnsafeMutablePointer<SmkUtf8Slice>
) -> Int32

@_silgen_name("smk_edit_result_file_write_result")
private nonisolated func smk_edit_result_file_write_result(
    _ result: OpaquePointer?,
    _ outInfo: UnsafeMutablePointer<SmkScriptMetadataFileWriteResult>
) -> Int32

@_silgen_name("smk_edit_result_metadata_edit_read_result")
private nonisolated func smk_edit_result_metadata_edit_read_result(
    _ result: OpaquePointer?,
    _ outInfo: UnsafeMutablePointer<SmkScriptMetadataEditReadResult>
) -> Int32

@_silgen_name("smk_edit_result_metadata_edit_preview_result")
private nonisolated func smk_edit_result_metadata_edit_preview_result(
    _ result: OpaquePointer?,
    _ outInfo: UnsafeMutablePointer<SmkScriptMetadataEditPreviewResult>
) -> Int32

@_silgen_name("smk_edit_result_existing_lines")
private nonisolated func smk_edit_result_existing_lines(
    _ result: OpaquePointer?,
    _ outLines: UnsafeMutablePointer<SmkUtf8SliceSlice>
) -> Int32

@_silgen_name("smk_edit_result_unknown_lines")
private nonisolated func smk_edit_result_unknown_lines(
    _ result: OpaquePointer?,
    _ outLines: UnsafeMutablePointer<SmkUtf8SliceSlice>
) -> Int32

@_silgen_name("smk_edit_result_backup_record")
private nonisolated func smk_edit_result_backup_record(
    _ result: OpaquePointer?,
    _ outHasRecord: UnsafeMutablePointer<UInt8>,
    _ outRecord: UnsafeMutablePointer<SmkScriptMetaBackupRecord>
) -> Int32

@_silgen_name("smk_edit_result_backup_generations")
private nonisolated func smk_edit_result_backup_generations(
    _ result: OpaquePointer?,
    _ outGenerations: UnsafeMutablePointer<SmkScriptMetaBackupGenerationSlice>
) -> Int32

@_silgen_name("smk_edit_result_free")
private nonisolated func smk_edit_result_free(_ result: OpaquePointer?)

private nonisolated final class ScriptMetaKitFFIEngineBox: @unchecked Sendable {
    private let lock = NSLock()
    private let cancellationLock = NSLock()
    private var engine: ScriptMetaKitFFIEngine?
    private var cancellationEngine: ScriptMetaKitFFIEngine?
    private var watchNotificationSink: ScriptMetaKitWatchNotificationSink?

    deinit {
        shutdown()
    }

    private func ensureEngineLocked() throws -> ScriptMetaKitFFIEngine {
        if let engine {
            return engine
        }
        let engine = try ScriptMetaKitFFIEngine()
        self.engine = engine
        cancellationLock.lock()
        cancellationEngine = engine
        cancellationLock.unlock()
        return engine
    }

    public func cancelCurrentOperation() {
        cancellationLock.lock()
        let engine = cancellationEngine
        cancellationLock.unlock()
        engine?.cancelCurrentOperation()
    }

    public func shutdown() {
        lock.lock()
        let engineToRelease = engine
        engineToRelease?.stopWatching()
        watchNotificationSink = nil
        engine = nil
        lock.unlock()

        cancellationLock.lock()
        cancellationEngine = nil
        cancellationLock.unlock()
    }

    public func scan(
        folderURLs: [URL],
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) throws -> ScriptMetaScanResult {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().scan(folderURLs: folderURLs, checkUpdates: checkUpdates, onProgress: onProgress)
    }

    public func checkUpdate(
        item: ScriptMetaItem,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) throws -> UpdateCheckResult {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().checkUpdate(item: item, onProgress: onProgress)
    }

    public func checkUpdates(
        items: [ScriptMetaItem],
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) throws -> UpdateCheckResult {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().checkUpdates(items: items, onProgress: onProgress)
    }

    public func setRoots(_ roots: [ScriptMetaKitRoot]) throws {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        try engine.setRoots(roots)
        try restartWatcherIfNeeded()
    }

    public func replaceRootGroup(_ roots: [ScriptMetaKitRoot], groupID: String) throws {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        try engine.replaceRootGroup(roots, groupID: groupID)
        try restartWatcherIfNeeded()
    }

    public func insertRootsIntoGroup(_ roots: [ScriptMetaKitRoot], groupID: String) throws {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        try engine.insertRootsIntoGroup(roots, groupID: groupID)
        try restartWatcherIfNeeded()
    }

    public func setVisibleRoot(_ rootID: String?) throws {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        try engine.setVisibleRoot(rootID)
        try restartWatcherIfNeeded()
    }

    public func scanRegisteredRoots(
        mode: ScriptMetaScanMode,
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) throws -> ScriptMetaScanResult {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().scanRegisteredRoots(mode: mode, checkUpdates: checkUpdates, onProgress: onProgress)
    }

    public func scanRoots(
        rootIDs: [String],
        mode: ScriptMetaScanMode,
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) throws -> ScriptMetaScanResult {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().scanRoots(rootIDs: rootIDs, mode: mode, checkUpdates: checkUpdates, onProgress: onProgress)
    }

    public func cachedRoots(
        rootIDs: [String],
        mode: ScriptMetaScanMode
    ) throws -> ScriptMetaScanResult {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().cachedRoots(rootIDs: rootIDs, mode: mode)
    }

    public func startWatching(onChange: @escaping @Sendable () -> Void) throws {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        engine.stopWatching()
        watchNotificationSink = nil
        let sink = ScriptMetaKitWatchNotificationSink(onChange: onChange)
        do {
            try engine.startWatching(notificationSink: sink)
            watchNotificationSink = sink
        } catch {
            watchNotificationSink = nil
            throw error
        }
    }

    public func startWatching(folderURLs: [URL], onChange: @escaping @Sendable () -> Void) throws {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        engine.stopWatching()
        watchNotificationSink = nil
        _ = try engine.scan(folderURLs: folderURLs, checkUpdates: false)
        let sink = ScriptMetaKitWatchNotificationSink(onChange: onChange)
        do {
            try engine.startWatching(notificationSink: sink)
            watchNotificationSink = sink
        } catch {
            watchNotificationSink = nil
            throw error
        }
    }

    public func stopWatching() {
        lock.lock()
        defer { lock.unlock() }
        engine?.stopWatching()
        watchNotificationSink = nil
    }

    private func restartWatcherIfNeeded() throws {
        guard let sink = watchNotificationSink, let engine else { return }
        engine.stopWatching()
        do {
            try engine.startWatching(notificationSink: sink)
        } catch {
            watchNotificationSink = nil
            throw error
        }
    }

    public func setResolveMacOSAlias(_ enabled: Bool) throws {
        lock.lock()
        defer { lock.unlock() }
        try ensureEngineLocked().setResolveMacOSAlias(enabled)
    }

    public func setDecompileCompiledOSADuringScan(_ enabled: Bool) throws {
        lock.lock()
        defer { lock.unlock() }
        try ensureEngineLocked().setDecompileCompiledOSADuringScan(enabled)
    }

    public func setNativeEventLatencyMillis(_ latencyMillis: UInt64) throws {
        lock.lock()
        defer { lock.unlock() }
        try ensureEngineLocked().setNativeEventLatencyMillis(latencyMillis)
        try restartWatcherIfNeeded()
    }

    public func setRootPreflightOptions(_ options: ScriptMetaRootPreflightOptions) throws {
        lock.lock()
        defer { lock.unlock() }
        try ensureEngineLocked().setRootPreflightOptions(options)
    }

    public func loadCache(from fileURL: URL) throws {
        lock.lock()
        defer { lock.unlock() }
        try ensureEngineLocked().loadCache(from: fileURL)
    }

    public func saveCache(to fileURL: URL, scope: ScriptMetaCacheScope) throws {
        lock.lock()
        defer { lock.unlock() }
        try ensureEngineLocked().saveCache(to: fileURL, scope: scope)
    }

    public func writeScriptMetadata(
        fileURL: URL,
        draft: ScriptMetadataDraft,
        mode: ScriptMetaWriteMode,
        backupRootURL: URL?
    ) throws -> ScriptMetadataFileWriteResult {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().writeScriptMetadata(
            fileURL: fileURL,
            draft: draft,
            mode: mode,
            backupRootURL: backupRootURL
        )
    }

    public func readScriptMetadataDraft(fileURL: URL) throws -> ScriptMetadataEditReadResult {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().readScriptMetadataDraft(fileURL: fileURL)
    }

    public func readScriptMetadataEditPreview(
        fileURL: URL,
        maxBytes: Int = 8 * 1024
    ) throws -> ScriptMetadataEditPreviewResult {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().readScriptMetadataEditPreview(fileURL: fileURL, maxBytes: maxBytes)
    }

    public func renderDistributionMetadata(records: [DistributionMetadataDraft]) throws -> String {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().renderDistributionMetadata(records: records)
    }

    public func generateEditPasswordSHA256(password: String) throws -> String {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().generateEditPasswordSHA256(password: password)
    }

    public func verifyEditPasswordSHA256(password: String, storedValue: String) throws -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().verifyEditPasswordSHA256(password: password, storedValue: storedValue)
    }

    public func scriptMetaBackupGenerations(
        fileURL: URL,
        backupRootURL: URL
    ) throws -> [ScriptMetaBackupGeneration] {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().scriptMetaBackupGenerations(fileURL: fileURL, backupRootURL: backupRootURL)
    }

    public func createScriptMetaBackup(
        fileURL: URL,
        backupRootURL: URL,
        reason: ScriptMetaBackupReason
    ) throws -> ScriptMetaBackupRecord {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().createScriptMetaBackup(
            fileURL: fileURL,
            backupRootURL: backupRootURL,
            reason: reason
        )
    }

    public func restoreScriptMetaBackup(
        fileURL: URL,
        backupRootURL: URL,
        generationID: String
    ) throws -> ScriptMetaBackupRecord {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().restoreScriptMetaBackup(
            fileURL: fileURL,
            backupRootURL: backupRootURL,
            generationID: generationID
        )
    }

    public func clearScriptMetaBackups(fileURL: URL, backupRootURL: URL) throws {
        lock.lock()
        defer { lock.unlock() }
        try ensureEngineLocked().clearScriptMetaBackups(fileURL: fileURL, backupRootURL: backupRootURL)
    }

    public func resetScriptMetaBackupsWithCurrentAsInitial(
        fileURL: URL,
        backupRootURL: URL
    ) throws -> ScriptMetaBackupRecord {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().resetScriptMetaBackupsWithCurrentAsInitial(
            fileURL: fileURL,
            backupRootURL: backupRootURL
        )
    }

    public func pollWatchChanges(dirtyOnly: Bool = false) throws -> ScriptMetaScanResult? {
        lock.lock()
        defer { lock.unlock() }
        guard let engine else { return nil }
        return try engine.pollWatchChanges(dirtyOnly: dirtyOnly)
    }
}

private nonisolated final class SmkInputStringArena: @unchecked Sendable {
    private var allocations: [(pointer: UnsafeMutablePointer<UInt8>, count: Int)] = []

    func slice(_ value: String?) -> SmkUtf8Slice {
        guard let value, !value.isEmpty else {
            return SmkUtf8Slice()
        }
        var mutableValue = value
        return mutableValue.withUTF8 { buffer in
            guard let baseAddress = buffer.baseAddress else {
                return SmkUtf8Slice()
            }
            let pointer = UnsafeMutablePointer<UInt8>.allocate(capacity: buffer.count)
            pointer.initialize(from: baseAddress, count: buffer.count)
            allocations.append((pointer, buffer.count))
            return SmkUtf8Slice(ptr: UnsafePointer(pointer), len: buffer.count)
        }
    }

    deinit {
        for allocation in allocations {
            allocation.pointer.deinitialize(count: allocation.count)
            allocation.pointer.deallocate()
        }
    }
}

private nonisolated final class ScriptMetaKitProgressSink: @unchecked Sendable {
    private let onProgress: @Sendable (UpdateCheckProgress) -> Void

    init(onProgress: @escaping @Sendable (UpdateCheckProgress) -> Void) {
        self.onProgress = onProgress
    }

    func emit(_ progress: UpdateCheckProgress) {
        onProgress(progress)
    }
}

private nonisolated final class ScriptMetaKitWatchNotificationSink: @unchecked Sendable {
    private let onChange: @Sendable () -> Void

    init(onChange: @escaping @Sendable () -> Void) {
        self.onChange = onChange
    }

    func emit() {
        onChange()
    }
}

private nonisolated let updateProgressCallback: SmkUpdateProgressCallback = { progressPointer, context in
    guard let progressPointer, let context else { return }
    let sink = Unmanaged<ScriptMetaKitProgressSink>.fromOpaque(context).takeUnretainedValue()
    let progress = progressPointer.assumingMemoryBound(to: SmkUpdateProgress.self).pointee
    sink.emit(updateProgress(from: progress))
}

private nonisolated let watchNotificationCallback: SmkWatchNotificationCallback = { context in
    guard let context else { return }
    let sink = Unmanaged<ScriptMetaKitWatchNotificationSink>.fromOpaque(context).takeUnretainedValue()
    sink.emit()
}

private nonisolated final class ScriptMetaKitFFIEngine: @unchecked Sendable {
    private var handle: OpaquePointer?

    init() throws {
        var engine: OpaquePointer?
        let status = smk_engine_create_default(&engine)
        guard status == smkStatusOK, let engine else {
            throw ScriptMetaKitError.engineCreationFailed(status)
        }
        handle = engine
    }

    deinit {
        smk_engine_free(handle)
    }

    public func scan(
        folderURLs: [URL],
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) throws -> ScriptMetaScanResult {
        let inputArena = SmkInputStringArena()
        let pathSlices = folderURLs.map { inputArena.slice($0.standardizedFileURL.path) }
        let progressSink = onProgress.map { ScriptMetaKitProgressSink(onProgress: $0) }
        let progressContext = progressSink.map { Unmanaged.passUnretained($0).toOpaque() }
        var result: OpaquePointer?
        let status = withExtendedLifetime(inputArena) {
            withExtendedLifetime(progressSink) {
                pathSlices.withUnsafeBufferPointer { buffer in
                    if progressSink != nil {
                        smk_engine_scan_folders_with_progress(
                            handle,
                            buffer.baseAddress,
                            buffer.count,
                            checkUpdates ? 1 : 0,
                            updateProgressCallback,
                            progressContext,
                            &result
                        )
                    } else {
                        smk_engine_scan_folders(handle, buffer.baseAddress, buffer.count, checkUpdates ? 1 : 0, &result)
                    }
                }
            }
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_scan_result_free(result)
        }
        return try makeResult(from: result)
    }

    public func setRoots(_ roots: [ScriptMetaKitRoot]) throws {
        let arena = SmkInputStringArena()
        let registrations = rootRegistrations(from: roots, arena: arena)
        let status = withExtendedLifetime(arena) {
            registrations.withUnsafeBufferPointer { buffer in
                smk_engine_set_roots(handle, buffer.baseAddress, buffer.count)
            }
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func replaceRootGroup(_ roots: [ScriptMetaKitRoot], groupID: String) throws {
        let arena = SmkInputStringArena()
        let registrations = rootRegistrations(from: roots, arena: arena)
        let status = withExtendedLifetime(arena) {
            registrations.withUnsafeBufferPointer { buffer in
                smk_engine_replace_root_group(
                    handle,
                    arena.slice(groupID),
                    buffer.baseAddress,
                    buffer.count
                )
            }
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func insertRootsIntoGroup(_ roots: [ScriptMetaKitRoot], groupID: String) throws {
        let arena = SmkInputStringArena()
        let registrations = rootRegistrations(from: roots, arena: arena)
        let status = withExtendedLifetime(arena) {
            registrations.withUnsafeBufferPointer { buffer in
                smk_engine_insert_roots_into_group(
                    handle,
                    arena.slice(groupID),
                    buffer.baseAddress,
                    buffer.count
                )
            }
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    private func rootRegistrations(
        from roots: [ScriptMetaKitRoot],
        arena: SmkInputStringArena
    ) -> [SmkRootRegistration] {
        roots.map { root in
            SmkRootRegistration(
                rootID: arena.slice(root.rootID),
                path: arena.slice(root.url.standardizedFileURL.path),
                displayName: arena.slice(root.displayName),
                purpose: root.purpose.rawValue,
                watchPolicy: root.watchPolicy.rawValue,
                cachePolicy: root.cachePolicy.rawValue,
                refreshPolicy: root.refreshPolicy.rawValue,
                priority: root.priority.rawValue
            )
        }
    }

    public func setVisibleRoot(_ rootID: String?) throws {
        let arena = SmkInputStringArena()
        let rootIDSlice = arena.slice(rootID)
        let status = withExtendedLifetime(arena) {
            smk_engine_set_visible_root(handle, rootIDSlice, rootID == nil ? 0 : 1)
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func scanRegisteredRoots(
        mode: ScriptMetaScanMode,
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) throws -> ScriptMetaScanResult {
        let progressSink = onProgress.map { ScriptMetaKitProgressSink(onProgress: $0) }
        let progressContext = progressSink.map { Unmanaged.passUnretained($0).toOpaque() }
        var result: OpaquePointer?
        let status = withExtendedLifetime(progressSink) {
            if progressSink != nil {
                smk_engine_scan_registered_roots_with_progress(
                    handle,
                    mode.rawValue,
                    checkUpdates ? 1 : 0,
                    updateProgressCallback,
                    progressContext,
                    &result
                )
            } else {
                smk_engine_scan_registered_roots(handle, mode.rawValue, checkUpdates ? 1 : 0, &result)
            }
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_scan_result_free(result)
        }
        return try makeResult(from: result)
    }

    public func scanRoots(
        rootIDs: [String],
        mode: ScriptMetaScanMode,
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) throws -> ScriptMetaScanResult {
        let arena = SmkInputStringArena()
        let rootIDSlices = rootIDs.map { arena.slice($0) }
        let progressSink = onProgress.map { ScriptMetaKitProgressSink(onProgress: $0) }
        let progressContext = progressSink.map { Unmanaged.passUnretained($0).toOpaque() }
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            withExtendedLifetime(progressSink) {
                rootIDSlices.withUnsafeBufferPointer { buffer in
                    if progressSink != nil {
                        smk_engine_scan_roots_with_progress(
                            handle,
                            buffer.baseAddress,
                            buffer.count,
                            mode.rawValue,
                            checkUpdates ? 1 : 0,
                            updateProgressCallback,
                            progressContext,
                            &result
                        )
                    } else {
                        smk_engine_scan_roots(
                            handle,
                            buffer.baseAddress,
                            buffer.count,
                            mode.rawValue,
                            checkUpdates ? 1 : 0,
                            &result
                        )
                    }
                }
            }
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_scan_result_free(result)
        }
        return try makeResult(from: result)
    }

    public func cachedRoots(
        rootIDs: [String],
        mode: ScriptMetaScanMode
    ) throws -> ScriptMetaScanResult {
        let arena = SmkInputStringArena()
        let rootIDSlices = rootIDs.map { arena.slice($0) }
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            rootIDSlices.withUnsafeBufferPointer { buffer in
                smk_engine_cached_roots(
                    handle,
                    buffer.baseAddress,
                    buffer.count,
                    mode.rawValue,
                    &result
                )
            }
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_scan_result_free(result)
        }
        return try makeResult(from: result)
    }

    public func checkUpdate(
        item: ScriptMetaItem,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) throws -> UpdateCheckResult {
        let arena = SmkInputStringArena()
        var ffiItem = smkScriptItem(from: item, arena: arena)
        let progressSink = onProgress.map { ScriptMetaKitProgressSink(onProgress: $0) }
        let progressContext = progressSink.map { Unmanaged.passUnretained($0).toOpaque() }
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            withExtendedLifetime(progressSink) {
                withUnsafePointer(to: &ffiItem) { itemPointer in
                    if progressSink != nil {
                        smk_engine_check_update_item_with_progress(
                            handle,
                            itemPointer,
                            updateProgressCallback,
                            progressContext,
                            &result
                        )
                    } else {
                        smk_engine_check_update_item(handle, itemPointer, &result)
                    }
                }
            }
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_scan_result_free(result)
        }
        guard let updateResult = try updateResult(from: result) else {
            throw ScriptMetaKitError.operationFailed(4, "update check result was not returned")
        }
        return updateResult
    }

    public func checkUpdates(
        items: [ScriptMetaItem],
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) throws -> UpdateCheckResult {
        let arena = SmkInputStringArena()
        let ffiItems = items.map { smkScriptItem(from: $0, arena: arena) }
        let progressSink = onProgress.map { ScriptMetaKitProgressSink(onProgress: $0) }
        let progressContext = progressSink.map { Unmanaged.passUnretained($0).toOpaque() }
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            withExtendedLifetime(progressSink) {
                ffiItems.withUnsafeBufferPointer { buffer in
                    if progressSink != nil {
                        smk_engine_check_updates_for_items_with_progress(
                            handle,
                            buffer.baseAddress,
                            buffer.count,
                            updateProgressCallback,
                            progressContext,
                            &result
                        )
                    } else {
                        smk_engine_check_updates_for_items(
                            handle,
                            buffer.baseAddress,
                            buffer.count,
                            &result
                        )
                    }
                }
            }
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_scan_result_free(result)
        }
        guard let updateResult = try updateResult(from: result) else {
            throw ScriptMetaKitError.operationFailed(4, "update check result was not returned")
        }
        return updateResult
    }

    public func setResolveMacOSAlias(_ enabled: Bool) throws {
        let status = smk_engine_set_resolve_macos_alias(handle, enabled ? 1 : 0)
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func setDecompileCompiledOSADuringScan(_ enabled: Bool) throws {
        let status = smk_engine_set_decompile_compiled_osa_during_scan(handle, enabled ? 1 : 0)
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func setNativeEventLatencyMillis(_ latencyMillis: UInt64) throws {
        let status = smk_engine_set_native_event_latency_millis(handle, latencyMillis)
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func setRootPreflightOptions(_ options: ScriptMetaRootPreflightOptions) throws {
        guard options.maxScannedItems >= 0,
              options.minScannedFileCountForLargeRoot >= 0,
              options.minScriptRatioDenominator >= 0,
              options.minScannedItemsForTimeLimit >= 0 else {
            throw ScriptMetaKitError.operationFailed(2, "root preflight thresholds must not be negative")
        }
        let status = smk_engine_set_root_preflight_options(
            handle,
            options.rejectTrashRoots ? 1 : 0,
            options.rejectRestrictedRoots ? 1 : 0,
            options.rejectLowScriptDensityLargeRoots ? 1 : 0,
            options.maxScannedItems,
            options.maxDurationMillis,
            options.minScannedFileCountForLargeRoot,
            options.minScriptRatioDenominator,
            options.minScannedItemsForTimeLimit
        )
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func loadCache(from fileURL: URL) throws {
        let arena = SmkInputStringArena()
        let status = withExtendedLifetime(arena) {
            smk_engine_load_cache_file(handle, arena.slice(fileURL.standardizedFileURL.path))
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func saveCache(to fileURL: URL, scope: ScriptMetaCacheScope) throws {
        let arena = SmkInputStringArena()
        let status = withExtendedLifetime(arena) {
            smk_engine_save_cache_file(handle, scope.rawValue, arena.slice(fileURL.standardizedFileURL.path))
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func cancelCurrentOperation() {
        _ = smk_engine_cancel_current_operation(handle)
    }

    public func writeScriptMetadata(
        fileURL: URL,
        draft: ScriptMetadataDraft,
        mode: ScriptMetaWriteMode,
        backupRootURL: URL?
    ) throws -> ScriptMetadataFileWriteResult {
        let arena = SmkInputStringArena()
        var request = SmkScriptMetadataWriteRequest(
            filePath: arena.slice(fileURL.standardizedFileURL.path),
            backupRootPath: arena.slice(backupRootURL?.standardizedFileURL.path),
            writeMode: mode.ffiValue,
            draft: smkScriptMetadataDraft(from: draft, arena: arena)
        )
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            withUnsafePointer(to: &request) { requestPointer in
                smk_engine_write_script_metadata_file(handle, requestPointer, &result)
            }
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_edit_result_free(result)
        }
        var writeResult = SmkScriptMetadataFileWriteResult()
        try check(smk_edit_result_file_write_result(result, &writeResult))
        return scriptMetadataFileWriteResult(from: writeResult)
    }

    public func readScriptMetadataDraft(fileURL: URL) throws -> ScriptMetadataEditReadResult {
        let arena = SmkInputStringArena()
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            smk_engine_read_script_metadata_draft_file(
                handle,
                arena.slice(fileURL.standardizedFileURL.path),
                &result
            )
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_edit_result_free(result)
        }
        var readResult = SmkScriptMetadataEditReadResult()
        try check(smk_edit_result_metadata_edit_read_result(result, &readResult))
        var existingLineSlice = SmkUtf8SliceSlice()
        try check(smk_edit_result_existing_lines(result, &existingLineSlice))
        var unknownLineSlice = SmkUtf8SliceSlice()
        try check(smk_edit_result_unknown_lines(result, &unknownLineSlice))
        return scriptMetadataEditReadResult(
            from: readResult,
            existingLines: array(from: existingLineSlice).map(string),
            unknownLines: array(from: unknownLineSlice).map(string)
        )
    }

    public func readScriptMetadataEditPreview(
        fileURL: URL,
        maxBytes: Int
    ) throws -> ScriptMetadataEditPreviewResult {
        let arena = SmkInputStringArena()
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            smk_engine_read_script_metadata_edit_preview_file(
                handle,
                arena.slice(fileURL.standardizedFileURL.path),
                maxBytes,
                &result
            )
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_edit_result_free(result)
        }
        var previewResult = SmkScriptMetadataEditPreviewResult()
        try check(smk_edit_result_metadata_edit_preview_result(result, &previewResult))
        return scriptMetadataEditPreviewResult(from: previewResult)
    }

    public func renderDistributionMetadata(records: [DistributionMetadataDraft]) throws -> String {
        let arena = SmkInputStringArena()
        let ffiRecords = records.map { smkDistributionMetadataDraft(from: $0, arena: arena) }
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            ffiRecords.withUnsafeBufferPointer { buffer in
                smk_engine_render_distribution_metadata(handle, buffer.baseAddress, buffer.count, &result)
            }
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_edit_result_free(result)
        }
        var text = SmkUtf8Slice()
        try check(smk_edit_result_text(result, &text))
        return string(text)
    }

    public func generateEditPasswordSHA256(password: String) throws -> String {
        let arena = SmkInputStringArena()
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            smk_engine_generate_edit_password_sha256(
                handle,
                arena.slice(password),
                &result
            )
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_edit_result_free(result)
        }
        var text = SmkUtf8Slice()
        try check(smk_edit_result_text(result, &text))
        return string(text)
    }

    public func verifyEditPasswordSHA256(password: String, storedValue: String) throws -> Bool {
        let arena = SmkInputStringArena()
        var isMatch: UInt8 = 0
        let status = withExtendedLifetime(arena) {
            smk_engine_verify_edit_password_sha256(
                handle,
                arena.slice(password),
                arena.slice(storedValue),
                &isMatch
            )
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        return isMatch != 0
    }

    public func scriptMetaBackupGenerations(
        fileURL: URL,
        backupRootURL: URL
    ) throws -> [ScriptMetaBackupGeneration] {
        let arena = SmkInputStringArena()
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            smk_engine_scriptmeta_backup_generations(
                handle,
                arena.slice(fileURL.standardizedFileURL.path),
                arena.slice(backupRootURL.standardizedFileURL.path),
                &result
            )
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_edit_result_free(result)
        }
        var generationSlice = SmkScriptMetaBackupGenerationSlice()
        try check(smk_edit_result_backup_generations(result, &generationSlice))
        return array(from: generationSlice).map(scriptMetaBackupGeneration(from:))
    }

    public func createScriptMetaBackup(
        fileURL: URL,
        backupRootURL: URL,
        reason: ScriptMetaBackupReason
    ) throws -> ScriptMetaBackupRecord {
        guard let reasonValue = reason.ffiValue else {
            throw ScriptMetaKitError.operationFailed(3, "backup reason is unknown")
        }
        let arena = SmkInputStringArena()
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            smk_engine_create_scriptmeta_backup(
                handle,
                arena.slice(fileURL.standardizedFileURL.path),
                arena.slice(backupRootURL.standardizedFileURL.path),
                reasonValue,
                &result
            )
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_edit_result_free(result)
        }
        return try backupRecord(from: result)
    }

    public func restoreScriptMetaBackup(
        fileURL: URL,
        backupRootURL: URL,
        generationID: String
    ) throws -> ScriptMetaBackupRecord {
        let arena = SmkInputStringArena()
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            smk_engine_restore_scriptmeta_backup(
                handle,
                arena.slice(fileURL.standardizedFileURL.path),
                arena.slice(backupRootURL.standardizedFileURL.path),
                arena.slice(generationID),
                &result
            )
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_edit_result_free(result)
        }
        return try backupRecord(from: result)
    }

    public func clearScriptMetaBackups(fileURL: URL, backupRootURL: URL) throws {
        let arena = SmkInputStringArena()
        let status = withExtendedLifetime(arena) {
            smk_engine_clear_scriptmeta_backups(
                handle,
                arena.slice(fileURL.standardizedFileURL.path),
                arena.slice(backupRootURL.standardizedFileURL.path)
            )
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func resetScriptMetaBackupsWithCurrentAsInitial(
        fileURL: URL,
        backupRootURL: URL
    ) throws -> ScriptMetaBackupRecord {
        let arena = SmkInputStringArena()
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            smk_engine_reset_scriptmeta_backups_with_current_as_initial(
                handle,
                arena.slice(fileURL.standardizedFileURL.path),
                arena.slice(backupRootURL.standardizedFileURL.path),
                &result
            )
        }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        defer {
            smk_edit_result_free(result)
        }
        return try backupRecord(from: result)
    }

    public func startWatching(notificationSink: ScriptMetaKitWatchNotificationSink?) throws {
        let notificationContext = notificationSink.map { Unmanaged.passUnretained($0).toOpaque() }
        let status = smk_engine_start_watching_with_callback(
            handle,
            notificationSink == nil ? nil : watchNotificationCallback,
            notificationContext
        )
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func stopWatching() {
        _ = smk_engine_stop_watching(handle)
    }

    public func pollWatchChanges(dirtyOnly: Bool = false) throws -> ScriptMetaScanResult? {
        var changed: UInt8 = 0
        var result: OpaquePointer?
        let status = if dirtyOnly {
            smk_engine_poll_watcher_scan_dirty_only(handle, &changed, &result)
        } else {
            smk_engine_poll_watcher_scan(handle, &changed, &result)
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        guard changed != 0, let result else { return nil }
        defer {
            smk_scan_result_free(result)
        }
        return try makeResult(from: result)
    }

    private func lastErrorMessage() -> String {
        var message = SmkUtf8Slice()
        let status = smk_engine_last_error(handle, &message)
        guard status == smkStatusOK else { return "" }
        return string(message)
    }
}

private nonisolated func makeResult(from result: OpaquePointer) throws -> ScriptMetaScanResult {
    var rootSlice = SmkRootSnapshotSlice()
    try check(smk_scan_result_roots(result, &rootSlice))
    let roots = buffer(from: rootSlice).map { root -> RootSnapshot in
        let code = string(root.errorCode)
        let message = string(root.errorMessage)
        let error = code.isEmpty && message.isEmpty ? nil : RootError(code: code, message: message)
        return RootSnapshot(
            rootID: string(root.rootID),
            path: string(root.path),
            status: string(root.status),
            isDirty: root.isDirty != 0,
            lastLoadedAt: root.hasLastLoadedAt != 0 ? root.lastLoadedAt : nil,
            lastEventAt: root.hasLastEventAt != 0 ? root.lastEventAt : nil,
            itemCount: root.itemCount,
            error: error
        )
    }

    var fileEntrySlice = SmkFileEntrySlice()
    try check(smk_scan_result_file_entries(result, &fileEntrySlice))
    let ffiFileEntries = buffer(from: fileEntrySlice)

    var fileListSlice = SmkFileListSnapshotSlice()
    try check(smk_scan_result_file_lists(result, &fileListSlice))
    let fileListSnapshots = buffer(from: fileListSlice).map { snapshot -> FileListSnapshot in
        let root = roots.indices.contains(snapshot.rootIndex)
            ? roots[snapshot.rootIndex]
            : RootSnapshot(
                rootID: "",
                path: "",
                status: "missing",
                isDirty: false,
                lastLoadedAt: nil,
                lastEventAt: nil,
                itemCount: 0,
                error: RootError(code: "invalid_root_index", message: "invalid root index returned by SCRIPTMETAKit")
            )
        return FileListSnapshot(
            root: root,
            children: fileEntries(
                from: ffiFileEntries,
                firstIndex: snapshot.firstChildIndex,
                count: snapshot.childCount
            ),
            directoryStates: [:],
            truncated: snapshot.truncated != 0
        )
    }

    var itemSlice = SmkScriptItemSlice()
    try check(smk_scan_result_items(result, &itemSlice))
    let allItems = scriptItems(from: buffer(from: itemSlice))

    var fileItemSlice = SmkScriptItemSlice()
    try check(smk_scan_result_file_items(result, &fileItemSlice))
    let fileItems = scriptItems(from: buffer(from: fileItemSlice))

    let updateCheckResult = try updateResult(from: result)
    let changeSummary = try scanChangeSummary(from: result)
    let operationInfo = try operationInfo(from: result)
    let fileIssues = try fileIssues(from: result)
    let catalogInfo = try catalogInfo(from: result)
    let hasCatalog = catalogInfo.hasCatalog != 0
    let catalog: ScriptMetaCatalogSnapshot?
    if hasCatalog {
        catalog = ScriptMetaCatalogSnapshot(
            sourceRevision: string(catalogInfo.sourceRevision),
            roots: roots,
            allItems: allItems,
            fileItems: fileItems,
            candidateCache: CandidateCache(
                schemaVersion: catalogInfo.candidateCacheSchemaVersion,
                builtAt: catalogInfo.candidateCacheBuiltAt,
                registeredRoots: try registeredRootSignatures(from: result),
                records: try candidateRecords(from: result)
            ),
            updateCheckResult: updateCheckResult
        )
    } else {
        catalog = nil
    }

    return ScriptMetaScanResult(
        roots: roots,
        fileListSnapshots: fileListSnapshots,
        catalogSnapshot: catalog,
        operation: operationInfo,
        fileIssues: fileIssues,
        updateCheckResult: updateCheckResult,
        changeSummary: changeSummary,
        watchChangeBatch: try watchChangeBatch(from: result)
    )
}

private nonisolated func catalogInfo(from result: OpaquePointer) throws -> SmkCatalogInfo {
    var info = SmkCatalogInfo()
    try check(smk_scan_result_catalog_info(result, &info))
    return info
}

private nonisolated func registeredRootSignatures(from result: OpaquePointer) throws -> [RegisteredRootSignature] {
    var rootSlice = SmkRegisteredRootSignatureSlice()
    try check(smk_scan_result_registered_root_signatures(result, &rootSlice))
    return array(from: rootSlice).map { signature in
        RegisteredRootSignature(
            rootID: string(signature.rootID),
            path: string(signature.path)
        )
    }
}

private nonisolated func candidateRecords(from result: OpaquePointer) throws -> [CandidateRecord] {
    var recordSlice = SmkCandidateRecordSlice()
    try check(smk_scan_result_candidate_records(result, &recordSlice))
    return buffer(from: recordSlice).map { record in
        CandidateRecord(
            rootID: string(record.rootID),
            rootPath: string(record.rootPath),
            filePath: string(record.filePath),
            identityPath: string(record.identityPath),
            pathKind: optionalString(record.pathKind),
            resolutionStatus: optionalString(record.resolutionStatus),
            resolutionMessage: optionalString(record.resolutionMessage),
            runtimeKind: optionalString(record.runtimeKind),
            shebang: optionalString(record.shebang),
            hasScriptMeta: record.hasScriptMeta != 0,
            hasScriptMetaEditPassword: record.hasScriptMetaEditPassword != 0,
            isFileLocked: record.isFileLocked != 0,
            isReadOnly: record.isReadOnly != 0,
            canEditScriptMeta: record.canEditScriptMeta != 0,
            canAppendScriptMeta: record.canAppendScriptMeta != 0,
            scriptMetaEditState: optionalString(record.scriptMetaEditState),
            fileSize: record.hasFileSize != 0 ? record.fileSize : nil,
            contentModifiedAt: record.hasContentModifiedAt != 0 ? record.contentModifiedAt : nil,
            item: record.hasItem != 0 ? scriptItem(from: record.item) : nil
        )
    }
}

private nonisolated func scriptItems(from ffiItems: UnsafeBufferPointer<SmkScriptItem>) -> [ScriptMetaItem] {
    ffiItems.map(scriptItem(from:))
}

private nonisolated func scriptItem(from item: SmkScriptItem) -> ScriptMetaItem {
    ScriptMetaItem(
        rootID: string(item.rootID),
        filePath: string(item.filePath),
        identityPath: string(item.identityPath),
        runtimeKind: optionalString(item.runtimeKind),
        shebang: optionalString(item.shebang),
        scriptID: string(item.scriptID),
        version: optionalString(item.version),
        itemDescription: optionalString(item.description),
        targetApp: optionalString(item.targetApp),
        minTargetVersion: optionalString(item.minTargetVersion),
        metaURL: optionalString(item.metaURL),
        name: optionalString(item.name),
        author: optionalString(item.author),
        releaseDate: optionalString(item.releaseDate),
        editPasswordSHA256: optionalString(item.editPasswordSHA256),
        hasScriptMeta: item.hasScriptMeta != 0,
        hasScriptMetaEditPassword: item.hasScriptMetaEditPassword != 0,
        isFileLocked: item.isFileLocked != 0,
        isReadOnly: item.isReadOnly != 0,
        canEditScriptMeta: item.canEditScriptMeta != 0,
        canAppendScriptMeta: item.canAppendScriptMeta != 0,
        scriptMetaEditState: string(item.scriptMetaEditState)
    )
}

private nonisolated func scanChangeSummary(from result: OpaquePointer) throws -> ScanChangeSummary? {
    var changeInfo = SmkScanChangeInfo()
    try check(smk_scan_result_change_info(result, &changeInfo))
    guard changeInfo.hasChangeSummary != 0 else { return nil }

    var changeSlice = SmkFileEntryChangeSlice()
    try check(smk_scan_result_file_entry_changes(result, &changeSlice))
    let changes = array(from: changeSlice).map { change in
        FileEntryChange(
            rootID: string(change.rootID),
            kind: string(change.kind),
            displayPath: string(change.displayPath),
            resolvedPath: string(change.resolvedPath),
            pathKind: string(change.pathKind),
            resolutionStatus: string(change.resolutionStatus),
            resolutionMessage: optionalString(change.resolutionMessage),
            isDirectory: change.isDirectory != 0,
            fileSize: change.hasFileSize != 0 ? change.fileSize : nil,
            contentModifiedAt: change.hasContentModifiedAt != 0 ? change.contentModifiedAt : nil,
            identity: change.hasIdentity != 0 ? fileIdentity(from: change.identity) : nil,
            runtimeKind: optionalString(change.runtimeKind),
            shebang: optionalString(change.shebang),
            hasScriptMeta: change.hasScriptMeta != 0,
            hasScriptMetaEditPassword: change.hasScriptMetaEditPassword != 0,
            isFileLocked: change.isFileLocked != 0,
            isReadOnly: change.isReadOnly != 0,
            canEditScriptMeta: change.canEditScriptMeta != 0,
            canAppendScriptMeta: change.canAppendScriptMeta != 0,
            scriptMetaEditState: string(change.scriptMetaEditState)
        )
    }

    return ScanChangeSummary(
        addedCount: changeInfo.addedCount,
        removedCount: changeInfo.removedCount,
        modifiedCount: changeInfo.modifiedCount,
        changes: changes
    )
}

private nonisolated func updateResult(from result: OpaquePointer) throws -> UpdateCheckResult? {
    var updateInfo = SmkUpdateCheckInfo()
    try check(smk_scan_result_update_info(result, &updateInfo))
    guard updateInfo.hasUpdateCheck != 0 else { return nil }

    var statusSlice = SmkUpdateStatusEntrySlice()
    try check(smk_scan_result_update_statuses(result, &statusSlice))
    let statuses = Dictionary(uniqueKeysWithValues: array(from: statusSlice).map {
        (string($0.itemID), string($0.status))
    })

    var historySlice = SmkUtf8SliceSlice()
    try check(smk_scan_result_latest_url_history_urls(result, &historySlice))
    let historyURLs = array(from: historySlice).map(string)

    var resolutionSlice = SmkDistributionResolutionEntrySlice()
    try check(smk_scan_result_update_resolutions(result, &resolutionSlice))
    let resolutions = Dictionary(uniqueKeysWithValues: array(from: resolutionSlice).map { resolution in
        let historyRange = safeRange(
            start: resolution.firstLatestURLHistoryIndex,
            count: resolution.latestURLHistoryCount,
            upperBound: historyURLs.count
        )
        let latestURLHistory = historyRange.map { Array(historyURLs[$0]) } ?? []
        return (
            string(resolution.itemID),
            DistributionResolution(
                latestVersion: optionalString(resolution.latestVersion),
                latestPageURL: optionalString(resolution.latestPageURL),
                finalPageURL: string(resolution.finalPageURL),
                latestURLHistory: latestURLHistory,
                checkedAt: resolution.checkedAt,
                isUnresolved: resolution.isUnresolved != 0,
                note: optionalString(resolution.note),
                redirectCount: resolution.hasRedirectCount != 0 ? resolution.redirectCount : nil
            )
        )
    })

    var failureSlice = SmkUpdateFailureEntrySlice()
    try check(smk_scan_result_update_failures(result, &failureSlice))
    let failures = Dictionary(uniqueKeysWithValues: array(from: failureSlice).map { failure in
        (
            string(failure.itemID),
            UpdateFailure(
                code: string(failure.code),
                message: string(failure.message),
                itemID: string(failure.itemID),
                filePath: string(failure.filePath),
                scriptID: string(failure.scriptID),
                currentVersion: optionalString(failure.currentVersion),
                metaURL: optionalString(failure.metaURL),
                sourceURL: optionalString(failure.sourceURL),
                checkedAt: failure.checkedAt
            )
        )
    })

    var errorSlice = SmkUpdateErrorEntrySlice()
    try check(smk_scan_result_update_errors(result, &errorSlice))
    let errors = Dictionary(uniqueKeysWithValues: array(from: errorSlice).map {
        (string($0.itemID), string($0.message))
    })

    return UpdateCheckResult(
        checkedAt: updateInfo.checkedAt,
        operation: try operationInfo(from: result),
        resolutionsByItemID: resolutions,
        failuresByItemID: failures,
        errorsByItemID: errors,
        statusesByItemID: statuses
    )
}

private nonisolated func operationInfo(from result: OpaquePointer) throws -> OperationInfo? {
    var info = SmkOperationInfo()
    try check(smk_scan_result_operation_info(result, &info))
    let status = string(info.status)
    guard !status.isEmpty else { return nil }
    return OperationInfo(
        status: status,
        totalUnits: info.totalUnits,
        completedUnits: info.completedUnits,
        failedUnits: info.failedUnits,
        cancelled: info.cancelled != 0,
        timedOut: info.timedOut != 0,
        reasonCode: optionalString(info.reasonCode),
        message: optionalString(info.message)
    )
}

private nonisolated func fileIssues(from result: OpaquePointer) throws -> [FileIssue] {
    var issueSlice = SmkFileIssueSlice()
    try check(smk_scan_result_file_issues(result, &issueSlice))
    return array(from: issueSlice).map { issue in
        FileIssue(
            rootID: issue.hasRootID != 0 ? string(issue.rootID) : nil,
            path: string(issue.path),
            code: string(issue.code),
            message: string(issue.message),
            pathKind: optionalString(issue.pathKind),
            resolutionStatus: optionalString(issue.resolutionStatus),
            isDirectory: issue.isDirectory != 0
        )
    }
}

private nonisolated func watchChangeBatch(from result: OpaquePointer) throws -> WatchChangeBatch? {
    var info = SmkWatchChangeInfo()
    try check(smk_scan_result_watch_change_info(result, &info))
    guard info.hasWatchChange != 0 else { return nil }

    var eventSlice = SmkWatchPathEventSlice()
    try check(smk_scan_result_watch_events(result, &eventSlice))
    let events = array(from: eventSlice).map { event in
        WatchPathEvent(
            rootID: string(event.rootID),
            path: string(event.path),
            kind: string(event.kind),
            isDirectory: event.isDirectory != 0,
            rescanDirectory: string(event.rescanDirectory)
        )
    }

    var ignoredSlice = SmkIgnoredWatchPathSlice()
    try check(smk_scan_result_ignored_watch_paths(result, &ignoredSlice))
    let ignoredPaths = array(from: ignoredSlice).map { ignored in
        IgnoredWatchPath(
            rootID: ignored.hasRootID != 0 ? string(ignored.rootID) : nil,
            path: string(ignored.path),
            reason: string(ignored.reason)
        )
    }

    var renameSlice = SmkWatchRenameCandidateSlice()
    try check(smk_scan_result_watch_rename_candidates(result, &renameSlice))
    let renameCandidates = array(from: renameSlice).map { candidate in
        WatchRenameCandidate(
            rootID: string(candidate.rootID),
            oldPath: string(candidate.oldPath),
            newPath: string(candidate.newPath),
            confidence: string(candidate.confidence)
        )
    }

    var targetSlice = SmkWatchRescanTargetSlice()
    try check(smk_scan_result_watch_rescan_targets(result, &targetSlice))
    let rescanTargets = array(from: targetSlice).map { target in
        WatchRescanTarget(
            rootID: string(target.rootID),
            path: string(target.path),
            reason: string(target.reason)
        )
    }

    return WatchChangeBatch(
        overflowed: info.overflowed != 0,
        events: events,
        ignoredPaths: ignoredPaths,
        renameCandidates: renameCandidates,
        rescanTargets: rescanTargets
    )
}

private nonisolated func smkScriptMetadataDraft(
    from draft: ScriptMetadataDraft,
    arena: SmkInputStringArena
) -> SmkScriptMetadataDraft {
    SmkScriptMetadataDraft(
        scriptID: arena.slice(draft.scriptID),
        version: arena.slice(draft.version),
        description: arena.slice(draft.itemDescription),
        targetApp: arena.slice(draft.targetApp),
        minTargetVersion: arena.slice(draft.minTargetVersion),
        metaURL: arena.slice(draft.metaURL),
        name: arena.slice(draft.name),
        author: arena.slice(draft.author),
        releaseDate: arena.slice(draft.releaseDate),
        editPasswordSHA256: arena.slice(draft.editPasswordSHA256)
    )
}

private nonisolated func smkScriptItem(
    from item: ScriptMetaItem,
    arena: SmkInputStringArena
) -> SmkScriptItem {
    SmkScriptItem(
        rootID: arena.slice(item.rootID),
        filePath: arena.slice(item.filePath),
        identityPath: arena.slice(item.identityPath),
        runtimeKind: arena.slice(item.runtimeKind),
        shebang: arena.slice(item.shebang),
        scriptID: arena.slice(item.scriptID),
        version: arena.slice(item.version),
        name: arena.slice(item.name),
        description: arena.slice(item.itemDescription),
        targetApp: arena.slice(item.targetApp),
        minTargetVersion: arena.slice(item.minTargetVersion),
        metaURL: arena.slice(item.metaURL),
        author: arena.slice(item.author),
        releaseDate: arena.slice(item.releaseDate),
        editPasswordSHA256: arena.slice(item.editPasswordSHA256),
        hasScriptMeta: item.hasScriptMeta ? 1 : 0,
        hasScriptMetaEditPassword: item.hasScriptMetaEditPassword ? 1 : 0,
        isFileLocked: item.isFileLocked ? 1 : 0,
        isReadOnly: item.isReadOnly ? 1 : 0,
        canEditScriptMeta: item.canEditScriptMeta ? 1 : 0,
        canAppendScriptMeta: item.canAppendScriptMeta ? 1 : 0,
        scriptMetaEditState: arena.slice(item.scriptMetaEditState)
    )
}

private nonisolated func smkDistributionMetadataDraft(
    from draft: DistributionMetadataDraft,
    arena: SmkInputStringArena
) -> SmkDistributionMetadataDraft {
    SmkDistributionMetadataDraft(
        scriptID: arena.slice(draft.scriptID),
        version: arena.slice(draft.version),
        latestURL: arena.slice(draft.latestURL),
        latestPageURL: arena.slice(draft.latestPageURL)
    )
}

private nonisolated func scriptMetadataFileWriteResult(
    from result: SmkScriptMetadataFileWriteResult
) -> ScriptMetadataFileWriteResult {
    ScriptMetadataFileWriteResult(
        filePath: string(result.filePath),
        operation: ScriptMetaWriteOperation(rawValue: string(result.operation)) ?? .unknown,
        backup: result.hasBackup != 0 ? scriptMetaBackupRecord(from: result.backup) : nil
    )
}

private nonisolated func scriptMetadataEditReadResult(
    from result: SmkScriptMetadataEditReadResult,
    existingLines: [String],
    unknownLines: [String]
) -> ScriptMetadataEditReadResult {
    ScriptMetadataEditReadResult(
        filePath: string(result.filePath),
        draft: scriptMetadataDraft(from: result.draft),
        commentStyle: string(result.commentStyle),
        lineEnding: string(result.lineEnding),
        hasExistingBlock: result.hasExistingBlock != 0,
        existingLines: existingLines,
        unknownLines: unknownLines,
        existingBlockText: optionalString(result.existingBlockText),
        sourceFingerprint: string(result.sourceFingerprint)
    )
}

private nonisolated func scriptMetadataEditPreviewResult(
    from result: SmkScriptMetadataEditPreviewResult
) -> ScriptMetadataEditPreviewResult {
    ScriptMetadataEditPreviewResult(
        filePath: string(result.filePath),
        previewText: string(result.previewText),
        previewByteCount: result.previewByteCount,
        fileSize: result.hasFileSize != 0 ? result.fileSize : nil,
        commentStyle: optionalString(result.commentStyle),
        lineEnding: string(result.lineEnding),
        hasScriptmetaMarkerInPreview: result.hasScriptmetaMarkerInPreview != 0,
        isTruncated: result.isTruncated != 0,
        requiresFullRead: result.requiresFullRead != 0,
        fileStateFingerprint: string(result.fileStateFingerprint)
    )
}

private nonisolated func scriptMetadataDraft(from draft: SmkScriptMetadataDraft) -> ScriptMetadataDraft {
    ScriptMetadataDraft(
        scriptID: string(draft.scriptID),
        version: optionalString(draft.version),
        itemDescription: optionalString(draft.description),
        targetApp: optionalString(draft.targetApp),
        minTargetVersion: optionalString(draft.minTargetVersion),
        metaURL: optionalString(draft.metaURL),
        name: optionalString(draft.name),
        author: optionalString(draft.author),
        releaseDate: optionalString(draft.releaseDate),
        editPasswordSHA256: optionalString(draft.editPasswordSHA256)
    )
}

private nonisolated func normalizeVersionStringViaFFI(_ value: String) throws -> String? {
    let arena = SmkInputStringArena()
    var hasVersion: UInt8 = 0
    var result: OpaquePointer?
    let status = withExtendedLifetime(arena) {
        smk_normalize_version_string(arena.slice(value), &hasVersion, &result)
    }
    guard status == smkStatusOK else {
        throw ScriptMetaKitError.operationFailed(status, "SCRIPTMETAKit version normalization failed.")
    }
    guard hasVersion != 0, let result else {
        return nil
    }
    defer {
        smk_edit_result_free(result)
    }
    var text = SmkUtf8Slice()
    try check(smk_edit_result_text(result, &text))
    return string(text)
}

private nonisolated func validateVersionStringViaFFI(_ value: String) throws -> Bool {
    let arena = SmkInputStringArena()
    var isValid: UInt8 = 0
    let status = withExtendedLifetime(arena) {
        smk_validate_version_string(arena.slice(value), &isValid)
    }
    guard status == smkStatusOK else {
        throw ScriptMetaKitError.operationFailed(status, "SCRIPTMETAKit version validation failed.")
    }
    return isValid != 0
}

private nonisolated func compareVersionsViaFFI(_ lhs: String, _ rhs: String) throws -> ScriptMetaVersionOrdering {
    let arena = SmkInputStringArena()
    var ordering: Int32 = 0
    let status = withExtendedLifetime(arena) {
        smk_compare_versions(arena.slice(lhs), arena.slice(rhs), &ordering)
    }
    guard status == smkStatusOK else {
        throw ScriptMetaKitError.operationFailed(status, "SCRIPTMETAKit version comparison failed.")
    }
    guard let result = ScriptMetaVersionOrdering(rawValue: ordering) else {
        throw ScriptMetaKitError.operationFailed(4, "SCRIPTMETAKit version comparison returned an invalid value.")
    }
    return result
}

private nonisolated func validateEditPasswordSHA256FormatViaFFI(_ value: String) throws -> Bool {
    let arena = SmkInputStringArena()
    var isValid: UInt8 = 0
    let status = withExtendedLifetime(arena) {
        smk_validate_edit_password_sha256_format(arena.slice(value), &isValid)
    }
    guard status == smkStatusOK else {
        throw ScriptMetaKitError.operationFailed(status, "SCRIPTMETAKit edit password SHA256 validation failed.")
    }
    return isValid != 0
}

private nonisolated func validateScriptIDUniquenessViaFFI(
    in items: [ScriptIdUniquenessItem]
) throws -> ScriptIdUniquenessReport {
    let arena = SmkInputStringArena()
    let ffiItems = scriptItemsForUniqueness(from: items, arena: arena)
    var result: OpaquePointer?
    let status = withExtendedLifetime(arena) {
        ffiItems.withUnsafeBufferPointer { buffer in
            smk_validate_script_id_uniqueness(
                buffer.baseAddress,
                buffer.count,
                &result
            )
        }
    }
    guard status == smkStatusOK, let result else {
        throw ScriptMetaKitError.operationFailed(status, "SCRIPTMETAKit Script-ID uniqueness validation failed.")
    }
    defer {
        smk_script_id_uniqueness_result_free(result)
    }
    return try scriptIDUniquenessReport(from: result)
}

private nonisolated func scriptItemsForUniqueness(
    from items: [ScriptIdUniquenessItem],
    arena: SmkInputStringArena
) -> [SmkScriptIdUniquenessItem] {
    items.map { item in
        SmkScriptIdUniquenessItem(
            itemID: arena.slice(item.itemID),
            filePath: arena.slice(item.filePath),
            scriptID: arena.slice(item.scriptID)
        )
    }
}

private nonisolated func scriptIDUniquenessReport(from result: OpaquePointer) throws -> ScriptIdUniquenessReport {
    var report = SmkScriptIdUniquenessReport()
    try check(smk_script_id_uniqueness_result_report(result, &report))

    var duplicateSlice = SmkScriptIdDuplicateSlice()
    try check(smk_script_id_uniqueness_result_duplicates(result, &duplicateSlice))

    var itemIDSlice = SmkUtf8SliceSlice()
    try check(smk_script_id_uniqueness_result_item_ids(result, &itemIDSlice))
    let itemIDs = array(from: itemIDSlice).map(string)

    var filePathSlice = SmkUtf8SliceSlice()
    try check(smk_script_id_uniqueness_result_file_paths(result, &filePathSlice))
    let filePaths = array(from: filePathSlice).map(string)

    var duplicates: [ScriptIdDuplicate] = []
    duplicates.reserveCapacity(report.duplicateCount)
    for duplicate in array(from: duplicateSlice) {
        guard let itemIDRange = safeRange(
            start: duplicate.firstItemIDIndex,
            count: duplicate.itemIDCount,
            upperBound: itemIDs.count
        ),
            let filePathRange = safeRange(
                start: duplicate.firstFilePathIndex,
                count: duplicate.filePathCount,
                upperBound: filePaths.count
            ) else {
            throw ScriptMetaKitError.operationFailed(4, "invalid Script-ID uniqueness result")
        }
        duplicates.append(
            ScriptIdDuplicate(
                scriptID: string(duplicate.scriptID),
                itemIDs: Array(itemIDs[itemIDRange]),
                filePaths: Array(filePaths[filePathRange])
            )
        )
    }

    return ScriptIdUniquenessReport(
        totalItems: report.totalItems,
        uniqueScriptIDs: report.uniqueScriptIDs,
        duplicates: duplicates
    )
}

private nonisolated func fileIdentity(from identity: SmkFileIdentity) -> FileIdentity {
    FileIdentity(
        stableID: string(identity.stableID),
        volumeID: optionalString(identity.volumeID),
        fileID: optionalString(identity.fileID),
        fileSize: identity.hasFileSize != 0 ? identity.fileSize : nil,
        contentModifiedAt: identity.hasContentModifiedAt != 0 ? identity.contentModifiedAt : nil
    )
}

private nonisolated func backupRecord(from result: OpaquePointer) throws -> ScriptMetaBackupRecord {
    var hasRecord: UInt8 = 0
    var record = SmkScriptMetaBackupRecord()
    try check(smk_edit_result_backup_record(result, &hasRecord, &record))
    guard hasRecord != 0 else {
        throw ScriptMetaKitError.operationFailed(4, "backup record was not returned")
    }
    return scriptMetaBackupRecord(from: record)
}

private nonisolated func scriptMetaBackupRecord(
    from record: SmkScriptMetaBackupRecord
) -> ScriptMetaBackupRecord {
    ScriptMetaBackupRecord(
        id: string(record.id),
        createdAtMillis: record.createdAtMillis,
        backupFileName: string(record.backupFileName),
        backupFilePath: string(record.backupFilePath),
        fileSize: record.fileSize,
        reason: ScriptMetaBackupReason(rawValue: string(record.reason)) ?? .unknown
    )
}

private nonisolated func scriptMetaBackupGeneration(
    from generation: SmkScriptMetaBackupGeneration
) -> ScriptMetaBackupGeneration {
    ScriptMetaBackupGeneration(
        id: string(generation.id),
        sequenceNumber: generation.sequenceNumber,
        createdAtMillis: generation.createdAtMillis,
        filePath: string(generation.filePath),
        fileSize: generation.fileSize,
        reason: ScriptMetaBackupReason(rawValue: string(generation.reason)) ?? .unknown,
        isCurrentFile: generation.isCurrentFile != 0
    )
}

private nonisolated func fileEntries(
    from ffiEntries: UnsafeBufferPointer<SmkFileEntry>,
    firstIndex: Int,
    count: Int
) -> [FileSystemEntry] {
    guard let range = safeRange(start: firstIndex, count: count, upperBound: ffiEntries.count) else {
        return []
    }
    return range.map { index in
        let entry = ffiEntries[index]
        return FileSystemEntry(
            displayPath: string(entry.displayPath),
            resolvedPath: string(entry.resolvedPath),
            pathKind: string(entry.pathKind),
            resolutionStatus: string(entry.resolutionStatus),
            resolutionMessage: optionalString(entry.resolutionMessage),
            isDirectory: entry.isDirectory != 0,
            fileSize: entry.hasFileSize != 0 ? entry.fileSize : nil,
            contentModifiedAt: entry.hasContentModifiedAt != 0 ? entry.contentModifiedAt : nil,
            identity: entry.hasIdentity != 0 ? fileIdentity(from: entry.identity) : nil,
            runtimeKind: optionalString(entry.runtimeKind),
            shebang: optionalString(entry.shebang),
            hasScriptMeta: entry.hasScriptMeta != 0,
            hasScriptMetaEditPassword: entry.hasScriptMetaEditPassword != 0,
            isFileLocked: entry.isFileLocked != 0,
            isReadOnly: entry.isReadOnly != 0,
            canEditScriptMeta: entry.canEditScriptMeta != 0,
            canAppendScriptMeta: entry.canAppendScriptMeta != 0,
            scriptMetaEditState: string(entry.scriptMetaEditState),
            scriptMetaItem: entry.hasScriptMetaItem != 0 ? scriptItem(from: entry.scriptMetaItem) : nil,
            children: fileEntries(
                from: ffiEntries,
                firstIndex: entry.firstChildIndex,
                count: entry.childCount
            )
        )
    }
}

private nonisolated func safeRange(start: Int, count: Int, upperBound: Int) -> Range<Int>? {
    guard start >= 0, count >= 0, start <= upperBound else { return nil }
    let end = start + count
    guard end >= start, end <= upperBound else { return nil }
    return start..<end
}

private nonisolated func copyArray<T>(ptr: UnsafePointer<T>?, len: Int) -> [T] {
    guard let ptr, len > 0 else { return [] }
    return Array(UnsafeBufferPointer(start: ptr, count: len))
}

private nonisolated func buffer<T>(ptr: UnsafePointer<T>?, len: Int) -> UnsafeBufferPointer<T> {
    guard let ptr, len > 0 else {
        return UnsafeBufferPointer(start: nil, count: 0)
    }
    return UnsafeBufferPointer(start: ptr, count: len)
}

private nonisolated func array(from slice: SmkRootSnapshotSlice) -> [SmkRootSnapshot] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func buffer(from slice: SmkRootSnapshotSlice) -> UnsafeBufferPointer<SmkRootSnapshot> {
    buffer(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkRegisteredRootSignatureSlice) -> [SmkRegisteredRootSignature] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkFileListSnapshotSlice) -> [SmkFileListSnapshot] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func buffer(from slice: SmkFileListSnapshotSlice) -> UnsafeBufferPointer<SmkFileListSnapshot> {
    buffer(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkFileEntrySlice) -> [SmkFileEntry] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func buffer(from slice: SmkFileEntrySlice) -> UnsafeBufferPointer<SmkFileEntry> {
    buffer(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkScriptItemSlice) -> [SmkScriptItem] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func buffer(from slice: SmkScriptItemSlice) -> UnsafeBufferPointer<SmkScriptItem> {
    buffer(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkScriptIdDuplicateSlice) -> [SmkScriptIdDuplicate] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func buffer(from slice: SmkCandidateRecordSlice) -> UnsafeBufferPointer<SmkCandidateRecord> {
    buffer(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkUpdateStatusEntrySlice) -> [SmkUpdateStatusEntry] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkDistributionResolutionEntrySlice) -> [SmkDistributionResolutionEntry] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkUpdateFailureEntrySlice) -> [SmkUpdateFailureEntry] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkUpdateErrorEntrySlice) -> [SmkUpdateErrorEntry] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkUtf8SliceSlice) -> [SmkUtf8Slice] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkFileEntryChangeSlice) -> [SmkFileEntryChange] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkFileIssueSlice) -> [SmkFileIssue] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkWatchPathEventSlice) -> [SmkWatchPathEvent] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkIgnoredWatchPathSlice) -> [SmkIgnoredWatchPath] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkWatchRenameCandidateSlice) -> [SmkWatchRenameCandidate] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkWatchRescanTargetSlice) -> [SmkWatchRescanTarget] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func array(from slice: SmkScriptMetaBackupGenerationSlice) -> [SmkScriptMetaBackupGeneration] {
    copyArray(ptr: slice.ptr, len: slice.len)
}

private nonisolated func updateProgress(from progress: SmkUpdateProgress) -> UpdateCheckProgress {
    UpdateCheckProgress(
        completedItems: progress.completedItems,
        totalItems: progress.totalItems,
        itemID: optionalString(progress.itemID),
        scriptID: optionalString(progress.scriptID),
        phase: string(progress.phase),
        message: string(progress.message)
    )
}

private extension ScriptMetaWriteMode {
    nonisolated var ffiValue: UInt32 {
        switch self {
        case .insertOrReplace:
            0
        case .insertOnly:
            1
        case .replaceOnly:
            2
        }
    }
}

private extension ScriptMetaBackupReason {
    nonisolated var ffiValue: UInt32? {
        switch self {
        case .beforeSave:
            0
        case .beforeRestore:
            1
        case .resetInitial:
            2
        case .unknown:
            nil
        }
    }
}

private nonisolated func check(_ status: Int32) throws {
    guard status == smkStatusOK else {
        throw ScriptMetaKitError.operationFailed(status, "SCRIPTMETAKit FFI status=\(status)")
    }
}

private nonisolated func optionalString(_ slice: SmkUtf8Slice) -> String? {
    let value = string(slice)
    return value.isEmpty ? nil : value
}

private nonisolated func string(_ slice: SmkUtf8Slice) -> String {
    guard let ptr = slice.ptr, slice.len > 0 else { return "" }
    let buffer = UnsafeBufferPointer(start: ptr, count: slice.len)
    return String(decoding: buffer, as: UTF8.self)
}
