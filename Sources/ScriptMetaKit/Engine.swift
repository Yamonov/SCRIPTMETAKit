import Foundation

public nonisolated enum ScriptMetaKitError: LocalizedError {
    case engineCreationFailed(Int32)
    case operationFailed(Int32, String)
    case sourceConflict(String)

    public var errorDescription: String? {
        switch self {
        case .engineCreationFailed(let status):
            "SCRIPTMETAKit engineを作成できませんでした。status=\(status)"
        case .operationFailed(_, let message):
            message.isEmpty ? "SCRIPTMETAKit FFI呼び出しに失敗しました。" : message
        case .sourceConflict(let message):
            message.isEmpty ? "The script changed after it was loaded." : message
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
    public static let maximumCacheFileBytes: UInt64 = 64 * 1024 * 1024

    public init() {}

    public func configuration() async throws -> ScriptMetaKitConfiguration {
        try await runOperation { try $0.configuration() }
    }

    func cacheRevisions(rootIDs: [String], mode: ScriptMetaScanMode) async throws -> [ScriptMetaCacheRevision] {
        try await runOperation { try $0.cacheRevisions(rootIDs: rootIDs, mode: mode) }
    }

    func loadCacheRevisions(from fileURL: URL, maximumBytes: UInt64, rootIDs: [String], mode: ScriptMetaScanMode) async throws -> [ScriptMetaCacheRevision] {
        try await runOperation {
            try $0.loadCacheRevisions(from: fileURL, maximumBytes: maximumBytes, rootIDs: rootIDs, mode: mode)
        }
    }

    /// Validates and applies settings together. Invalid input leaves the engine unchanged.
    /// When refreshRequired is true, refresh or scan roots to apply the new read settings.
    @discardableResult
    public func applyConfiguration(_ configuration: ScriptMetaKitConfiguration) async throws -> ScriptMetaKitConfigurationChange {
        try await runOperation { try $0.applyConfiguration(configuration) }
    }

    /// Invalidates engine caches and marks roots for a full refresh. Does not delete files.
    public func invalidateCache(scope: ScriptMetaCacheScope = .all) async throws {
        try await runOperation { try $0.invalidateCache(scope: scope) }
    }

    /// Records externally observed changes. Call refreshDirtyRoots to process them.
    public func notifyChangedPaths(_ fileURLs: [URL], overflowed: Bool = false) async throws {
        try await runOperation { try $0.notifyChangedPaths(fileURLs, overflowed: overflowed) }
    }

    public func refreshDirtyRoots(mode: ScriptMetaScanMode = .fileListAndMetadata) async throws -> ScriptMetaScanResult {
        try await runOperation { try $0.refreshDirtyRoots(mode: mode) }
    }

    /// Pauses event-driven reads for registered root IDs; explicit scans remain available.
    /// Repeated calls are idempotent. Empty input and unknown IDs throw without changes.
    public func pauseWatching(rootIDs: [String]) async throws {
        try await runOperation { try $0.pauseWatching(rootIDs: rootIDs) }
    }

    /// Fully rereads the requested roots before resuming their watch policies.
    /// File metadata is read even when size and modification time are unchanged.
    /// Incomplete results leave the entire batch paused. No network checks occur.
    public func resumeWatching(rootIDs: [String]) async throws -> ScriptMetaScanResult {
        try await runOperation { try $0.resumeWatching(rootIDs: rootIDs) }
    }

    /// Unknown root IDs return false.
    public func isWatchingPaused(rootID: String) async throws -> Bool {
        try await runOperation { try $0.isWatchingPaused(rootID: rootID) }
    }

    func pausedWatchRoots(rootIDs: [String]) async throws -> Set<String> {
        try await runOperation { try $0.pausedWatchRoots(rootIDs: rootIDs) }
    }

    private func runOperation<Result: Sendable>(
        _ operation: @escaping @Sendable (ScriptMetaKitFFIEngineBox) throws -> Result
    ) async throws -> Result {
        // Once a worker is created, join it even if cancellation arrives during
        // hand-off. Returning early could release Workspace serialization while
        // the native operation was still changing its state.
        try Task.checkCancellation()
        let ticket = ScriptMetaKitOperationTicket()
        engineBox.registerOperationTicket(ticket)
        let task = Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.runOperation(ticket: ticket) {
                try operation(engineBox)
            }
        }
        return try await withTaskCancellationHandler {
            return try await task.value
        } onCancel: { [engineBox] in
            let shouldCancelActiveOperation = ticket.requestCancellation()
            engineBox.notifyOperationWaiters()
            if shouldCancelActiveOperation {
                engineBox.cancelCurrentOrReservedOperation()
            }
        }
    }

    private func runTerminationOperation(
        _ operation: @escaping @Sendable (ScriptMetaKitFFIEngineBox) -> Void
    ) async {
        let ticket = ScriptMetaKitOperationTicket()
        engineBox.registerOperationTicket(ticket)
        let task = Task.detached(priority: Self.operationPriority) { [engineBox] in
            try engineBox.runOperation(ticket: ticket) {
                operation(engineBox)
            }
        }
        _ = try? await task.value
    }

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
        try await runOperation { engineBox in
            try engineBox.scan(folderURLs: folderURLs, checkUpdates: checkUpdates, onProgress: onProgress)
        }
    }

    public func checkUpdate(
        item: ScriptMetaItem,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> UpdateCheckResult {
        try await runOperation { engineBox in
            try engineBox.checkUpdate(item: item, onProgress: onProgress)
        }
    }

    public func checkUpdates(
        items: [ScriptMetaItem],
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> UpdateCheckResult {
        try await runOperation { engineBox in
            try engineBox.checkUpdates(items: items, onProgress: onProgress)
        }
    }

    public func setRoots(_ roots: [ScriptMetaKitRoot]) async throws {
        try await runOperation { engineBox in
            try engineBox.setRoots(roots)
        }
    }

    /// Select complete custom keys, or ["X-"] for all. Empty input retains none.
    @discardableResult
    public func setCustomMetadataKeys(_ keys: [String]) async throws -> Bool {
        try await runOperation { engineBox in
            try engineBox.setCustomMetadataKeys(keys)
        }
    }

    /// Explicit consumer priorities, for example ["ja", "en", "fr"].
    /// Empty input uses metadata defaults. No OS language settings are read.
    @discardableResult
    public func setPreferredLanguages(_ languages: [String]) async throws -> Bool {
        try await runOperation { engineBox in
            try engineBox.setPreferredLanguages(languages)
        }
    }

    public func replaceRootGroup(_ roots: [ScriptMetaKitRoot], groupID: String) async throws {
        try await runOperation { engineBox in
            try engineBox.replaceRootGroup(roots, groupID: groupID)
        }
    }

    public func insertRootsIntoGroup(_ roots: [ScriptMetaKitRoot], groupID: String) async throws {
        try await runOperation { engineBox in
            try engineBox.insertRootsIntoGroup(roots, groupID: groupID)
        }
    }

    public func setVisibleRoot(_ rootID: String?) async throws {
        try await runOperation { engineBox in
            try engineBox.setVisibleRoot(rootID)
        }
    }

    public func clearVisibleRoot() async throws {
        try await setVisibleRoot(nil)
    }

    public func preflightRoot(_ root: ScriptMetaKitRoot) async throws -> ScriptMetaScanResult {
        try await runOperation { engineBox in
            try engineBox.preflightRoot(root)
        }
    }

    func failNextWatcherStartForTesting() async throws {
        try await runOperation { engineBox in
            try engineBox.failNextWatcherStartForTesting()
        }
    }

    func cancelNextCacheLoadForTesting() async throws {
        try await runOperation { engineBox in
            try engineBox.cancelNextCacheLoadForTesting()
        }
    }

    func cancelNextWatchResumeForTesting() async throws {
        try await runOperation { $0.cancelNextWatchResumeForTesting() }
    }

    public func cancelCurrentOperation() {
        engineBox.cancelCurrentOperation()
    }

    func cancelCurrentOrPendingOperationForTermination() {
        engineBox.cancelCurrentOrPendingOperation()
    }

    public func shutdown(cancelCurrentOperation: Bool = false) async {
        if cancelCurrentOperation {
            engineBox.cancelCurrentOrPendingOperation()
        }
        await runTerminationOperation { engineBox in
            engineBox.shutdown()
        }
    }

    public func scanRegisteredRoots(
        mode: ScriptMetaScanMode = .fileListAndMetadata,
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> ScriptMetaScanResult {
        try await runOperation { engineBox in
            try engineBox.scanRegisteredRoots(mode: mode, checkUpdates: checkUpdates, onProgress: onProgress)
        }
    }

    public func scanRoots(
        rootIDs: [String],
        mode: ScriptMetaScanMode = .fileListAndMetadata,
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> ScriptMetaScanResult {
        try await runOperation { engineBox in
            try engineBox.scanRoots(rootIDs: rootIDs, mode: mode, checkUpdates: checkUpdates, onProgress: onProgress)
        }
    }

    public func cachedRoots(
        rootIDs: [String],
        mode: ScriptMetaScanMode = .fileListAndMetadata
    ) async throws -> ScriptMetaScanResult {
        try await runOperation { engineBox in
            try engineBox.cachedRoots(rootIDs: rootIDs, mode: mode)
        }
    }

    public func scanRoot(
        rootID: String,
        mode: ScriptMetaScanMode = .fileListAndMetadata,
        checkUpdates: Bool,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> ScriptMetaScanResult {
        try await scanRoots(rootIDs: [rootID], mode: mode, checkUpdates: checkUpdates, onProgress: onProgress)
    }

    public func startWatching(
        onChange: @escaping @Sendable () -> Void,
        reconciliationDelivery: ScriptMetaKitReconciliationDeliveryPolicy = .complete
    ) async throws {
        try await runOperation { engineBox in
            try engineBox.startWatching(
                onChange: onChange,
                reconciliationDelivery: reconciliationDelivery
            )
        }
    }

    public func startWatching(folderURLs: [URL], onChange: @escaping @Sendable () -> Void) async throws {
        try await runOperation { engineBox in
            try engineBox.startWatching(folderURLs: folderURLs, onChange: onChange)
        }
    }

    public func stopWatching(cancelCurrentOperation: Bool = false) async {
        if cancelCurrentOperation {
            engineBox.cancelCurrentOrPendingOperation()
        }
        await runTerminationOperation { engineBox in
            engineBox.stopWatching()
        }
    }

    public func setResolveMacOSAlias(_ enabled: Bool) async throws {
        try await runOperation { engineBox in
            try engineBox.setResolveMacOSAlias(enabled)
        }
    }

    public func setDecompileCompiledOSADuringScan(_ enabled: Bool) async throws {
        try await runOperation { engineBox in
            try engineBox.setDecompileCompiledOSADuringScan(enabled)
        }
    }

    public func setNativeEventLatencyMillis(_ latencyMillis: UInt64) async throws {
        try await runOperation { engineBox in
            try engineBox.setNativeEventLatencyMillis(latencyMillis)
        }
    }

    public func setOperationalPolicy(_ policy: ScriptMetaKitOperationalPolicy) async throws {
        try await runOperation { engineBox in
            try engineBox.setOperationalPolicy(policy)
        }
    }

    public func setRootPreflightOptions(_ options: ScriptMetaRootPreflightOptions) async throws {
        try await runOperation { engineBox in
            try engineBox.setRootPreflightOptions(options)
        }
    }

    public func loadCache(
        from fileURL: URL,
        maximumBytes: UInt64 = ScriptMetaKitEngine.maximumCacheFileBytes
    ) async throws {
        try await runOperation { engineBox in
            try engineBox.loadCache(from: fileURL, maximumBytes: maximumBytes)
        }
    }

    public func saveCache(
        to fileURL: URL,
        scope: ScriptMetaCacheScope = .all,
        maximumBytes: UInt64 = ScriptMetaKitEngine.maximumCacheFileBytes
    ) async throws {
        try await runOperation { engineBox in
            try engineBox.saveCache(to: fileURL, scope: scope, maximumBytes: maximumBytes)
        }
    }

    public func writeScriptMetadata(
        fileURL: URL,
        draft: ScriptMetadataDraft,
        mode: ScriptMetaWriteMode = .insertOrReplace,
        backupRootURL: URL? = nil,
        expectedSourceFingerprint: String? = nil
    ) async throws -> ScriptMetadataFileWriteResult {
        try await runOperation { engineBox in
            try engineBox.writeScriptMetadata(
                fileURL: fileURL,
                draft: draft,
                mode: mode,
                backupRootURL: backupRootURL,
                expectedSourceFingerprint: expectedSourceFingerprint
            )
        }
    }

    public func readScriptMetadataDraft(fileURL: URL) async throws -> ScriptMetadataEditReadResult {
        try await runOperation { engineBox in
            try engineBox.readScriptMetadataDraft(fileURL: fileURL)
        }
    }

    public func readScriptMetadataEditPreview(
        fileURL: URL,
        maxBytes: Int = 8 * 1024
    ) async throws -> ScriptMetadataEditPreviewResult {
        try await runOperation { engineBox in
            try engineBox.readScriptMetadataEditPreview(fileURL: fileURL, maxBytes: maxBytes)
        }
    }

    public func renderDistributionMetadata(records: [DistributionMetadataDraft]) async throws -> String {
        try await runOperation { engineBox in
            try engineBox.renderDistributionMetadata(records: records)
        }
    }

    public func generateEditPasswordSHA256(password: String) async throws -> String {
        try await runOperation { engineBox in
            try engineBox.generateEditPasswordSHA256(password: password)
        }
    }

    public func verifyEditPasswordSHA256(password: String, storedValue: String) async throws -> Bool {
        try await runOperation { engineBox in
            try engineBox.verifyEditPasswordSHA256(password: password, storedValue: storedValue)
        }
    }

    public func scriptMetaBackupGenerations(
        fileURL: URL,
        backupRootURL: URL
    ) async throws -> [ScriptMetaBackupGeneration] {
        try await runOperation { engineBox in
            try engineBox.scriptMetaBackupGenerations(fileURL: fileURL, backupRootURL: backupRootURL)
        }
    }

    public func createScriptMetaBackup(
        fileURL: URL,
        backupRootURL: URL,
        reason: ScriptMetaBackupReason = .beforeSave
    ) async throws -> ScriptMetaBackupRecord {
        try await runOperation { engineBox in
            try engineBox.createScriptMetaBackup(fileURL: fileURL, backupRootURL: backupRootURL, reason: reason)
        }
    }

    public func restoreScriptMetaBackup(
        fileURL: URL,
        backupRootURL: URL,
        generationID: String
    ) async throws -> ScriptMetaBackupRecord {
        try await runOperation { engineBox in
            try engineBox.restoreScriptMetaBackup(
                fileURL: fileURL,
                backupRootURL: backupRootURL,
                generationID: generationID
            )
        }
    }

    public func clearScriptMetaBackups(fileURL: URL, backupRootURL: URL) async throws {
        try await runOperation { engineBox in
            try engineBox.clearScriptMetaBackups(fileURL: fileURL, backupRootURL: backupRootURL)
        }
    }

    public func resetScriptMetaBackupsWithCurrentAsInitial(
        fileURL: URL,
        backupRootURL: URL
    ) async throws -> ScriptMetaBackupRecord {
        try await runOperation { engineBox in
            try engineBox.resetScriptMetaBackupsWithCurrentAsInitial(
                fileURL: fileURL,
                backupRootURL: backupRootURL
            )
        }
    }

    public func validateScriptIDUniqueness(in items: [ScriptMetaItem]) throws -> ScriptIdUniquenessReport {
        try Self.validateScriptIDUniqueness(in: items)
    }

    public func pollWatchChanges(dirtyOnly: Bool = false) async throws -> ScriptMetaScanResult? {
        try await runOperation { engineBox in
            try engineBox.pollWatchChanges(dirtyOnly: dirtyOnly)
        }
    }
}

private nonisolated let smkStatusOK: Int32 = 0
private nonisolated let smkStatusInvalidArgument: Int32 = 3
private nonisolated let smkStatusEngineError: Int32 = 4
private nonisolated let smkStatusConflict: Int32 = 6

nonisolated struct SmkUtf8Slice {
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

private nonisolated struct SmkOperationalPolicy {
    var maxConcurrentMetaURLChecks: Int
    var retryAttempts: Int
    var retryInitialDelayMillis: UInt64
    var retryBackoffMultiplier: UInt32
    var maxRetryDelayMillis: UInt64
    var requestTimeoutMillis: UInt64
    var resourceTimeoutMillis: UInt64
    var watcherDebounceDelayMillis: UInt64
    var watcherMaxDeliveryDelayMillis: UInt64
    var watcherMaxPendingPaths: Int
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

private nonisolated struct SmkRootSnapshotRevision {
    var rootIndex: Int
    var workspaceEpoch: SmkUtf8Slice
    var sequence: UInt64
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

private nonisolated struct SmkFileListDirectoryStateRange {
    var firstDirectoryStateIndex: Int
    var directoryStateCount: Int
}

private nonisolated struct SmkDirectoryStateEntry {
    var path: SmkUtf8Slice
    var hasModificationTimeMillis: UInt8
    var modificationTimeMillis: UInt64
    var childCount: Int
    var childFingerprint: UInt64
    var hasIdentity: UInt8
    var identity: SmkFileIdentity
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

private nonisolated struct SmkWatchDeliveryInfo {
    var isWatchUpdate: UInt8 = 0
    var isReconciliation: UInt8 = 0
    var coversAllWatchedRoots: UInt8 = 0
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

private nonisolated struct SmkRootSnapshotRevisionSlice {
    var ptr: UnsafePointer<SmkRootSnapshotRevision>?
    var len: Int

    init(ptr: UnsafePointer<SmkRootSnapshotRevision>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkFileListSnapshotDetails {
    var fileListIndex: Int
    var hasChildren: UInt8
    var workspaceEpoch: SmkUtf8Slice
    var contentSequence: UInt64
}

private nonisolated struct SmkFileListSnapshotDetailsSlice {
    var ptr: UnsafePointer<SmkFileListSnapshotDetails>?
    var len: Int

    init(ptr: UnsafePointer<SmkFileListSnapshotDetails>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkFileListDirectoryStateRangeSlice {
    var ptr: UnsafePointer<SmkFileListDirectoryStateRange>?
    var len: Int

    init(ptr: UnsafePointer<SmkFileListDirectoryStateRange>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkDirectoryStateEntrySlice {
    var ptr: UnsafePointer<SmkDirectoryStateEntry>?
    var len: Int

    init(ptr: UnsafePointer<SmkDirectoryStateEntry>? = nil, len: Int = 0) {
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

private nonisolated struct SmkMetadataPresentation {
    var defaultLanguage = SmkUtf8Slice()
    var nameLanguage = SmkUtf8Slice()
    var authorLanguage = SmkUtf8Slice()
    var descriptionLanguage = SmkUtf8Slice()
    var firstTagIndex = 0
    var tagCount = 0
    var firstDiagnosticIndex = 0
    var diagnosticCount = 0
}
private nonisolated struct SmkMetadataPresentationSlice {
    var ptr: UnsafePointer<SmkMetadataPresentation>?
    var len = 0
}
private nonisolated struct SmkCustomMetadataValue {
    var key = SmkUtf8Slice()
    var value = SmkUtf8Slice()
}
private nonisolated struct SmkCustomMetadataValueSlice {
    var ptr: UnsafePointer<SmkCustomMetadataValue>?
    var len = 0
}
private nonisolated struct SmkCustomMetadataRange {
    var firstValueIndex = 0
    var valueCount = 0
}
private nonisolated struct SmkCustomMetadataRangeSlice {
    var ptr: UnsafePointer<SmkCustomMetadataRange>?
    var len = 0
}
@_silgen_name("smk_scan_result_custom_metadata_values")
private nonisolated func smk_scan_result_custom_metadata_values(_ result: OpaquePointer?, _ output: UnsafeMutablePointer<SmkCustomMetadataValueSlice>) -> Int32
@_silgen_name("smk_scan_result_custom_metadata_ranges")
private nonisolated func smk_scan_result_custom_metadata_ranges(_ result: OpaquePointer?, _ collection: UInt32, _ output: UnsafeMutablePointer<SmkCustomMetadataRangeSlice>) -> Int32
@_silgen_name("smk_engine_set_custom_metadata_keys")
private nonisolated func smk_engine_set_custom_metadata_keys(_ engine: OpaquePointer?, _ keys: UnsafePointer<SmkUtf8Slice>?, _ count: Int, _ changed: UnsafeMutablePointer<UInt8>) -> Int32
@_silgen_name("smk_edit_result_custom_metadata_fields")
private nonisolated func smk_edit_result_custom_metadata_fields(_ result: OpaquePointer?, _ output: UnsafeMutablePointer<SmkUtf8Slice>) -> Int32
@_silgen_name("smk_engine_write_script_metadata_file_if_unchanged_v3")
private nonisolated func smk_engine_write_script_metadata_file_if_unchanged_v3(_ engine: OpaquePointer?, _ request: UnsafePointer<SmkScriptMetadataWriteRequest>, _ extensions: UnsafePointer<SmkMetadataEditExtensions>?, _ customFields: SmkUtf8Slice, _ fingerprint: SmkUtf8Slice, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
private nonisolated struct SmkMetadataDiagnostic {
    var code = SmkUtf8Slice()
    var field = SmkUtf8Slice()
    var language = SmkUtf8Slice()
    var line = 0
    var hasLine: UInt8 = 0
}
private nonisolated struct SmkMetadataDiagnosticSlice {
    var ptr: UnsafePointer<SmkMetadataDiagnostic>?
    var len = 0
}
private nonisolated struct SmkLocalizedText {
    var language = SmkUtf8Slice()
    var value = SmkUtf8Slice()
}
private nonisolated struct SmkLocalizedTextSlice {
    var ptr: UnsafePointer<SmkLocalizedText>?
    var len = 0
}
private nonisolated struct SmkMetadataEditExtensions {
    var hasExtensions: UInt8 = 0
    var defaultLanguage = SmkUtf8Slice()
    var names = SmkLocalizedTextSlice()
    var authors = SmkLocalizedTextSlice()
    var tags = SmkUtf8SliceSlice()
}

@_silgen_name("smk_engine_set_preferred_languages")
private nonisolated func smk_engine_set_preferred_languages(_ engine: OpaquePointer?, _ languages: UnsafePointer<SmkUtf8Slice>?, _ count: Int, _ changed: UnsafeMutablePointer<UInt8>?) -> Int32
@_silgen_name("smk_scan_result_preferred_languages")
private nonisolated func smk_scan_result_preferred_languages(_ result: OpaquePointer?, _ values: UnsafeMutablePointer<SmkUtf8SliceSlice>?) -> Int32
@_silgen_name("smk_scan_result_metadata_presentations")
private nonisolated func smk_scan_result_metadata_presentations(_ result: OpaquePointer?, _ collection: UInt32, _ values: UnsafeMutablePointer<SmkMetadataPresentationSlice>?) -> Int32
@_silgen_name("smk_scan_result_metadata_tags")
private nonisolated func smk_scan_result_metadata_tags(_ result: OpaquePointer?, _ values: UnsafeMutablePointer<SmkUtf8SliceSlice>?) -> Int32
@_silgen_name("smk_scan_result_metadata_diagnostics")
private nonisolated func smk_scan_result_metadata_diagnostics(_ result: OpaquePointer?, _ values: UnsafeMutablePointer<SmkMetadataDiagnosticSlice>?) -> Int32
@_silgen_name("smk_edit_result_metadata_extensions")
private nonisolated func smk_edit_result_metadata_extensions(_ result: OpaquePointer?, _ extensions: UnsafeMutablePointer<SmkMetadataEditExtensions>?) -> Int32
@_silgen_name("smk_edit_result_metadata_diagnostics")
private nonisolated func smk_edit_result_metadata_diagnostics(_ result: OpaquePointer?, _ values: UnsafeMutablePointer<SmkMetadataDiagnosticSlice>?) -> Int32
@_silgen_name("smk_engine_write_script_metadata_file_if_unchanged_v2")
private nonisolated func smk_engine_write_script_metadata_file_if_unchanged_v2(_ engine: OpaquePointer?, _ request: UnsafePointer<SmkScriptMetadataWriteRequest>?, _ extensions: UnsafePointer<SmkMetadataEditExtensions>?, _ fingerprint: SmkUtf8Slice, _ result: UnsafeMutablePointer<OpaquePointer?>?) -> Int32

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

@_silgen_name("smk_engine_set_operational_policy")
private nonisolated func smk_engine_set_operational_policy(
    _ engine: OpaquePointer?,
    _ policy: UnsafePointer<SmkOperationalPolicy>?
) -> Int32

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

@_silgen_name("smk_engine_reserve_next_operation")
private nonisolated func smk_engine_reserve_next_operation(_ engine: OpaquePointer?) -> Int32

@_silgen_name("smk_engine_finish_operation_reservation")
private nonisolated func smk_engine_finish_operation_reservation(_ engine: OpaquePointer?) -> Int32

@_silgen_name("smk_engine_cancel_current_or_reserved_operation")
private nonisolated func smk_engine_cancel_current_or_reserved_operation(_ engine: OpaquePointer?) -> Int32

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

@_silgen_name("smk_engine_preflight_root")
private nonisolated func smk_engine_preflight_root(
    _ engine: OpaquePointer?,
    _ root: UnsafePointer<SmkRootRegistration>?,
    _ outResult: UnsafeMutablePointer<OpaquePointer?>
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

@_silgen_name("smk_engine_load_cache_file_with_limit")
private nonisolated func smk_engine_load_cache_file_with_limit(
    _ engine: OpaquePointer?,
    _ cachePath: SmkUtf8Slice,
    _ maximumBytes: UInt64
) -> Int32

@_silgen_name("smk_engine_save_cache_file_with_limit")
private nonisolated func smk_engine_save_cache_file_with_limit(
    _ engine: OpaquePointer?,
    _ scope: UInt32,
    _ cachePath: SmkUtf8Slice,
    _ maximumBytes: UInt64
) -> Int32

@_silgen_name("smk_engine_start_watching_with_callback_v2")
private nonisolated func smk_engine_start_watching_with_callback_v2(
    _ engine: OpaquePointer?,
    _ callback: SmkWatchNotificationCallback?,
    _ context: UnsafeMutableRawPointer?,
    _ reconciliationDelivery: UInt32
) -> Int32

@_silgen_name("smk_engine_stop_watching")
private nonisolated func smk_engine_stop_watching(_ engine: OpaquePointer?) -> Int32

@_silgen_name("smk_engine_watcher_requires_restart")
private nonisolated func smk_engine_watcher_requires_restart(
    _ engine: OpaquePointer?,
    _ outRequiresRestart: UnsafeMutablePointer<UInt8>
) -> Int32

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

@_silgen_name("smk_scan_result_root_revisions")
private nonisolated func smk_scan_result_root_revisions(_ result: OpaquePointer?, _ outRevisions: UnsafeMutablePointer<SmkRootSnapshotRevisionSlice>) -> Int32

@_silgen_name("smk_scan_result_catalog_info")
private nonisolated func smk_scan_result_catalog_info(_ result: OpaquePointer?, _ outInfo: UnsafeMutablePointer<SmkCatalogInfo>) -> Int32

@_silgen_name("smk_scan_result_registered_root_signatures")
private nonisolated func smk_scan_result_registered_root_signatures(
    _ result: OpaquePointer?,
    _ outRoots: UnsafeMutablePointer<SmkRegisteredRootSignatureSlice>
) -> Int32

@_silgen_name("smk_scan_result_file_lists")
private nonisolated func smk_scan_result_file_lists(_ result: OpaquePointer?, _ outFileLists: UnsafeMutablePointer<SmkFileListSnapshotSlice>) -> Int32

@_silgen_name("smk_scan_result_file_list_details")
private nonisolated func smk_scan_result_file_list_details(_ result: OpaquePointer?, _ outDetails: UnsafeMutablePointer<SmkFileListSnapshotDetailsSlice>) -> Int32

@_silgen_name("smk_scan_result_file_entries")
private nonisolated func smk_scan_result_file_entries(_ result: OpaquePointer?, _ outFileEntries: UnsafeMutablePointer<SmkFileEntrySlice>) -> Int32

@_silgen_name("smk_scan_result_file_list_directory_state_ranges")
private nonisolated func smk_scan_result_file_list_directory_state_ranges(
    _ result: OpaquePointer?,
    _ outRanges: UnsafeMutablePointer<SmkFileListDirectoryStateRangeSlice>
) -> Int32

@_silgen_name("smk_scan_result_directory_states")
private nonisolated func smk_scan_result_directory_states(
    _ result: OpaquePointer?,
    _ outDirectoryStates: UnsafeMutablePointer<SmkDirectoryStateEntrySlice>
) -> Int32

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

@_silgen_name("smk_scan_result_watch_delivery_info")
private nonisolated func smk_scan_result_watch_delivery_info(_ result: OpaquePointer?, _ outInfo: UnsafeMutablePointer<SmkWatchDeliveryInfo>) -> Int32

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
nonisolated func smk_edit_result_text(
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
nonisolated func smk_edit_result_free(_ result: OpaquePointer?)

private nonisolated final class ScriptMetaKitOperationTicket: @unchecked Sendable {
    private enum State {
        case waiting
        case cancellationRequested
        case active
        case finished
    }

    private let lock = NSLock()
    private var state = State.waiting

    var isCancellationRequested: Bool {
        lock.lock()
        defer { lock.unlock() }
        return state == .cancellationRequested
    }

    func requestCancellation() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        switch state {
        case .waiting:
            state = .cancellationRequested
            return false
        case .active:
            return true
        case .cancellationRequested, .finished:
            return false
        }
    }

    func activate() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        switch state {
        case .waiting:
            state = .active
            return true
        case .cancellationRequested:
            state = .finished
            return false
        case .active, .finished:
            return false
        }
    }

    func finish() {
        lock.lock()
        state = .finished
        lock.unlock()
    }
}

private nonisolated final class ScriptMetaKitFFIEngineBox: @unchecked Sendable {
    private let operationCondition = NSCondition()
    private var operationIsActive = false
    private var operationTickets: [ScriptMetaKitOperationTicket] = []
    private var activeOperationTicket: ScriptMetaKitOperationTicket?
    private let lock = NSLock()
    private let cancellationLock = NSLock()
    private var engine: ScriptMetaKitFFIEngine?
    private var storedConfiguration: ScriptMetaKitConfiguration?
    private var cancellationEngine: ScriptMetaKitFFIEngine?
    private var watchNotificationSink: ScriptMetaKitWatchNotificationSink?
    private var shouldFailNextWatcherStartForTesting = false
    private var shouldCancelNextCacheLoadForTesting = false
    private var shouldCancelNextWatchResumeForTesting = false

    deinit {
        shutdown()
    }

    func runOperation<Result>(
        ticket: ScriptMetaKitOperationTicket,
        _ operation: () throws -> Result
    ) throws -> Result {
        defer { unregisterOperationTicket(ticket) }
        operationCondition.lock()
        while operationIsActive {
            if ticket.isCancellationRequested {
                operationCondition.unlock()
                throw CancellationError()
            }
            operationCondition.wait()
        }
        if ticket.isCancellationRequested {
            operationCondition.unlock()
            throw CancellationError()
        }
        operationIsActive = true
        activeOperationTicket = ticket
        operationCondition.unlock()
        defer {
            operationCondition.lock()
            operationIsActive = false
            activeOperationTicket = nil
            operationCondition.broadcast()
            operationCondition.unlock()
        }

        let reservationEngine = try prepareOperationCancellation()
        defer { reservationEngine.finishOperationReservation() }
        guard ticket.activate() else {
            throw CancellationError()
        }
        defer { ticket.finish() }
        return try operation()
    }

    func registerOperationTicket(_ ticket: ScriptMetaKitOperationTicket) {
        operationCondition.lock()
        operationTickets.append(ticket)
        operationCondition.unlock()
    }

    private func unregisterOperationTicket(_ ticket: ScriptMetaKitOperationTicket) {
        operationCondition.lock()
        operationTickets.removeAll { $0 === ticket }
        operationCondition.unlock()
    }

    func cancelCurrentOrPendingOperation() {
        operationCondition.lock()
        let ticket = activeOperationTicket ?? operationTickets.first
        let shouldCancelNativeOperation = ticket?.requestCancellation() ?? false
        operationCondition.broadcast()
        operationCondition.unlock()
        if shouldCancelNativeOperation {
            cancelCurrentOrReservedOperation()
        }
    }

    func notifyOperationWaiters() {
        operationCondition.lock()
        operationCondition.broadcast()
        operationCondition.unlock()
    }

    private func prepareOperationCancellation() throws -> ScriptMetaKitFFIEngine {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        try engine.reserveNextOperation()
        return engine
    }

    private func ensureEngineLocked() throws -> ScriptMetaKitFFIEngine {
        if let engine {
            return engine
        }
        let engine = try ScriptMetaKitFFIEngine()
        if let storedConfiguration { _ = try engine.applyConfiguration(storedConfiguration) }
        self.engine = engine
        cancellationLock.lock()
        cancellationEngine = engine
        cancellationLock.unlock()
        return engine
    }

    public func setCustomMetadataKeys(_ keys: [String]) throws -> Bool {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        let changed = try engine.setCustomMetadataKeys(keys)
        storedConfiguration = try engine.configuration()
        return changed
    }

    public func setPreferredLanguages(_ languages: [String]) throws -> Bool {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        let changed = try engine.setPreferredLanguages(languages)
        storedConfiguration = try engine.configuration()
        return changed
    }

    func configuration() throws -> ScriptMetaKitConfiguration {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().configuration()
    }

    func cacheRevisions(rootIDs: [String], mode: ScriptMetaScanMode) throws -> [ScriptMetaCacheRevision] {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().cacheRevisions(rootIDs: rootIDs, mode: mode)
    }

    func applyConfiguration(_ configuration: ScriptMetaKitConfiguration) throws -> ScriptMetaKitConfigurationChange {
        lock.lock()
        defer { lock.unlock() }
        try failWatcherReconfigurationIfNeeded()
        let applied = try ensureEngineLocked().applyConfiguration(configuration)
        storedConfiguration = applied.configuration
        if !applied.configuration.watcher.allowsWatching { watchNotificationSink = nil }
        return applied.change
    }

    func invalidateCache(scope: ScriptMetaCacheScope) throws {
        lock.lock()
        defer { lock.unlock() }
        try ensureEngineLocked().invalidateCache(scope: scope)
    }

    func notifyChangedPaths(_ fileURLs: [URL], overflowed: Bool) throws {
        lock.lock()
        defer { lock.unlock() }
        try ensureEngineLocked().notifyChangedPaths(fileURLs, overflowed: overflowed)
    }

    func refreshDirtyRoots(mode: ScriptMetaScanMode) throws -> ScriptMetaScanResult {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().refreshDirtyRoots(mode: mode)
    }

    func pauseWatching(rootIDs: [String]) throws {
        lock.lock()
        defer { lock.unlock() }
        try failWatcherReconfigurationIfNeeded()
        try ensureEngineLocked().pauseWatching(rootIDs: rootIDs)
    }

    func resumeWatching(rootIDs: [String]) throws -> ScriptMetaScanResult {
        lock.lock()
        defer { lock.unlock() }
        try failWatcherReconfigurationIfNeeded()
        let engine = try ensureEngineLocked()
        if shouldCancelNextWatchResumeForTesting {
            shouldCancelNextWatchResumeForTesting = false
            engine.cancelCurrentOrReservedOperation()
        }
        return try engine.resumeWatching(rootIDs: rootIDs)
    }

    func cancelNextWatchResumeForTesting() {
        lock.lock()
        defer { lock.unlock() }
        shouldCancelNextWatchResumeForTesting = true
    }

    func isWatchingPaused(rootID: String) throws -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().isWatchingPaused(rootID: rootID)
    }

    func pausedWatchRoots(rootIDs: [String]) throws -> Set<String> {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        return try Set(rootIDs.filter { try engine.isWatchingPaused(rootID: $0) })
    }

    private func updateConfiguration(_ update: (ScriptMetaKitFFIEngine) throws -> Void) throws {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        try update(engine)
        storedConfiguration = try engine.configuration()
    }

    public func cancelCurrentOperation() {
        cancellationLock.lock()
        let engine = cancellationEngine
        cancellationLock.unlock()
        engine?.cancelCurrentOperation()
    }

    public func cancelCurrentOrReservedOperation() {
        cancellationLock.lock()
        let engine = cancellationEngine
        cancellationLock.unlock()
        engine?.cancelCurrentOrReservedOperation()
    }

    public func shutdown() {
        lock.lock()
        let engineToRelease = engine
        engineToRelease?.stopWatching()
        watchNotificationSink = nil
        shouldFailNextWatcherStartForTesting = false
        shouldCancelNextCacheLoadForTesting = false
        shouldCancelNextWatchResumeForTesting = false
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
        try failWatcherReconfigurationIfNeeded()
        try engine.setRoots(roots)
    }

    public func replaceRootGroup(_ roots: [ScriptMetaKitRoot], groupID: String) throws {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        try failWatcherReconfigurationIfNeeded()
        try engine.replaceRootGroup(roots, groupID: groupID)
    }

    public func insertRootsIntoGroup(_ roots: [ScriptMetaKitRoot], groupID: String) throws {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        try failWatcherReconfigurationIfNeeded()
        try engine.insertRootsIntoGroup(roots, groupID: groupID)
    }

    public func setVisibleRoot(_ rootID: String?) throws {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        try failWatcherReconfigurationIfNeeded()
        try engine.setVisibleRoot(rootID)
    }

    public func preflightRoot(_ root: ScriptMetaKitRoot) throws -> ScriptMetaScanResult {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().preflightRoot(root)
    }

    func failNextWatcherStartForTesting() throws {
        lock.lock()
        defer { lock.unlock() }
        _ = try ensureEngineLocked()
        shouldFailNextWatcherStartForTesting = true
    }

    func cancelNextCacheLoadForTesting() throws {
        lock.lock()
        defer { lock.unlock() }
        _ = try ensureEngineLocked()
        shouldCancelNextCacheLoadForTesting = true
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

    public func startWatching(
        onChange: @escaping @Sendable () -> Void,
        reconciliationDelivery: ScriptMetaKitReconciliationDeliveryPolicy = .complete
    ) throws {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        let sink = ScriptMetaKitWatchNotificationSink(onChange: onChange)
        do {
            try engine.startWatching(
                notificationSink: sink,
                reconciliationDelivery: reconciliationDelivery
            )
            watchNotificationSink = sink
        } catch {
            throw error
        }
    }

    public func startWatching(folderURLs: [URL], onChange: @escaping @Sendable () -> Void) throws {
        lock.lock()
        defer { lock.unlock() }
        let engine = try ensureEngineLocked()
        _ = try engine.scan(folderURLs: folderURLs, checkUpdates: false)
        let sink = ScriptMetaKitWatchNotificationSink(onChange: onChange)
        do {
            try engine.startWatching(notificationSink: sink)
            watchNotificationSink = sink
        } catch {
            throw error
        }
    }

    public func stopWatching() {
        lock.lock()
        defer { lock.unlock() }
        engine?.stopWatching()
        watchNotificationSink = nil
    }

    private func failWatcherReconfigurationIfNeeded() throws {
        guard watchNotificationSink != nil, shouldFailNextWatcherStartForTesting else { return }
        shouldFailNextWatcherStartForTesting = false
        throw ScriptMetaKitError.operationFailed(
            smkStatusEngineError,
            "injected watcher start failure"
        )
    }

    public func setResolveMacOSAlias(_ enabled: Bool) throws {
        try updateConfiguration { try $0.setResolveMacOSAlias(enabled) }
    }

    public func setDecompileCompiledOSADuringScan(_ enabled: Bool) throws {
        try updateConfiguration { try $0.setDecompileCompiledOSADuringScan(enabled) }
    }

    public func setNativeEventLatencyMillis(_ latencyMillis: UInt64) throws {
        try updateConfiguration {
            try failWatcherReconfigurationIfNeeded()
            try $0.setNativeEventLatencyMillis(latencyMillis)
        }
    }

    public func setOperationalPolicy(_ policy: ScriptMetaKitOperationalPolicy) throws {
        try updateConfiguration {
            try failWatcherReconfigurationIfNeeded()
            try $0.setOperationalPolicy(policy)
        }
    }

    public func setRootPreflightOptions(_ options: ScriptMetaRootPreflightOptions) throws {
        try updateConfiguration { try $0.setRootPreflightOptions(options) }
    }

    public func loadCache(from fileURL: URL, maximumBytes: UInt64) throws {
        lock.lock()
        defer { lock.unlock() }
        _ = try loadCacheLocked(from: fileURL, maximumBytes: maximumBytes)
    }

    func loadCacheRevisions(from fileURL: URL, maximumBytes: UInt64, rootIDs: [String], mode: ScriptMetaScanMode) throws -> [ScriptMetaCacheRevision] {
        lock.lock()
        defer { lock.unlock() }
        let engine = try loadCacheLocked(from: fileURL, maximumBytes: maximumBytes)
        // Complete bookkeeping within the same operation, without a cancellable
        // task hand-off after native cache adoption has already succeeded.
        return try engine.cacheRevisions(rootIDs: rootIDs, mode: mode)
    }

    private func loadCacheLocked(from fileURL: URL, maximumBytes: UInt64) throws -> ScriptMetaKitFFIEngine {
        if shouldCancelNextCacheLoadForTesting {
            shouldCancelNextCacheLoadForTesting = false
            throw CancellationError()
        }
        let engine = try ensureEngineLocked()
        try engine.loadCache(from: fileURL, maximumBytes: maximumBytes)
        return engine
    }

    public func saveCache(
        to fileURL: URL,
        scope: ScriptMetaCacheScope,
        maximumBytes: UInt64
    ) throws {
        lock.lock()
        defer { lock.unlock() }
        try ensureEngineLocked().saveCache(
            to: fileURL,
            scope: scope,
            maximumBytes: maximumBytes
        )
    }

    public func writeScriptMetadata(
        fileURL: URL,
        draft: ScriptMetadataDraft,
        mode: ScriptMetaWriteMode,
        backupRootURL: URL?,
        expectedSourceFingerprint: String?
    ) throws -> ScriptMetadataFileWriteResult {
        lock.lock()
        defer { lock.unlock() }
        return try ensureEngineLocked().writeScriptMetadata(
            fileURL: fileURL,
            draft: draft,
            mode: mode,
            backupRootURL: backupRootURL,
            expectedSourceFingerprint: expectedSourceFingerprint
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

nonisolated final class SmkInputStringArena: @unchecked Sendable {
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

    func configuration() throws -> ScriptMetaKitConfiguration {
        try ScriptMetaService.decode { smk_engine_configuration_json(handle, $0) }
    }

    func cacheRevisions(rootIDs: [String], mode: ScriptMetaScanMode) throws -> [ScriptMetaCacheRevision] {
        try ScriptMetaService.withStrings(rootIDs) { roots, count in
            try ScriptMetaService.decode { smk_engine_cache_revisions_json(handle, roots, count, mode.rawValue, $0) }
        }
    }

    func applyConfiguration(_ configuration: ScriptMetaKitConfiguration) throws -> AppliedScriptMetaConfiguration {
        try ScriptMetaService.withJSON(configuration) { input in
            try ScriptMetaService.decode { smk_engine_apply_configuration_json(handle, input, $0) }
        }
    }

    func invalidateCache(scope: ScriptMetaCacheScope) throws {
        try ScriptMetaService.complete { smk_engine_invalidate_cache(handle, scope.rawValue, $0) }
    }

    func notifyChangedPaths(_ fileURLs: [URL], overflowed: Bool) throws {
        guard fileURLs.allSatisfy(\.isFileURL) else {
            throw ScriptMetaKitError.operationFailed(smkStatusInvalidArgument, "Changed paths must be file URLs.")
        }
        try ScriptMetaService.withStrings(fileURLs.map(\.path)) { paths, count in
            try ScriptMetaService.complete { smk_engine_notify_changed_paths(handle, paths, count, overflowed ? 1 : 0, $0) }
        }
    }

    func refreshDirtyRoots(mode: ScriptMetaScanMode) throws -> ScriptMetaScanResult {
        var result: OpaquePointer?
        let status = smk_engine_refresh_dirty_roots(handle, mode.rawValue, &result)
        defer { smk_scan_result_free(result) }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        return try makeResult(from: result)
    }

    func pauseWatching(rootIDs: [String]) throws {
        let status = ScriptMetaService.withStrings(rootIDs) { ids, count in
            smk_engine_pause_root_watching(handle, ids, count)
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    func resumeWatching(rootIDs: [String]) throws -> ScriptMetaScanResult {
        var result: OpaquePointer?
        let status = ScriptMetaService.withStrings(rootIDs) { ids, count in
            smk_engine_resume_root_watching(handle, ids, count, &result)
        }
        defer { smk_scan_result_free(result) }
        guard status == smkStatusOK, let result else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        return try makeResult(from: result)
    }

    func isWatchingPaused(rootID: String) throws -> Bool {
        let arena = SmkInputStringArena()
        var paused: UInt8 = 0
        let status = withExtendedLifetime(arena) {
            smk_engine_is_root_watching_paused(handle, arena.slice(rootID), &paused)
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        return paused != 0
    }

    public func setCustomMetadataKeys(_ keys: [String]) throws -> Bool {
        let arena = SmkInputStringArena()
        let values = keys.map { arena.slice($0) }
        var changed: UInt8 = 0
        let status = withExtendedLifetime(arena) {
            values.withUnsafeBufferPointer { smk_engine_set_custom_metadata_keys(handle, $0.baseAddress, $0.count, &changed) }
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        return changed != 0
    }

    public func setPreferredLanguages(_ languages: [String]) throws -> Bool {
        let arena = SmkInputStringArena()
        let values = languages.map { arena.slice($0) }
        var changed: UInt8 = 0
        let status = withExtendedLifetime(arena) {
            values.withUnsafeBufferPointer { smk_engine_set_preferred_languages(handle, $0.baseAddress, $0.count, &changed) }
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        return changed != 0
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

    public func preflightRoot(_ root: ScriptMetaKitRoot) throws -> ScriptMetaScanResult {
        let arena = SmkInputStringArena()
        var registration = rootRegistrations(from: [root], arena: arena)[0]
        var result: OpaquePointer?
        let status = withExtendedLifetime(arena) {
            withUnsafePointer(to: &registration) { rootPointer in
                smk_engine_preflight_root(handle, rootPointer, &result)
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
        var ffiItem = smkUpdateItem(from: item, arena: arena)
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
        let ffiItems = items.map { smkUpdateItem(from: $0, arena: arena) }
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

    public func setOperationalPolicy(_ policy: ScriptMetaKitOperationalPolicy) throws {
        guard policy.maxConcurrentMetaURLChecks >= 0,
              policy.retryAttempts >= 0,
              policy.watcherMaxPendingPaths >= 0 else {
            throw ScriptMetaKitError.operationFailed(
                smkStatusInvalidArgument,
                "operational policy limits must not be negative"
            )
        }
        var ffiPolicy = SmkOperationalPolicy(
            maxConcurrentMetaURLChecks: policy.maxConcurrentMetaURLChecks,
            retryAttempts: policy.retryAttempts,
            retryInitialDelayMillis: policy.retryInitialDelayMillis,
            retryBackoffMultiplier: policy.retryBackoffMultiplier,
            maxRetryDelayMillis: policy.maxRetryDelayMillis,
            requestTimeoutMillis: policy.requestTimeoutMillis,
            resourceTimeoutMillis: policy.resourceTimeoutMillis,
            watcherDebounceDelayMillis: policy.watcherDebounceDelayMillis,
            watcherMaxDeliveryDelayMillis: policy.watcherMaxDeliveryDelayMillis,
            watcherMaxPendingPaths: policy.watcherMaxPendingPaths
        )
        let status = withUnsafePointer(to: &ffiPolicy) {
            smk_engine_set_operational_policy(handle, $0)
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func setRootPreflightOptions(_ options: ScriptMetaRootPreflightOptions) throws {
        guard options.maxScannedItems >= 0,
              options.minScannedFileCountForLargeRoot >= 0,
              options.minScriptRatioDenominator >= 0,
              options.minScannedItemsForTimeLimit >= 0 else {
            throw ScriptMetaKitError.operationFailed(
                smkStatusInvalidArgument,
                "root preflight thresholds must not be negative"
            )
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

    public func loadCache(from fileURL: URL, maximumBytes: UInt64) throws {
        let arena = SmkInputStringArena()
        let status = withExtendedLifetime(arena) {
            smk_engine_load_cache_file_with_limit(
                handle,
                arena.slice(fileURL.standardizedFileURL.path),
                maximumBytes
            )
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func saveCache(
        to fileURL: URL,
        scope: ScriptMetaCacheScope,
        maximumBytes: UInt64
    ) throws {
        let arena = SmkInputStringArena()
        let status = withExtendedLifetime(arena) {
            smk_engine_save_cache_file_with_limit(
                handle,
                scope.rawValue,
                arena.slice(fileURL.standardizedFileURL.path),
                maximumBytes
            )
        }
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func cancelCurrentOperation() {
        _ = smk_engine_cancel_current_operation(handle)
    }

    public func reserveNextOperation() throws {
        let status = smk_engine_reserve_next_operation(handle)
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func finishOperationReservation() {
        _ = smk_engine_finish_operation_reservation(handle)
    }

    public func cancelCurrentOrReservedOperation() {
        _ = smk_engine_cancel_current_or_reserved_operation(handle)
    }

    public func writeScriptMetadata(
        fileURL: URL,
        draft: ScriptMetadataDraft,
        mode: ScriptMetaWriteMode,
        backupRootURL: URL?,
        expectedSourceFingerprint: String?
    ) throws -> ScriptMetadataFileWriteResult {
        let arena = SmkInputStringArena()
        var request = SmkScriptMetadataWriteRequest(
            filePath: arena.slice(fileURL.standardizedFileURL.path),
            backupRootPath: arena.slice(backupRootURL?.standardizedFileURL.path),
            writeMode: mode.ffiValue,
            draft: smkScriptMetadataDraft(from: draft, arena: arena)
        )
        var result: OpaquePointer?
        let status = try ScriptMetaService.withJSON(draft.customFields) { customFields in
            withExtendedLifetime(arena) {
                withMetadataEditExtensions(draft.extensions, arena: arena) { extensions in
                    withUnsafePointer(to: &request) { requestPointer in
                        smk_engine_write_script_metadata_file_if_unchanged_v3(
                            handle, requestPointer, extensions, customFields,
                            arena.slice(expectedSourceFingerprint), &result
                        )
                    }
                }
            }
        }
        if status == smkStatusConflict {
            throw ScriptMetaKitError.sourceConflict(lastErrorMessage())
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
        var decoded = scriptMetadataEditReadResult(
            from: readResult,
            existingLines: array(from: existingLineSlice).map(string),
            unknownLines: array(from: unknownLineSlice).map(string)
        )
        var customFields = SmkUtf8Slice()
        try check(smk_edit_result_custom_metadata_fields(result, &customFields))
        if customFields.len > 0 {
            decoded.draft.customFields = try JSONDecoder().decode([String: [String]].self, from: Data(buffer: buffer(ptr: customFields.ptr, len: customFields.len)))
        }
        var extensions = SmkMetadataEditExtensions()
        try check(smk_edit_result_metadata_extensions(result, &extensions))
        if extensions.hasExtensions != 0 {
            let names = buffer(ptr: extensions.names.ptr, len: extensions.names.len)
            let authors = buffer(ptr: extensions.authors.ptr, len: extensions.authors.len)
            decoded.draft.extensions = MetadataEditExtensions(
                defaultLanguage: optionalString(extensions.defaultLanguage),
                names: Dictionary(names.map { (string($0.language), string($0.value)) }, uniquingKeysWith: { first, second in first + " " + second }),
                authors: Dictionary(authors.map { (string($0.language), string($0.value)) }, uniquingKeysWith: { first, second in first + " " + second }),
                tags: array(from: extensions.tags).map(string)
            )
        }
        var diagnosticSlice = SmkMetadataDiagnosticSlice()
        try check(smk_edit_result_metadata_diagnostics(result, &diagnosticSlice))
        let decoder = SmkResultDecoder()
        decoded.diagnostics = buffer(ptr: diagnosticSlice.ptr, len: diagnosticSlice.len).map { metadataDiagnostic(from: $0, decoder: decoder) }
        return decoded
    }

    public func readScriptMetadataEditPreview(
        fileURL: URL,
        maxBytes: Int
    ) throws -> ScriptMetadataEditPreviewResult {
        guard maxBytes >= 0 else {
            throw ScriptMetaKitError.operationFailed(
                smkStatusInvalidArgument,
                "metadata preview byte limit must not be negative"
            )
        }
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

    public func startWatching(
        notificationSink: ScriptMetaKitWatchNotificationSink?,
        reconciliationDelivery: ScriptMetaKitReconciliationDeliveryPolicy = .complete
    ) throws {
        let notificationContext = notificationSink.map { Unmanaged.passUnretained($0).toOpaque() }
        let status = smk_engine_start_watching_with_callback_v2(
            handle,
            notificationSink == nil ? nil : watchNotificationCallback,
            notificationContext,
            reconciliationDelivery.rawValue
        )
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
    }

    public func stopWatching() {
        _ = smk_engine_stop_watching(handle)
    }

    public func watcherRequiresRestart() throws -> Bool {
        var requiresRestart: UInt8 = 0
        let status = smk_engine_watcher_requires_restart(handle, &requiresRestart)
        guard status == smkStatusOK else {
            throw ScriptMetaKitError.operationFailed(status, lastErrorMessage())
        }
        return requiresRestart != 0
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

private nonisolated struct SmkStringStorageKey: Hashable {
    let address: UInt
    let length: Int
}

/// Lives only while one C result is decoded. Returned values own their storage;
/// C addresses are never cached across result lifetimes or language changes.
private nonisolated final class SmkResultDecoder {
    private var customFieldMaps: [Range<Int>: [String: [String]]] = [:]

    func customFields(in range: Range<Int>, from values: [(String, String)]) -> [String: [String]] {
        guard !range.isEmpty else { return [:] }
        if let value = customFieldMaps[range] { return value }
        var fields: [String: [String]] = [:]
        for (key, value) in values[range] {
            fields[key, default: []].append(value)
        }
        customFieldMaps[range] = fields
        return fields
    }
    private var strings: [SmkStringStorageKey: String] = [:]
    private var tagArrays: [Range<Int>: [String]] = [:]
    private var diagnosticArrays: [Range<Int>: [MetadataDiagnostic]] = [:]

    func string(_ slice: SmkUtf8Slice) -> String {
        guard let pointer = slice.ptr, slice.len > 0 else { return "" }
        let key = SmkStringStorageKey(address: UInt(bitPattern: pointer), length: slice.len)
        if let value = strings[key] { return value }
        let value = String(decoding: UnsafeBufferPointer(start: pointer, count: slice.len), as: UTF8.self)
        strings[key] = value
        return value
    }

    func optionalString(_ slice: SmkUtf8Slice) -> String? {
        let value = string(slice)
        return value.isEmpty ? nil : value
    }

    func tags(in range: Range<Int>, from values: [String]) -> [String] {
        guard !range.isEmpty else { return [] }
        if let value = tagArrays[range] { return value }
        let value = Array(values[range])
        tagArrays[range] = value
        return value
    }

    func diagnostics(in range: Range<Int>, from values: [MetadataDiagnostic]) -> [MetadataDiagnostic] {
        guard !range.isEmpty else { return [] }
        if let value = diagnosticArrays[range] { return value }
        let value = Array(values[range])
        diagnosticArrays[range] = value
        return value
    }
}

private nonisolated func makeResult(from result: OpaquePointer) throws -> ScriptMetaScanResult {
    let decoder = SmkResultDecoder()
    var customSlice = SmkCustomMetadataValueSlice()
    try check(smk_scan_result_custom_metadata_values(result, &customSlice))
    let customMetadataValues = buffer(ptr: customSlice.ptr, len: customSlice.len).map { (decoder.string($0.key), decoder.string($0.value)) }
    var languageSlice = SmkUtf8SliceSlice()
    try check(smk_scan_result_preferred_languages(result, &languageSlice))
    let preferredLanguages = buffer(ptr: languageSlice.ptr, len: languageSlice.len).map(decoder.string)
    var tagSlice = SmkUtf8SliceSlice()
    try check(smk_scan_result_metadata_tags(result, &tagSlice))
    let metadataTags = buffer(ptr: tagSlice.ptr, len: tagSlice.len).map(decoder.string)
    var diagnosticSlice = SmkMetadataDiagnosticSlice()
    try check(smk_scan_result_metadata_diagnostics(result, &diagnosticSlice))
    let metadataDiagnostics = buffer(ptr: diagnosticSlice.ptr, len: diagnosticSlice.len).map { metadataDiagnostic(from: $0, decoder: decoder) }
    let itemPresentations = try metadataPresentations(from: result, collection: 0, tags: metadataTags, diagnostics: metadataDiagnostics, customValues: customMetadataValues, decoder: decoder)
    let fileItemPresentations = try metadataPresentations(from: result, collection: 1, tags: metadataTags, diagnostics: metadataDiagnostics, customValues: customMetadataValues, decoder: decoder)
    let fileEntryPresentations = try metadataPresentations(from: result, collection: 2, tags: metadataTags, diagnostics: metadataDiagnostics, customValues: customMetadataValues, decoder: decoder)
    let candidatePresentations = try metadataPresentations(from: result, collection: 3, tags: metadataTags, diagnostics: metadataDiagnostics, customValues: customMetadataValues, decoder: decoder)
    var rootSlice = SmkRootSnapshotSlice()
    try check(smk_scan_result_roots(result, &rootSlice))
    var rootRevisionSlice = SmkRootSnapshotRevisionSlice()
    try check(smk_scan_result_root_revisions(result, &rootRevisionSlice))
    let rootRevisions = buffer(from: rootRevisionSlice)
    try validateSnapshotSidecarIndices(
        expectedCount: rootSlice.len,
        indices: rootRevisions.map(\.rootIndex),
        label: "root revision"
    )
    let roots = buffer(from: rootSlice).enumerated().map { index, root -> RootSnapshot in
        let code = decoder.string(root.errorCode)
        let message = decoder.string(root.errorMessage)
        let error = code.isEmpty && message.isEmpty ? nil : RootError(code: code, message: message)
        var snapshot = RootSnapshot(
            rootID: decoder.string(root.rootID),
            path: decoder.string(root.path),
            status: decoder.string(root.status),
            isDirty: root.isDirty != 0,
            lastLoadedAt: root.hasLastLoadedAt != 0 ? root.lastLoadedAt : nil,
            lastEventAt: root.hasLastEventAt != 0 ? root.lastEventAt : nil,
            itemCount: root.itemCount,
            error: error
        )
        snapshot.stateRevision = ScriptMetaKitRevision(
            workspaceEpoch: decoder.string(rootRevisions[index].workspaceEpoch),
            sequence: rootRevisions[index].sequence
        )
        return snapshot
    }

    var fileEntrySlice = SmkFileEntrySlice()
    try check(smk_scan_result_file_entries(result, &fileEntrySlice))
    let ffiFileEntries = buffer(from: fileEntrySlice)
    guard fileEntryPresentations.count == ffiFileEntries.count else {
        throw ScriptMetaKitError.operationFailed(4, "file-entry metadata count mismatch")
    }

    var directoryStateSlice = SmkDirectoryStateEntrySlice()
    try check(smk_scan_result_directory_states(result, &directoryStateSlice))
    let ffiDirectoryStates = buffer(from: directoryStateSlice)

    var directoryStateRangeSlice = SmkFileListDirectoryStateRangeSlice()
    try check(smk_scan_result_file_list_directory_state_ranges(result, &directoryStateRangeSlice))
    let ffiDirectoryStateRanges = buffer(from: directoryStateRangeSlice)

    var fileListSlice = SmkFileListSnapshotSlice()
    try check(smk_scan_result_file_lists(result, &fileListSlice))
    var fileListDetailsSlice = SmkFileListSnapshotDetailsSlice()
    try check(smk_scan_result_file_list_details(result, &fileListDetailsSlice))
    let fileListDetails = buffer(from: fileListDetailsSlice)
    try validateSnapshotSidecarIndices(
        expectedCount: fileListSlice.len,
        indices: fileListDetails.map(\.fileListIndex),
        label: "file-list details"
    )
    let fileListSnapshots = buffer(from: fileListSlice).enumerated().map { index, snapshot -> FileListSnapshot in
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
        let directoryStateRange = ffiDirectoryStateRanges.indices.contains(index)
            ? ffiDirectoryStateRanges[index]
            : SmkFileListDirectoryStateRange(firstDirectoryStateIndex: 0, directoryStateCount: 0)
        let details = fileListDetails[index]
        var fileList = FileListSnapshot(
            root: root,
            children: details.hasChildren != 0 ? fileEntries(
                from: ffiFileEntries,
                firstIndex: snapshot.firstChildIndex,
                count: snapshot.childCount,
                presentations: fileEntryPresentations,
                decoder: decoder
            ) : nil,
            directoryStates: directoryStates(
                from: ffiDirectoryStates,
                firstIndex: directoryStateRange.firstDirectoryStateIndex,
                count: directoryStateRange.directoryStateCount
            ),
            truncated: snapshot.truncated != 0
        )
        fileList.contentRevision = ScriptMetaKitRevision(
            workspaceEpoch: decoder.string(details.workspaceEpoch),
            sequence: details.contentSequence
        )
        return fileList
    }

    var itemSlice = SmkScriptItemSlice()
    try check(smk_scan_result_items(result, &itemSlice))
    guard itemPresentations.count == itemSlice.len else {
        throw ScriptMetaKitError.operationFailed(4, "item metadata count mismatch")
    }
    let allItems = scriptItems(from: buffer(from: itemSlice), presentations: itemPresentations, decoder: decoder)

    var fileItemSlice = SmkScriptItemSlice()
    try check(smk_scan_result_file_items(result, &fileItemSlice))
    guard fileItemPresentations.count == fileItemSlice.len else {
        throw ScriptMetaKitError.operationFailed(4, "file-item metadata count mismatch")
    }
    let fileItems = scriptItems(from: buffer(from: fileItemSlice), presentations: fileItemPresentations, decoder: decoder)

    let updateCheckResult = try updateResult(from: result)
    let changeSummary = try scanChangeSummary(from: result, decoder: decoder)
    let operationInfo = try operationInfo(from: result)
    let fileIssues = try fileIssues(from: result)
    let catalogInfo = try catalogInfo(from: result)
    let hasCatalog = catalogInfo.hasCatalog != 0
    let catalog: ScriptMetaCatalogSnapshot?
    if hasCatalog {
        catalog = ScriptMetaCatalogSnapshot(
            sourceRevision: decoder.string(catalogInfo.sourceRevision),
            roots: roots,
            allItems: allItems,
            fileItems: fileItems,
            candidateCache: CandidateCache(
                schemaVersion: catalogInfo.candidateCacheSchemaVersion,
                builtAt: catalogInfo.candidateCacheBuiltAt,
                registeredRoots: try registeredRootSignatures(from: result),
                records: try candidateRecords(from: result, presentations: candidatePresentations, decoder: decoder)
            ),
            updateCheckResult: updateCheckResult
        )
    } else {
        catalog = nil
    }

    var watchDeliveryInfo = SmkWatchDeliveryInfo()
    try check(smk_scan_result_watch_delivery_info(result, &watchDeliveryInfo))
    return ScriptMetaScanResult(
        roots: roots,
        fileListSnapshots: fileListSnapshots,
        catalogSnapshot: catalog,
        operation: operationInfo,
        fileIssues: fileIssues,
        updateCheckResult: updateCheckResult,
        changeSummary: changeSummary,
        watchChangeBatch: try watchChangeBatch(from: result),
        watchDelivery: watchDeliveryInfo.isWatchUpdate != 0 ? ScriptMetaKitWatchDelivery(
            isReconciliation: watchDeliveryInfo.isReconciliation != 0,
            coversAllWatchedRoots: watchDeliveryInfo.coversAllWatchedRoots != 0,
            streamID: nil,
            sequence: nil
        ) : nil,
        preferredLanguages: preferredLanguages
    )
}

private nonisolated func metadataDiagnostic(from value: SmkMetadataDiagnostic, decoder: SmkResultDecoder) -> MetadataDiagnostic {
    MetadataDiagnostic(code: decoder.string(value.code), field: decoder.string(value.field),
                       language: decoder.optionalString(value.language), line: value.hasLine != 0 ? value.line : nil)
}

private nonisolated func metadataPresentations(from result: OpaquePointer, collection: UInt32,
                                             tags: [String], diagnostics: [MetadataDiagnostic], customValues: [(String, String)], decoder: SmkResultDecoder) throws -> [MetadataPresentation] {
    var values = SmkMetadataPresentationSlice()
    try check(smk_scan_result_metadata_presentations(result, collection, &values))
    var customSlice = SmkCustomMetadataRangeSlice()
    if !customValues.isEmpty {
        try check(smk_scan_result_custom_metadata_ranges(result, collection, &customSlice))
    }
    let customRanges = buffer(ptr: customSlice.ptr, len: customSlice.len)
    guard customRanges.isEmpty || customRanges.count == values.len else {
        throw ScriptMetaKitError.operationFailed(4, "invalid custom metadata range count")
    }
    return try buffer(ptr: values.ptr, len: values.len).enumerated().map { index, value in
        let custom = customRanges.isEmpty ? SmkCustomMetadataRange() : customRanges[index]
        guard let customRange = safeRange(start: custom.firstValueIndex, count: custom.valueCount, upperBound: customValues.count) else {
            throw ScriptMetaKitError.operationFailed(4, "invalid custom metadata range")
        }
        guard let tagRange = safeRange(start: value.firstTagIndex, count: value.tagCount, upperBound: tags.count),
              let diagnosticRange = safeRange(start: value.firstDiagnosticIndex, count: value.diagnosticCount, upperBound: diagnostics.count) else {
            throw ScriptMetaKitError.operationFailed(4, "invalid metadata presentation range")
        }
        return MetadataPresentation(defaultLanguage: decoder.optionalString(value.defaultLanguage),
                                    nameLanguage: decoder.optionalString(value.nameLanguage),
                                    authorLanguage: decoder.optionalString(value.authorLanguage),
                                    descriptionLanguage: decoder.optionalString(value.descriptionLanguage),
                                    tags: decoder.tags(in: tagRange, from: tags),
                                    diagnostics: decoder.diagnostics(in: diagnosticRange, from: diagnostics),
                                    customFields: decoder.customFields(in: customRange, from: customValues))
    }
}

nonisolated func validateSnapshotSidecarIndices(
    expectedCount: Int,
    indices: [Int],
    label: String
) throws {
    guard indices.count == expectedCount,
          indices.enumerated().allSatisfy({ $0.offset == $0.element }) else {
        throw ScriptMetaKitError.operationFailed(
            smkStatusInvalidArgument,
            "invalid \(label) sidecar"
        )
    }
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

private nonisolated func candidateRecords(from result: OpaquePointer, presentations: [MetadataPresentation], decoder: SmkResultDecoder) throws -> [CandidateRecord] {
    var recordSlice = SmkCandidateRecordSlice()
    try check(smk_scan_result_candidate_records(result, &recordSlice))
    guard presentations.count == recordSlice.len else {
        throw ScriptMetaKitError.operationFailed(4, "candidate metadata count mismatch")
    }
    return buffer(from: recordSlice).enumerated().map { index, record in
        CandidateRecord(
            rootID: decoder.string(record.rootID),
            rootPath: decoder.string(record.rootPath),
            filePath: decoder.string(record.filePath),
            identityPath: decoder.string(record.identityPath),
            pathKind: decoder.optionalString(record.pathKind),
            resolutionStatus: decoder.optionalString(record.resolutionStatus),
            resolutionMessage: decoder.optionalString(record.resolutionMessage),
            runtimeKind: decoder.optionalString(record.runtimeKind),
            shebang: decoder.optionalString(record.shebang),
            hasScriptMeta: record.hasScriptMeta != 0,
            hasScriptMetaEditPassword: record.hasScriptMetaEditPassword != 0,
            isFileLocked: record.isFileLocked != 0,
            isReadOnly: record.isReadOnly != 0,
            canEditScriptMeta: record.canEditScriptMeta != 0,
            canAppendScriptMeta: record.canAppendScriptMeta != 0,
            scriptMetaEditState: decoder.optionalString(record.scriptMetaEditState),
            fileSize: record.hasFileSize != 0 ? record.fileSize : nil,
            contentModifiedAt: record.hasContentModifiedAt != 0 ? record.contentModifiedAt : nil,
            item: record.hasItem != 0 ? scriptItem(from: record.item, presentation: presentations[index], decoder: decoder) : nil
        )
    }
}

private nonisolated func scriptItems(from ffiItems: UnsafeBufferPointer<SmkScriptItem>, presentations: [MetadataPresentation], decoder: SmkResultDecoder) -> [ScriptMetaItem] {
    ffiItems.enumerated().map { index, item in scriptItem(from: item, presentation: presentations[index], decoder: decoder) }
}

private nonisolated func scriptItem(from item: SmkScriptItem, presentation: MetadataPresentation, decoder: SmkResultDecoder) -> ScriptMetaItem {
    ScriptMetaItem(
        rootID: decoder.string(item.rootID),
        filePath: decoder.string(item.filePath),
        identityPath: decoder.string(item.identityPath),
        runtimeKind: decoder.optionalString(item.runtimeKind),
        shebang: decoder.optionalString(item.shebang),
        scriptID: decoder.string(item.scriptID),
        version: decoder.optionalString(item.version),
        itemDescription: decoder.optionalString(item.description),
        targetApp: decoder.optionalString(item.targetApp),
        minTargetVersion: decoder.optionalString(item.minTargetVersion),
        metaURL: decoder.optionalString(item.metaURL),
        name: decoder.optionalString(item.name),
        author: decoder.optionalString(item.author),
        releaseDate: decoder.optionalString(item.releaseDate),
        editPasswordSHA256: decoder.optionalString(item.editPasswordSHA256),
        hasScriptMeta: item.hasScriptMeta != 0,
        hasScriptMetaEditPassword: item.hasScriptMetaEditPassword != 0,
        isFileLocked: item.isFileLocked != 0,
        isReadOnly: item.isReadOnly != 0,
        canEditScriptMeta: item.canEditScriptMeta != 0,
        canAppendScriptMeta: item.canAppendScriptMeta != 0,
        scriptMetaEditState: decoder.string(item.scriptMetaEditState),
        presentation: presentation
    )
}

private nonisolated func scanChangeSummary(from result: OpaquePointer, decoder: SmkResultDecoder) throws -> ScanChangeSummary? {
    var changeInfo = SmkScanChangeInfo()
    try check(smk_scan_result_change_info(result, &changeInfo))
    guard changeInfo.hasChangeSummary != 0 else { return nil }

    var changeSlice = SmkFileEntryChangeSlice()
    try check(smk_scan_result_file_entry_changes(result, &changeSlice))
    let changes = array(from: changeSlice).map { change in
        FileEntryChange(
            rootID: decoder.string(change.rootID),
            kind: decoder.string(change.kind),
            displayPath: decoder.string(change.displayPath),
            resolvedPath: decoder.string(change.resolvedPath),
            pathKind: decoder.string(change.pathKind),
            resolutionStatus: decoder.string(change.resolutionStatus),
            resolutionMessage: decoder.optionalString(change.resolutionMessage),
            isDirectory: change.isDirectory != 0,
            fileSize: change.hasFileSize != 0 ? change.fileSize : nil,
            contentModifiedAt: change.hasContentModifiedAt != 0 ? change.contentModifiedAt : nil,
            identity: change.hasIdentity != 0 ? fileIdentity(from: change.identity) : nil,
            runtimeKind: decoder.optionalString(change.runtimeKind),
            shebang: decoder.optionalString(change.shebang),
            hasScriptMeta: change.hasScriptMeta != 0,
            hasScriptMetaEditPassword: change.hasScriptMetaEditPassword != 0,
            isFileLocked: change.isFileLocked != 0,
            isReadOnly: change.isReadOnly != 0,
            canEditScriptMeta: change.canEditScriptMeta != 0,
            canAppendScriptMeta: change.canAppendScriptMeta != 0,
            scriptMetaEditState: decoder.string(change.scriptMetaEditState)
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

private nonisolated func withMetadataEditExtensions<T>(
    _ value: MetadataEditExtensions?, arena: SmkInputStringArena,
    body: (UnsafePointer<SmkMetadataEditExtensions>?) -> T
) -> T {
    guard let value else { return body(nil) }
    let names = value.names.sorted { $0.key < $1.key }.map {
        SmkLocalizedText(language: arena.slice($0.key), value: arena.slice($0.value))
    }
    let authors = value.authors.sorted { $0.key < $1.key }.map {
        SmkLocalizedText(language: arena.slice($0.key), value: arena.slice($0.value))
    }
    let tags = value.tags.map { arena.slice($0) }
    return names.withUnsafeBufferPointer { nameBuffer in
        authors.withUnsafeBufferPointer { authorBuffer in
            tags.withUnsafeBufferPointer { tagBuffer in
                var input = SmkMetadataEditExtensions(
                    hasExtensions: 1, defaultLanguage: arena.slice(value.defaultLanguage),
                    names: SmkLocalizedTextSlice(ptr: nameBuffer.baseAddress, len: nameBuffer.count),
                    authors: SmkLocalizedTextSlice(ptr: authorBuffer.baseAddress, len: authorBuffer.count),
                    tags: SmkUtf8SliceSlice(ptr: tagBuffer.baseAddress, len: tagBuffer.count)
                )
                return withUnsafePointer(to: &input) { body($0) }
            }
        }
    }
}

/// Update resolution consumes identity, Script-ID, Version and Meta-URL only.
/// Keep the existing ABI while avoiding copies of display text and edit state.
private nonisolated func smkUpdateItem(
    from item: ScriptMetaItem,
    arena: SmkInputStringArena
) -> SmkScriptItem {
    SmkScriptItem(
        rootID: arena.slice(item.rootID),
        filePath: arena.slice(item.filePath),
        identityPath: arena.slice(item.identityPath),
        runtimeKind: SmkUtf8Slice(),
        shebang: SmkUtf8Slice(),
        scriptID: arena.slice(item.scriptID),
        version: arena.slice(item.version),
        name: SmkUtf8Slice(),
        description: SmkUtf8Slice(),
        targetApp: SmkUtf8Slice(),
        minTargetVersion: SmkUtf8Slice(),
        metaURL: arena.slice(item.metaURL),
        author: SmkUtf8Slice(),
        releaseDate: SmkUtf8Slice(),
        editPasswordSHA256: SmkUtf8Slice(),
        hasScriptMeta: 0,
        hasScriptMetaEditPassword: 0,
        isFileLocked: 0,
        isReadOnly: 0,
        canEditScriptMeta: 0,
        canAppendScriptMeta: 0,
        scriptMetaEditState: SmkUtf8Slice()
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

private nonisolated func directoryStates(
    from ffiStates: UnsafeBufferPointer<SmkDirectoryStateEntry>,
    firstIndex: Int,
    count: Int
) -> [String: DirectoryState] {
    guard let range = safeRange(start: firstIndex, count: count, upperBound: ffiStates.count) else {
        return [:]
    }
    return Dictionary(uniqueKeysWithValues: range.map { index in
        let state = ffiStates[index]
        return (
            string(state.path),
            DirectoryState(
                modificationTimeMillis: state.hasModificationTimeMillis != 0
                    ? state.modificationTimeMillis
                    : nil,
                childCount: state.childCount,
                childFingerprint: state.childFingerprint,
                identity: state.hasIdentity != 0 ? fileIdentity(from: state.identity) : nil
            )
        )
    })
}

private nonisolated func fileEntries(
    from ffiEntries: UnsafeBufferPointer<SmkFileEntry>,
    firstIndex: Int,
    count: Int,
    presentations: [MetadataPresentation],
    decoder: SmkResultDecoder
) -> [FileSystemEntry] {
    guard let range = safeRange(start: firstIndex, count: count, upperBound: ffiEntries.count) else {
        return []
    }
    return range.map { index in
        let entry = ffiEntries[index]
        return FileSystemEntry(
            displayPath: decoder.string(entry.displayPath),
            resolvedPath: decoder.string(entry.resolvedPath),
            pathKind: decoder.string(entry.pathKind),
            resolutionStatus: decoder.string(entry.resolutionStatus),
            resolutionMessage: decoder.optionalString(entry.resolutionMessage),
            isDirectory: entry.isDirectory != 0,
            fileSize: entry.hasFileSize != 0 ? entry.fileSize : nil,
            contentModifiedAt: entry.hasContentModifiedAt != 0 ? entry.contentModifiedAt : nil,
            identity: entry.hasIdentity != 0 ? fileIdentity(from: entry.identity) : nil,
            runtimeKind: decoder.optionalString(entry.runtimeKind),
            shebang: decoder.optionalString(entry.shebang),
            hasScriptMeta: entry.hasScriptMeta != 0,
            hasScriptMetaEditPassword: entry.hasScriptMetaEditPassword != 0,
            isFileLocked: entry.isFileLocked != 0,
            isReadOnly: entry.isReadOnly != 0,
            canEditScriptMeta: entry.canEditScriptMeta != 0,
            canAppendScriptMeta: entry.canAppendScriptMeta != 0,
            scriptMetaEditState: decoder.string(entry.scriptMetaEditState),
            scriptMetaItem: entry.hasScriptMetaItem != 0 ? scriptItem(from: entry.scriptMetaItem, presentation: presentations[index], decoder: decoder) : nil,
            children: fileEntries(
                from: ffiEntries,
                firstIndex: entry.firstChildIndex,
                count: entry.childCount,
                presentations: presentations,
                decoder: decoder
            )
        )
    }
}

private nonisolated func safeRange(start: Int, count: Int, upperBound: Int) -> Range<Int>? {
    guard start >= 0, count >= 0, start <= upperBound else { return nil }
    let (end, overflowed) = start.addingReportingOverflow(count)
    guard !overflowed, end >= start, end <= upperBound else { return nil }
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

private nonisolated func buffer(from slice: SmkRootSnapshotRevisionSlice) -> UnsafeBufferPointer<SmkRootSnapshotRevision> {
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

private nonisolated func buffer(from slice: SmkFileListSnapshotDetailsSlice) -> UnsafeBufferPointer<SmkFileListSnapshotDetails> {
    buffer(ptr: slice.ptr, len: slice.len)
}

private nonisolated func buffer(from slice: SmkFileListDirectoryStateRangeSlice) -> UnsafeBufferPointer<SmkFileListDirectoryStateRange> {
    buffer(ptr: slice.ptr, len: slice.len)
}

private nonisolated func buffer(from slice: SmkDirectoryStateEntrySlice) -> UnsafeBufferPointer<SmkDirectoryStateEntry> {
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

extension ScriptMetaWriteMode {
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
