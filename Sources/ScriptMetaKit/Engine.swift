import Foundation
import ScriptMetaKitFFI

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

private nonisolated let smkStatusOK = SMK_STATUS_OK
private nonisolated let smkStatusInvalidArgument = SMK_STATUS_INVALID_ARGUMENT
private nonisolated let smkStatusEngineError = SMK_STATUS_ENGINE_ERROR
private nonisolated let smkStatusConflict = SMK_STATUS_CONFLICT

extension ScriptMetaKitError {
    nonisolated static func engineCreationFailed(_ status: SmkStatus) -> Self {
        .engineCreationFailed(Int32(status.rawValue))
    }

    nonisolated static func operationFailed(_ status: SmkStatus, _ message: String) -> Self {
        .operationFailed(Int32(status.rawValue), message)
    }
}

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
    let progress = progressPointer.pointee
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
                root_id: arena.slice(root.rootID),
                path: arena.slice(root.url.standardizedFileURL.path),
                display_name: arena.slice(root.displayName),
                purpose: root.purpose.rawValue,
                watch_policy: root.watchPolicy.rawValue,
                cache_policy: root.cachePolicy.rawValue,
                refresh_policy: root.refreshPolicy.rawValue,
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
            max_concurrent_meta_url_checks: policy.maxConcurrentMetaURLChecks,
            retry_attempts: policy.retryAttempts,
            retry_initial_delay_millis: policy.retryInitialDelayMillis,
            retry_backoff_multiplier: policy.retryBackoffMultiplier,
            max_retry_delay_millis: policy.maxRetryDelayMillis,
            request_timeout_millis: policy.requestTimeoutMillis,
            resource_timeout_millis: policy.resourceTimeoutMillis,
            watcher_debounce_delay_millis: policy.watcherDebounceDelayMillis,
            watcher_max_delivery_delay_millis: policy.watcherMaxDeliveryDelayMillis,
            watcher_max_pending_paths: policy.watcherMaxPendingPaths
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
            file_path: arena.slice(fileURL.standardizedFileURL.path),
            backup_root_path: arena.slice(backupRootURL?.standardizedFileURL.path),
            write_mode: mode.ffiValue,
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
        if extensions.has_extensions != 0 {
            let names = buffer(ptr: extensions.names.ptr, len: extensions.names.len)
            let authors = buffer(ptr: extensions.authors.ptr, len: extensions.authors.len)
            decoded.draft.extensions = MetadataEditExtensions(
                defaultLanguage: optionalString(extensions.default_language),
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
        indices: rootRevisions.map(\.root_index),
        label: "root revision"
    )
    let roots = buffer(from: rootSlice).enumerated().map { index, root -> RootSnapshot in
        let code = decoder.string(root.error_code)
        let message = decoder.string(root.error_message)
        let error = code.isEmpty && message.isEmpty ? nil : RootError(code: code, message: message)
        var snapshot = RootSnapshot(
            rootID: decoder.string(root.root_id),
            path: decoder.string(root.path),
            status: decoder.string(root.status),
            isDirty: root.is_dirty != 0,
            lastLoadedAt: root.has_last_loaded_at != 0 ? root.last_loaded_at : nil,
            lastEventAt: root.has_last_event_at != 0 ? root.last_event_at : nil,
            itemCount: root.item_count,
            error: error
        )
        snapshot.stateRevision = ScriptMetaKitRevision(
            workspaceEpoch: decoder.string(rootRevisions[index].workspace_epoch),
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
        indices: fileListDetails.map(\.file_list_index),
        label: "file-list details"
    )
    let fileListSnapshots = buffer(from: fileListSlice).enumerated().map { index, snapshot -> FileListSnapshot in
        let root = roots.indices.contains(snapshot.root_index)
            ? roots[snapshot.root_index]
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
            : SmkFileListDirectoryStateRange(first_directory_state_index: 0, directory_state_count: 0)
        let details = fileListDetails[index]
        var fileList = FileListSnapshot(
            root: root,
            children: details.has_children != 0 ? fileEntries(
                from: ffiFileEntries,
                firstIndex: snapshot.first_child_index,
                count: snapshot.child_count,
                presentations: fileEntryPresentations,
                decoder: decoder
            ) : nil,
            directoryStates: directoryStates(
                from: ffiDirectoryStates,
                firstIndex: directoryStateRange.first_directory_state_index,
                count: directoryStateRange.directory_state_count
            ),
            truncated: snapshot.truncated != 0
        )
        fileList.contentRevision = ScriptMetaKitRevision(
            workspaceEpoch: decoder.string(details.workspace_epoch),
            sequence: details.content_sequence
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
    let hasCatalog = catalogInfo.has_catalog != 0
    let catalog: ScriptMetaCatalogSnapshot?
    if hasCatalog {
        catalog = ScriptMetaCatalogSnapshot(
            sourceRevision: decoder.string(catalogInfo.source_revision),
            roots: roots,
            allItems: allItems,
            fileItems: fileItems,
            candidateCache: CandidateCache(
                schemaVersion: catalogInfo.candidate_cache_schema_version,
                builtAt: catalogInfo.candidate_cache_built_at,
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
        watchDelivery: watchDeliveryInfo.is_watch_update != 0 ? ScriptMetaKitWatchDelivery(
            isReconciliation: watchDeliveryInfo.is_reconciliation != 0,
            coversAllWatchedRoots: watchDeliveryInfo.covers_all_watched_roots != 0,
            streamID: nil,
            sequence: nil
        ) : nil,
        preferredLanguages: preferredLanguages
    )
}

private nonisolated func metadataDiagnostic(from value: SmkMetadataDiagnostic, decoder: SmkResultDecoder) -> MetadataDiagnostic {
    MetadataDiagnostic(code: decoder.string(value.code), field: decoder.string(value.field),
                       language: decoder.optionalString(value.language), line: value.has_line != 0 ? value.line : nil)
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
        guard let customRange = safeRange(start: custom.first_value_index, count: custom.value_count, upperBound: customValues.count) else {
            throw ScriptMetaKitError.operationFailed(4, "invalid custom metadata range")
        }
        guard let tagRange = safeRange(start: value.first_tag_index, count: value.tag_count, upperBound: tags.count),
              let diagnosticRange = safeRange(start: value.first_diagnostic_index, count: value.diagnostic_count, upperBound: diagnostics.count) else {
            throw ScriptMetaKitError.operationFailed(4, "invalid metadata presentation range")
        }
        return MetadataPresentation(defaultLanguage: decoder.optionalString(value.default_language),
                                    nameLanguage: decoder.optionalString(value.name_language),
                                    authorLanguage: decoder.optionalString(value.author_language),
                                    descriptionLanguage: decoder.optionalString(value.description_language),
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
            rootID: string(signature.root_id),
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
            rootID: decoder.string(record.root_id),
            rootPath: decoder.string(record.root_path),
            filePath: decoder.string(record.file_path),
            identityPath: decoder.string(record.identity_path),
            pathKind: decoder.optionalString(record.path_kind),
            resolutionStatus: decoder.optionalString(record.resolution_status),
            resolutionMessage: decoder.optionalString(record.resolution_message),
            runtimeKind: decoder.optionalString(record.runtime_kind),
            shebang: decoder.optionalString(record.shebang),
            hasScriptMeta: record.has_scriptmeta != 0,
            hasScriptMetaEditPassword: record.has_scriptmeta_edit_password != 0,
            isFileLocked: record.is_file_locked != 0,
            isReadOnly: record.is_read_only != 0,
            canEditScriptMeta: record.can_edit_scriptmeta != 0,
            canAppendScriptMeta: record.can_append_scriptmeta != 0,
            scriptMetaEditState: decoder.optionalString(record.scriptmeta_edit_state),
            fileSize: record.has_file_size != 0 ? record.file_size : nil,
            contentModifiedAt: record.has_content_modified_at != 0 ? record.content_modified_at : nil,
            item: record.has_item != 0 ? scriptItem(from: record.item, presentation: presentations[index], decoder: decoder) : nil
        )
    }
}

private nonisolated func scriptItems(from ffiItems: UnsafeBufferPointer<SmkScriptItem>, presentations: [MetadataPresentation], decoder: SmkResultDecoder) -> [ScriptMetaItem] {
    ffiItems.enumerated().map { index, item in scriptItem(from: item, presentation: presentations[index], decoder: decoder) }
}

private nonisolated func scriptItem(from item: SmkScriptItem, presentation: MetadataPresentation, decoder: SmkResultDecoder) -> ScriptMetaItem {
    ScriptMetaItem(
        rootID: decoder.string(item.root_id),
        filePath: decoder.string(item.file_path),
        identityPath: decoder.string(item.identity_path),
        runtimeKind: decoder.optionalString(item.runtime_kind),
        shebang: decoder.optionalString(item.shebang),
        scriptID: decoder.string(item.script_id),
        version: decoder.optionalString(item.version),
        itemDescription: decoder.optionalString(item.description),
        targetApp: decoder.optionalString(item.target_app),
        minTargetVersion: decoder.optionalString(item.min_target_version),
        metaURL: decoder.optionalString(item.meta_url),
        name: decoder.optionalString(item.name),
        author: decoder.optionalString(item.author),
        releaseDate: decoder.optionalString(item.release_date),
        editPasswordSHA256: decoder.optionalString(item.edit_password_sha256),
        hasScriptMeta: item.has_scriptmeta != 0,
        hasScriptMetaEditPassword: item.has_scriptmeta_edit_password != 0,
        isFileLocked: item.is_file_locked != 0,
        isReadOnly: item.is_read_only != 0,
        canEditScriptMeta: item.can_edit_scriptmeta != 0,
        canAppendScriptMeta: item.can_append_scriptmeta != 0,
        scriptMetaEditState: decoder.string(item.scriptmeta_edit_state),
        presentation: presentation
    )
}

private nonisolated func scanChangeSummary(from result: OpaquePointer, decoder: SmkResultDecoder) throws -> ScanChangeSummary? {
    var changeInfo = SmkScanChangeInfo()
    try check(smk_scan_result_change_info(result, &changeInfo))
    guard changeInfo.has_change_summary != 0 else { return nil }

    var changeSlice = SmkFileEntryChangeSlice()
    try check(smk_scan_result_file_entry_changes(result, &changeSlice))
    let changes = array(from: changeSlice).map { change in
        FileEntryChange(
            rootID: decoder.string(change.root_id),
            kind: decoder.string(change.kind),
            displayPath: decoder.string(change.display_path),
            resolvedPath: decoder.string(change.resolved_path),
            pathKind: decoder.string(change.path_kind),
            resolutionStatus: decoder.string(change.resolution_status),
            resolutionMessage: decoder.optionalString(change.resolution_message),
            isDirectory: change.is_directory != 0,
            fileSize: change.has_file_size != 0 ? change.file_size : nil,
            contentModifiedAt: change.has_content_modified_at != 0 ? change.content_modified_at : nil,
            identity: change.has_identity != 0 ? fileIdentity(from: change.identity) : nil,
            runtimeKind: decoder.optionalString(change.runtime_kind),
            shebang: decoder.optionalString(change.shebang),
            hasScriptMeta: change.has_scriptmeta != 0,
            hasScriptMetaEditPassword: change.has_scriptmeta_edit_password != 0,
            isFileLocked: change.is_file_locked != 0,
            isReadOnly: change.is_read_only != 0,
            canEditScriptMeta: change.can_edit_scriptmeta != 0,
            canAppendScriptMeta: change.can_append_scriptmeta != 0,
            scriptMetaEditState: decoder.string(change.scriptmeta_edit_state)
        )
    }

    return ScanChangeSummary(
        addedCount: changeInfo.added_count,
        removedCount: changeInfo.removed_count,
        modifiedCount: changeInfo.modified_count,
        changes: changes
    )
}

private nonisolated func updateResult(from result: OpaquePointer) throws -> UpdateCheckResult? {
    var updateInfo = SmkUpdateCheckInfo()
    try check(smk_scan_result_update_info(result, &updateInfo))
    guard updateInfo.has_update_check != 0 else { return nil }

    var statusSlice = SmkUpdateStatusEntrySlice()
    try check(smk_scan_result_update_statuses(result, &statusSlice))
    let statuses = Dictionary(uniqueKeysWithValues: array(from: statusSlice).map {
        (string($0.item_id), string($0.status))
    })

    var historySlice = SmkUtf8SliceSlice()
    try check(smk_scan_result_latest_url_history_urls(result, &historySlice))
    let historyURLs = array(from: historySlice).map(string)

    var resolutionSlice = SmkDistributionResolutionEntrySlice()
    try check(smk_scan_result_update_resolutions(result, &resolutionSlice))
    let resolutions = Dictionary(uniqueKeysWithValues: array(from: resolutionSlice).map { resolution in
        let historyRange = safeRange(
            start: resolution.first_latest_url_history_index,
            count: resolution.latest_url_history_count,
            upperBound: historyURLs.count
        )
        let latestURLHistory = historyRange.map { Array(historyURLs[$0]) } ?? []
        return (
            string(resolution.item_id),
            DistributionResolution(
                latestVersion: optionalString(resolution.latest_version),
                latestPageURL: optionalString(resolution.latest_page_url),
                finalPageURL: string(resolution.final_page_url),
                latestURLHistory: latestURLHistory,
                checkedAt: resolution.checked_at,
                isUnresolved: resolution.is_unresolved != 0,
                note: optionalString(resolution.note),
                redirectCount: resolution.has_redirect_count != 0 ? resolution.redirect_count : nil
            )
        )
    })

    var failureSlice = SmkUpdateFailureEntrySlice()
    try check(smk_scan_result_update_failures(result, &failureSlice))
    let failures = Dictionary(uniqueKeysWithValues: array(from: failureSlice).map { failure in
        (
            string(failure.item_id),
            UpdateFailure(
                code: string(failure.code),
                message: string(failure.message),
                itemID: string(failure.item_id),
                filePath: string(failure.file_path),
                scriptID: string(failure.script_id),
                currentVersion: optionalString(failure.current_version),
                metaURL: optionalString(failure.meta_url),
                sourceURL: optionalString(failure.source_url),
                checkedAt: failure.checked_at
            )
        )
    })

    var errorSlice = SmkUpdateErrorEntrySlice()
    try check(smk_scan_result_update_errors(result, &errorSlice))
    let errors = Dictionary(uniqueKeysWithValues: array(from: errorSlice).map {
        (string($0.item_id), string($0.message))
    })

    return UpdateCheckResult(
        checkedAt: updateInfo.checked_at,
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
        totalUnits: info.total_units,
        completedUnits: info.completed_units,
        failedUnits: info.failed_units,
        cancelled: info.cancelled != 0,
        timedOut: info.timed_out != 0,
        reasonCode: optionalString(info.reason_code),
        message: optionalString(info.message)
    )
}

private nonisolated func fileIssues(from result: OpaquePointer) throws -> [FileIssue] {
    var issueSlice = SmkFileIssueSlice()
    try check(smk_scan_result_file_issues(result, &issueSlice))
    return array(from: issueSlice).map { issue in
        FileIssue(
            rootID: issue.has_root_id != 0 ? string(issue.root_id) : nil,
            path: string(issue.path),
            code: string(issue.code),
            message: string(issue.message),
            pathKind: optionalString(issue.path_kind),
            resolutionStatus: optionalString(issue.resolution_status),
            isDirectory: issue.is_directory != 0
        )
    }
}

private nonisolated func watchChangeBatch(from result: OpaquePointer) throws -> WatchChangeBatch? {
    var info = SmkWatchChangeInfo()
    try check(smk_scan_result_watch_change_info(result, &info))
    guard info.has_watch_change != 0 else { return nil }

    var eventSlice = SmkWatchPathEventSlice()
    try check(smk_scan_result_watch_events(result, &eventSlice))
    let events = array(from: eventSlice).map { event in
        WatchPathEvent(
            rootID: string(event.root_id),
            path: string(event.path),
            kind: string(event.kind),
            isDirectory: event.is_directory != 0,
            rescanDirectory: string(event.rescan_directory)
        )
    }

    var ignoredSlice = SmkIgnoredWatchPathSlice()
    try check(smk_scan_result_ignored_watch_paths(result, &ignoredSlice))
    let ignoredPaths = array(from: ignoredSlice).map { ignored in
        IgnoredWatchPath(
            rootID: ignored.has_root_id != 0 ? string(ignored.root_id) : nil,
            path: string(ignored.path),
            reason: string(ignored.reason)
        )
    }

    var renameSlice = SmkWatchRenameCandidateSlice()
    try check(smk_scan_result_watch_rename_candidates(result, &renameSlice))
    let renameCandidates = array(from: renameSlice).map { candidate in
        WatchRenameCandidate(
            rootID: string(candidate.root_id),
            oldPath: string(candidate.old_path),
            newPath: string(candidate.new_path),
            confidence: string(candidate.confidence)
        )
    }

    var targetSlice = SmkWatchRescanTargetSlice()
    try check(smk_scan_result_watch_rescan_targets(result, &targetSlice))
    let rescanTargets = array(from: targetSlice).map { target in
        WatchRescanTarget(
            rootID: string(target.root_id),
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
        script_id: arena.slice(draft.scriptID),
        version: arena.slice(draft.version),
        description: arena.slice(draft.itemDescription),
        target_app: arena.slice(draft.targetApp),
        min_target_version: arena.slice(draft.minTargetVersion),
        meta_url: arena.slice(draft.metaURL),
        name: arena.slice(draft.name),
        author: arena.slice(draft.author),
        release_date: arena.slice(draft.releaseDate),
        edit_password_sha256: arena.slice(draft.editPasswordSHA256)
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
                    has_extensions: 1, default_language: arena.slice(value.defaultLanguage),
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
        root_id: arena.slice(item.rootID),
        file_path: arena.slice(item.filePath),
        identity_path: arena.slice(item.identityPath),
        runtime_kind: SmkUtf8Slice(),
        shebang: SmkUtf8Slice(),
        script_id: arena.slice(item.scriptID),
        version: arena.slice(item.version),
        name: SmkUtf8Slice(),
        description: SmkUtf8Slice(),
        target_app: SmkUtf8Slice(),
        min_target_version: SmkUtf8Slice(),
        meta_url: arena.slice(item.metaURL),
        author: SmkUtf8Slice(),
        release_date: SmkUtf8Slice(),
        edit_password_sha256: SmkUtf8Slice(),
        has_scriptmeta: 0,
        has_scriptmeta_edit_password: 0,
        is_file_locked: 0,
        is_read_only: 0,
        can_edit_scriptmeta: 0,
        can_append_scriptmeta: 0,
        scriptmeta_edit_state: SmkUtf8Slice()
    )
}

private nonisolated func smkDistributionMetadataDraft(
    from draft: DistributionMetadataDraft,
    arena: SmkInputStringArena
) -> SmkDistributionMetadataDraft {
    SmkDistributionMetadataDraft(
        script_id: arena.slice(draft.scriptID),
        version: arena.slice(draft.version),
        latest_url: arena.slice(draft.latestURL),
        latest_page_url: arena.slice(draft.latestPageURL)
    )
}

private nonisolated func scriptMetadataFileWriteResult(
    from result: SmkScriptMetadataFileWriteResult
) -> ScriptMetadataFileWriteResult {
    ScriptMetadataFileWriteResult(
        filePath: string(result.file_path),
        operation: ScriptMetaWriteOperation(rawValue: string(result.operation)) ?? .unknown,
        backup: result.has_backup != 0 ? scriptMetaBackupRecord(from: result.backup) : nil
    )
}

private nonisolated func scriptMetadataEditReadResult(
    from result: SmkScriptMetadataEditReadResult,
    existingLines: [String],
    unknownLines: [String]
) -> ScriptMetadataEditReadResult {
    ScriptMetadataEditReadResult(
        filePath: string(result.file_path),
        draft: scriptMetadataDraft(from: result.draft),
        commentStyle: string(result.comment_style),
        lineEnding: string(result.line_ending),
        hasExistingBlock: result.has_existing_block != 0,
        existingLines: existingLines,
        unknownLines: unknownLines,
        existingBlockText: optionalString(result.existing_block_text),
        sourceFingerprint: string(result.source_fingerprint)
    )
}

private nonisolated func scriptMetadataEditPreviewResult(
    from result: SmkScriptMetadataEditPreviewResult
) -> ScriptMetadataEditPreviewResult {
    ScriptMetadataEditPreviewResult(
        filePath: string(result.file_path),
        previewText: string(result.preview_text),
        previewByteCount: result.preview_byte_count,
        fileSize: result.has_file_size != 0 ? result.file_size : nil,
        commentStyle: optionalString(result.comment_style),
        lineEnding: string(result.line_ending),
        hasScriptmetaMarkerInPreview: result.has_scriptmeta_marker_in_preview != 0,
        isTruncated: result.is_truncated != 0,
        requiresFullRead: result.requires_full_read != 0,
        fileStateFingerprint: string(result.file_state_fingerprint)
    )
}

private nonisolated func scriptMetadataDraft(from draft: SmkScriptMetadataDraft) -> ScriptMetadataDraft {
    ScriptMetadataDraft(
        scriptID: string(draft.script_id),
        version: optionalString(draft.version),
        itemDescription: optionalString(draft.description),
        targetApp: optionalString(draft.target_app),
        minTargetVersion: optionalString(draft.min_target_version),
        metaURL: optionalString(draft.meta_url),
        name: optionalString(draft.name),
        author: optionalString(draft.author),
        releaseDate: optionalString(draft.release_date),
        editPasswordSHA256: optionalString(draft.edit_password_sha256)
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
            item_id: arena.slice(item.itemID),
            file_path: arena.slice(item.filePath),
            script_id: arena.slice(item.scriptID)
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
    duplicates.reserveCapacity(report.duplicate_count)
    for duplicate in array(from: duplicateSlice) {
        guard let itemIDRange = safeRange(
            start: duplicate.first_item_id_index,
            count: duplicate.item_id_count,
            upperBound: itemIDs.count
        ),
            let filePathRange = safeRange(
                start: duplicate.first_file_path_index,
                count: duplicate.file_path_count,
                upperBound: filePaths.count
            ) else {
            throw ScriptMetaKitError.operationFailed(4, "invalid Script-ID uniqueness result")
        }
        duplicates.append(
            ScriptIdDuplicate(
                scriptID: string(duplicate.script_id),
                itemIDs: Array(itemIDs[itemIDRange]),
                filePaths: Array(filePaths[filePathRange])
            )
        )
    }

    return ScriptIdUniquenessReport(
        totalItems: report.total_items,
        uniqueScriptIDs: report.unique_script_ids,
        duplicates: duplicates
    )
}

private nonisolated func fileIdentity(from identity: SmkFileIdentity) -> FileIdentity {
    FileIdentity(
        stableID: string(identity.stable_id),
        volumeID: optionalString(identity.volume_id),
        fileID: optionalString(identity.file_id),
        fileSize: identity.has_file_size != 0 ? identity.file_size : nil,
        contentModifiedAt: identity.has_content_modified_at != 0 ? identity.content_modified_at : nil
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
        createdAtMillis: record.created_at_millis,
        backupFileName: string(record.backup_file_name),
        backupFilePath: string(record.backup_file_path),
        fileSize: record.file_size,
        reason: ScriptMetaBackupReason(rawValue: string(record.reason)) ?? .unknown
    )
}

private nonisolated func scriptMetaBackupGeneration(
    from generation: SmkScriptMetaBackupGeneration
) -> ScriptMetaBackupGeneration {
    ScriptMetaBackupGeneration(
        id: string(generation.id),
        sequenceNumber: generation.sequence_number,
        createdAtMillis: generation.created_at_millis,
        filePath: string(generation.file_path),
        fileSize: generation.file_size,
        reason: ScriptMetaBackupReason(rawValue: string(generation.reason)) ?? .unknown,
        isCurrentFile: generation.is_current_file != 0
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
                modificationTimeMillis: state.has_modification_time_millis != 0
                    ? state.modification_time_millis
                    : nil,
                childCount: state.child_count,
                childFingerprint: state.child_fingerprint,
                identity: state.has_identity != 0 ? fileIdentity(from: state.identity) : nil
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
            displayPath: decoder.string(entry.display_path),
            resolvedPath: decoder.string(entry.resolved_path),
            pathKind: decoder.string(entry.path_kind),
            resolutionStatus: decoder.string(entry.resolution_status),
            resolutionMessage: decoder.optionalString(entry.resolution_message),
            isDirectory: entry.is_directory != 0,
            fileSize: entry.has_file_size != 0 ? entry.file_size : nil,
            contentModifiedAt: entry.has_content_modified_at != 0 ? entry.content_modified_at : nil,
            identity: entry.has_identity != 0 ? fileIdentity(from: entry.identity) : nil,
            runtimeKind: decoder.optionalString(entry.runtime_kind),
            shebang: decoder.optionalString(entry.shebang),
            hasScriptMeta: entry.has_scriptmeta != 0,
            hasScriptMetaEditPassword: entry.has_scriptmeta_edit_password != 0,
            isFileLocked: entry.is_file_locked != 0,
            isReadOnly: entry.is_read_only != 0,
            canEditScriptMeta: entry.can_edit_scriptmeta != 0,
            canAppendScriptMeta: entry.can_append_scriptmeta != 0,
            scriptMetaEditState: decoder.string(entry.scriptmeta_edit_state),
            scriptMetaItem: entry.has_scriptmeta_item != 0 ? scriptItem(from: entry.scriptmeta_item, presentation: presentations[index], decoder: decoder) : nil,
            children: fileEntries(
                from: ffiEntries,
                firstIndex: entry.first_child_index,
                count: entry.child_count,
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
        completedItems: progress.completed_items,
        totalItems: progress.total_items,
        itemID: optionalString(progress.item_id),
        scriptID: optionalString(progress.script_id),
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

private nonisolated func check(_ status: SmkStatus) throws {
    guard status == smkStatusOK else {
        throw ScriptMetaKitError.operationFailed(status, "SCRIPTMETAKit FFI status=\(status.rawValue)")
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
