import Foundation
import ScriptMetaKitFFI

private nonisolated let smkWorkspaceStatusOK = SMK_STATUS_OK

public nonisolated enum ScriptMetaKitDiagnosticSeverity: String, Codable, Sendable {
    case information
    case warning
    case error
}

public nonisolated struct ScriptMetaKitDiagnostic: Codable, Sendable {
    public var severity: ScriptMetaKitDiagnosticSeverity
    public var code: String
    public var message: String
    public var path: String?

    public init(
        severity: ScriptMetaKitDiagnosticSeverity,
        code: String,
        message: String,
        path: String? = nil
    ) {
        self.severity = severity
        self.code = code
        self.message = message
        self.path = path
    }
}

public nonisolated struct ScriptMetaKitWatchSequence: AsyncSequence, Sendable {
    public typealias Element = ScriptMetaScanResult
    public typealias AsyncIterator = AsyncThrowingStream<Element, Error>.Iterator

    private let stream: AsyncThrowingStream<Element, Error>

    init(stream: AsyncThrowingStream<Element, Error>) {
        self.stream = stream
    }

    init(updates: ScriptMetaKitWatchUpdateSequence) {
        var iterator = updates.makeAsyncIterator()
        let (stream, continuation) = AsyncThrowingStream<Element, Error>.makeStream(
            bufferingPolicy: .bufferingNewest(1)
        )
        let task = Task {
            do {
                while let update = try await iterator.next() {
                    continuation.yield(update.result)
                }
                continuation.finish()
            } catch {
                continuation.finish(throwing: error)
            }
        }
        continuation.onTermination = { _ in task.cancel() }
        self.stream = stream
    }

    public func makeAsyncIterator() -> AsyncIterator {
        stream.makeAsyncIterator()
    }
}

public nonisolated enum ScriptMetaKitFileListFreshness: String, Codable, Sendable {
    case unavailable, cachedUnverified, current, failedRetainingLastGood
}

public nonisolated enum ScriptMetaKitFileListCompleteness: String, Codable, Sendable {
    case unavailable, complete, truncated
}

public nonisolated enum ScriptMetaKitFileListSource: String, Codable, Sendable {
    case none, memoryCache, persistentCache
}

public nonisolated struct ScriptMetaKitFileListState: Sendable {
    public let root: RootSnapshot
    public let availableSnapshot: FileListSnapshot?
    public let freshness: ScriptMetaKitFileListFreshness
    public let completeness: ScriptMetaKitFileListCompleteness
    public let source: ScriptMetaKitFileListSource
    public var reconciliationRequired: Bool { freshness != .current }
    public var isFullyCurrent: Bool { freshness == .current && completeness == .complete }
}

public nonisolated enum ScriptMetaKitWatchUpdateKind: String, Codable, Sendable {
    case incremental, reconciliation
}

public nonisolated enum ScriptMetaKitReconciliationDeliveryPolicy: UInt32, Codable, Sendable {
    /// Preserves the established behavior: one reconciliation update is delivered
    /// after every watched root has been reconciled.
    case complete = 0

    /// Delivers reconciliation in root-priority batches, followed by one complete
    /// update whose `coversAllWatchedRoots` value is `true`.
    case progressiveByRootPriority = 1
}

public nonisolated struct ScriptMetaKitWatchUpdate: Sendable {
    public let streamID: String
    public let sequence: UInt64
    public let kind: ScriptMetaKitWatchUpdateKind
    public let coversAllWatchedRoots: Bool
    /// Watched roots reconciled in the current reconciliation cycle up to and
    /// including this update. Consumers must still inspect each root's freshness
    /// and completeness before allowing mutations or script execution.
    public let reconciledRootIDs: [String]
    /// Watched roots not yet reconciled in the current reconciliation cycle.
    public let pendingRootIDs: [String]
    public let result: ScriptMetaScanResult

    init(
        streamID: String,
        sequence: UInt64,
        kind: ScriptMetaKitWatchUpdateKind,
        coversAllWatchedRoots: Bool,
        reconciledRootIDs: [String] = [],
        pendingRootIDs: [String] = [],
        result: ScriptMetaScanResult
    ) {
        self.streamID = streamID
        self.sequence = sequence
        self.kind = kind
        self.coversAllWatchedRoots = coversAllWatchedRoots
        self.reconciledRootIDs = reconciledRootIDs
        self.pendingRootIDs = pendingRootIDs
        self.result = result
    }
}

public nonisolated struct ScriptMetaKitWatchUpdateSequence: AsyncSequence, Sendable {
    public typealias Element = ScriptMetaKitWatchUpdate
    public typealias AsyncIterator = AsyncThrowingStream<Element, Error>.Iterator
    private let stream: AsyncThrowingStream<Element, Error>

    init(stream: AsyncThrowingStream<Element, Error>) {
        self.stream = stream
    }

    public func makeAsyncIterator() -> AsyncIterator {
        stream.makeAsyncIterator()
    }
}

public nonisolated struct ScriptMetaKitPersistentCacheStore: Sendable {
    public static let defaultMaximumCacheBytes = 64 * 1024 * 1024

    public var directoryURL: URL
    public var maximumCacheBytes: Int

    public init(
        directoryURL: URL,
        maximumCacheBytes: Int = Self.defaultMaximumCacheBytes
    ) {
        self.directoryURL = directoryURL
        self.maximumCacheBytes = maximumCacheBytes
    }

    public var validatedMaximumCacheBytes: UInt64? {
        guard maximumCacheBytes > 0,
              maximumCacheBytes <= Int(ScriptMetaKitEngine.maximumCacheFileBytes) else {
            return nil
        }
        return UInt64(maximumCacheBytes)
    }

    public static func applicationSupport(
        bundleIdentifier: String,
        maximumCacheBytes: Int = Self.defaultMaximumCacheBytes,
        fileManager: FileManager = .default
    ) -> ScriptMetaKitPersistentCacheStore? {
        guard let baseURL = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            return nil
        }
        return ScriptMetaKitPersistentCacheStore(
            directoryURL: baseURL.appendingPathComponent(bundleIdentifier, isDirectory: true),
            maximumCacheBytes: maximumCacheBytes
        )
    }

    public func readableCacheFileURL(scope: ScriptMetaCacheScope) -> URL? {
        let url = cacheFileURL(scope: scope)
        if isReadableCacheFile(url) {
            return url
        }
        if scope == .root {
            let legacyURL = legacyRootCacheFileURL()
            return isReadableCacheFile(legacyURL) ? legacyURL : nil
        }
        return nil
    }

    public func writableCacheFileURL(scope: ScriptMetaCacheScope) -> URL {
        cacheFileURL(scope: scope)
    }

    public func enforceSizeLimit(scope: ScriptMetaCacheScope) {
        guard let maximumCacheBytes = validatedMaximumCacheBytes else { return }
        var urls = [cacheFileURL(scope: scope)]
        if scope == .root {
            urls.append(legacyRootCacheFileURL())
        }
        for url in urls {
            guard let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey]),
                  values.isRegularFile == true,
                  let fileSize = values.fileSize,
                  UInt64(fileSize) > maximumCacheBytes else {
                continue
            }
            try? FileManager.default.removeItem(at: url)
        }
    }

    public func remove(scope: ScriptMetaCacheScope) {
        try? FileManager.default.removeItem(at: cacheFileURL(scope: scope))
        if scope == .root {
            try? FileManager.default.removeItem(at: legacyRootCacheFileURL())
        }
    }

    private func cacheFileURL(scope: ScriptMetaCacheScope) -> URL {
        directoryURL.appendingPathComponent(fileName(scope: scope), isDirectory: false)
    }

    private func fileName(scope: ScriptMetaCacheScope) -> String {
        switch scope {
        case .catalog:
            "ScriptMetaKitCatalogCache.cache"
        case .fileList:
            "ScriptMetaKitFileListCache.cache"
        case .all:
            "ScriptMetaKitCache.cache"
        case .root:
            "ScriptMetaKitFileListCache.cache"
        }
    }

    private func legacyRootCacheFileURL() -> URL {
        directoryURL.appendingPathComponent("ScriptMetaKitRootCache.cache", isDirectory: false)
    }

    private func isReadableCacheFile(_ url: URL) -> Bool {
        guard let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey]),
              values.isRegularFile == true,
              let fileSize = values.fileSize else {
            return false
        }
        return fileSize <= maximumCacheBytes
    }
}

public nonisolated struct ScriptMetaKitWorkspaceConfiguration: Sendable {
    public var preferredLanguages: [String]
    public var cacheStore: ScriptMetaKitPersistentCacheStore?
    public var rootPreflightOptions: ScriptMetaRootPreflightOptions?
    public var resolvesMacOSAliases: Bool
    public var nativeEventLatencyMillis: UInt64?
    public var cacheSaveDebounceMillis: UInt64
    public var operationalPolicy: ScriptMetaKitOperationalPolicy?
    public var diagnosticHandler: (@Sendable (ScriptMetaKitDiagnostic) -> Void)?

    public init(
        cacheStore: ScriptMetaKitPersistentCacheStore? = nil,
        rootPreflightOptions: ScriptMetaRootPreflightOptions? = nil,
        resolvesMacOSAliases: Bool = true,
        nativeEventLatencyMillis: UInt64? = nil,
        cacheSaveDebounceMillis: UInt64 = 300,
        operationalPolicy: ScriptMetaKitOperationalPolicy? = nil,
        diagnosticHandler: (@Sendable (ScriptMetaKitDiagnostic) -> Void)? = nil,
        preferredLanguages: [String] = []
    ) {
        self.cacheStore = cacheStore
        self.rootPreflightOptions = rootPreflightOptions
        self.resolvesMacOSAliases = resolvesMacOSAliases
        self.nativeEventLatencyMillis = nativeEventLatencyMillis
        self.cacheSaveDebounceMillis = cacheSaveDebounceMillis
        self.operationalPolicy = operationalPolicy
        self.diagnosticHandler = diagnosticHandler
        self.preferredLanguages = preferredLanguages
    }
}

public actor ScriptMetaKitWorkspace {
    private struct ExclusiveOperationWaiter {
        let id: UInt64
        let continuation: CheckedContinuation<Bool, Never>
    }

    private struct PersistentCacheFileSignature: Equatable {
        let fileSize: Int
        let modificationDate: Date?
    }

    private struct PersistentCacheLoadState {
        var signature: PersistentCacheFileSignature
        var rootRevisions: [String: ScriptMetaKitRevision]
    }

    private let engine = ScriptMetaKitEngine()
    private let configuration: ScriptMetaKitWorkspaceConfiguration
    private var didConfigureEngine = false
    private var persistentCacheEnabled = false
    private var hasExclusiveOperation = false
    private var nextExclusiveOperationWaiterID: UInt64 = 0
    private var exclusiveOperationWaiters: [ExclusiveOperationWaiter] = []
    private var cacheSaveGenerationByScope: [UInt32: UInt64] = [:]
    private var pendingCacheSaveTasks: [UInt32: Task<Void, Never>] = [:]
    private var persistentCacheLoadStates: [UInt32: PersistentCacheLoadState] = [:]
    private var persistentFileListRootIDs: Set<String> = []
    private var registeredRootIDsByGroup: [String: Set<String>] = [:]
    private var pausedWatchRootIDs: Set<String> = []
    private var watchControlGeneration: UInt64 = 0
    private var beforeWatchPublicationForTesting: (@Sendable () async -> Void)?

    private struct WatchPublicationState: Sendable {
        var sequence: UInt64 = 0
        var reconciledRootIDs: Set<String> = []
        var reconciliationIsInProgress = false
    }
    private var activeWatchSessionID: String?
    private var activeWatchSessionInvalidator: (@Sendable () -> Void)?

    public init(configuration: ScriptMetaKitWorkspaceConfiguration = ScriptMetaKitWorkspaceConfiguration()) {
        self.configuration = configuration
    }

    deinit {
        activeWatchSessionInvalidator?()
        for task in pendingCacheSaveTasks.values { task.cancel() }
    }

    public func engineConfiguration() async throws -> ScriptMetaKitConfiguration {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try await configureEngineIfNeeded()
        return try await engine.configuration()
    }

    /// A workspace without a cache store disables persistent cache retention.
    /// Runtime settings survive shutdown and clearVolatileState.
    @discardableResult
    public func applyConfiguration(_ requested: ScriptMetaKitConfiguration) async throws -> ScriptMetaKitConfigurationChange {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try await configureEngineIfNeeded()
        var effective = requested
        if configuration.cacheStore == nil { effective.cache.persistentCache = false }
        let change = try await engine.applyConfiguration(effective)
        if !effective.watcher.allowsWatching { invalidateActiveWatchSession() }
        persistentCacheEnabled = effective.cache.enabled && effective.cache.persistentCache
        if change.metadataInvalidated || change.persistenceChanged {
            resetPersistentCacheTracking(clearFileListProvenance: change.metadataInvalidated)
        }
        return change
    }

    /// Also removes this workspace's stored caches containing the requested scope.
    public func invalidateCache(scope: ScriptMetaCacheScope = .all) async throws {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try await configureEngineIfNeeded()
        try await engine.invalidateCache(scope: scope)
        resetPersistentCacheTracking(clearFileListProvenance: scope != .catalog)
        let store = configuration.cacheStore
        store?.remove(scope: .all)
        if scope == .all || scope == .catalog { store?.remove(scope: .catalog) }
        if scope == .all || scope == .fileList || scope == .root {
            store?.remove(scope: .fileList)
            store?.remove(scope: .root)
        }
    }

    public func notifyChangedPaths(_ fileURLs: [URL], overflowed: Bool = false) async throws {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try await configureEngineIfNeeded()
        try await engine.notifyChangedPaths(fileURLs, overflowed: overflowed)
    }

    public func refreshDirtyRoots(mode: ScriptMetaScanMode = .fileListAndMetadata, cacheScope: ScriptMetaCacheScope? = nil) async throws -> ScriptMetaScanResult {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try await configureEngineIfNeeded()
        let result = try await engine.refreshDirtyRoots(mode: mode)
        markReconciledFileListRoots(in: result)
        if let cacheScope { await savePersistentCache(scope: cacheScope) }
        return result
    }

    /// Suspends automatic reads for these root IDs while retaining the current
    /// watch stream, other roots, policies and cached content. Explicit scans are
    /// still allowed. Pause calls are idempotent, not reference counted.
    public func pauseWatching(rootIDs: [String]) async throws {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        try await engine.pauseWatching(rootIDs: rootIDs)
        let previous = pausedWatchRootIDs
        pausedWatchRootIDs.formUnion(rootIDs)
        if previous != pausedWatchRootIDs { watchControlGeneration &+= 1 }
    }

    /// Reloads file lists and metadata without unchanged-record reuse, then
    /// resumes the roots' configured watch policies. Monitoring is armed before
    /// the read when the workspace already has a running watcher.
    /// Incomplete results leave the entire batch paused; inspect root statuses.
    /// Does not start a globally stopped watcher or perform network update checks.
    public func resumeWatching(rootIDs: [String], cacheScope: ScriptMetaCacheScope? = nil) async throws -> ScriptMetaScanResult {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        let result: ScriptMetaScanResult
        do {
            result = try await engine.resumeWatching(rootIDs: rootIDs)
        } catch {
            // Cancellation can occur after the bridge has begun its source read.
            // Query the engine instead of assuming the pre-call suspension state.
            let engine = self.engine
            let suspended = try? await Task.detached { try await engine.pausedWatchRoots(rootIDs: rootIDs) }.value
            if let suspended {
                let previous = pausedWatchRootIDs
                pausedWatchRootIDs.subtract(rootIDs)
                pausedWatchRootIDs.formUnion(suspended)
                if previous != pausedWatchRootIDs { watchControlGeneration &+= 1 }
            }
            throw error
        }
        if result.roots.allSatisfy({ $0.status == "ready" }) {
            pausedWatchRootIDs.subtract(rootIDs)
            markReconciledFileListRoots(in: result)
        } else {
            pausedWatchRootIDs.formUnion(rootIDs)
        }
        watchControlGeneration &+= 1
        if let cacheScope { await savePersistentCache(scope: cacheScope) }
        return result
    }

    public func isWatchingPaused(rootID: String) async throws -> Bool {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try await configureEngineIfNeeded()
        return try await engine.isWatchingPaused(rootID: rootID)
    }

    /// The consumer supplies priorities. Call scanRoots/scanRoot after a change
    /// to reload source metadata; the Kit does not monitor OS language changes.
    public func setPreferredLanguages(_ languages: [String]) async throws {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try await configureEngineIfNeeded()
        let changed = try await engine.setPreferredLanguages(languages)
        guard changed else { return }
        resetPersistentCacheTracking()
    }

    private func resetPersistentCacheTracking(clearFileListProvenance: Bool = true) {
        for task in pendingCacheSaveTasks.values { task.cancel() }
        pendingCacheSaveTasks.removeAll()
        cacheSaveGenerationByScope = cacheSaveGenerationByScope.mapValues { $0 &+ 1 }
        persistentCacheLoadStates.removeAll()
        if clearFileListProvenance { persistentFileListRootIDs.removeAll() }
    }

    func performExclusiveOperation<T: Sendable>(
        _ operation: @Sendable () async throws -> T
    ) async throws -> T {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        return try await operation()
    }

    public func scanRoots(
        _ roots: [ScriptMetaKitRoot],
        replacingGroup groupID: String,
        rootIDs: [String],
        mode: ScriptMetaScanMode,
        checkUpdates: Bool = false,
        cacheScope: ScriptMetaCacheScope? = nil,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> ScriptMetaScanResult {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        try await replaceRoots(roots, groupID: groupID)
        if let cacheScope {
            try await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: rootIDs)
        }
        let result = try await engine.scanRoots(
            rootIDs: rootIDs,
            mode: mode,
            checkUpdates: checkUpdates,
            onProgress: onProgress
        )
        if mode == .fileListOnly || mode == .fileListAndMetadata {
            markReconciledFileListRoots(in: result)
        }
        if let cacheScope {
            await savePersistentCache(scope: cacheScope)
        }
        return result
    }

    public func scanRoot(
        _ root: ScriptMetaKitRoot,
        insertingIntoGroup groupID: String,
        mode: ScriptMetaScanMode,
        checkUpdates: Bool = false,
        cacheScope: ScriptMetaCacheScope? = nil,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> ScriptMetaScanResult {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        try await mergeRoot(root, groupID: groupID)
        if let cacheScope {
            try await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: [root.rootID])
        }
        let result = try await engine.scanRoot(
            rootID: root.rootID,
            mode: mode,
            checkUpdates: checkUpdates,
            onProgress: onProgress
        )
        if mode == .fileListOnly || mode == .fileListAndMetadata {
            markReconciledFileListRoots(in: result)
        }
        if let cacheScope {
            await savePersistentCache(scope: cacheScope)
        }
        return result
    }

    public func scanRoots(
        _ roots: [ScriptMetaKitRoot],
        insertingIntoGroup groupID: String,
        mode: ScriptMetaScanMode,
        checkUpdates: Bool = false,
        cacheScope: ScriptMetaCacheScope? = nil,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> ScriptMetaScanResult {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        try await mergeRoots(roots, groupID: groupID)
        let rootIDs = roots.map(\.rootID)
        if let cacheScope {
            try await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: rootIDs)
        }
        let result = try await engine.scanRoots(
            rootIDs: rootIDs,
            mode: mode,
            checkUpdates: checkUpdates,
            onProgress: onProgress
        )
        if mode == .fileListOnly || mode == .fileListAndMetadata {
            markReconciledFileListRoots(in: result)
        }
        if let cacheScope {
            await savePersistentCache(scope: cacheScope)
        }
        return result
    }

    /// Replaces a complete root group, scans only the requested roots, and returns
    /// the current combined result for `resultRootIDs` as one non-reentrant operation.
    public func scanRegisteredRoots(
        _ roots: [ScriptMetaKitRoot],
        replacingGroup groupID: String,
        scanningRootIDs: [String],
        resultRootIDs: [String],
        mode: ScriptMetaScanMode,
        checkUpdates: Bool = false,
        cacheScope: ScriptMetaCacheScope? = nil,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> ScriptMetaScanResult {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        try await replaceRoots(roots, groupID: groupID)
        if let cacheScope {
            try await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: resultRootIDs)
        }
        let scannedResult = try await engine.scanRoots(
            rootIDs: scanningRootIDs,
            mode: mode,
            checkUpdates: checkUpdates,
            onProgress: onProgress
        )
        if mode == .fileListOnly || mode == .fileListAndMetadata {
            markReconciledFileListRoots(in: scannedResult)
        }
        let result: ScriptMetaScanResult
        if resultRootIDs == scanningRootIDs {
            result = scannedResult
        } else {
            result = try await engine.cachedRoots(rootIDs: resultRootIDs, mode: mode)
        }
        if let cacheScope {
            await savePersistentCache(scope: cacheScope)
        }
        return result
    }

    public func cachedScanResult(
        rootIDs: [String],
        mode: ScriptMetaScanMode,
        cacheScope: ScriptMetaCacheScope? = nil
    ) async throws -> ScriptMetaScanResult {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        if let cacheScope {
            try await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: rootIDs)
        }
        return try await engine.cachedRoots(rootIDs: rootIDs, mode: mode)
    }

    public func cachedFileListSnapshot(
        rootID: String,
        cacheScope: ScriptMetaCacheScope? = .fileList
    ) async throws -> FileListSnapshot? {
        let result = try await cachedScanResult(
            rootIDs: [rootID],
            mode: .fileListOnly,
            cacheScope: cacheScope
        )
        return result.fileListSnapshots.first { $0.root.rootID == rootID }
    }

    public func cachedFileListStates(
        rootIDs: [String],
        cacheScope: ScriptMetaCacheScope? = .fileList
    ) async throws -> [String: ScriptMetaKitFileListState] {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        guard !rootIDs.isEmpty else { return [:] }
        let registeredResult = try await engine.cachedRoots(rootIDs: rootIDs, mode: .fileListOnly)
        try validateRegisteredRootIDs(rootIDs, in: registeredResult)
        if let cacheScope { try await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: rootIDs) }
        let result = cacheScope == nil
            ? registeredResult
            : try await engine.cachedRoots(rootIDs: rootIDs, mode: .fileListOnly)
        return Dictionary(uniqueKeysWithValues: result.roots.map { root in
            let snapshot = result.fileListSnapshots.first { $0.root.rootID == root.rootID }
            return (root.rootID, makeFileListState(root: root, snapshot: snapshot))
        })
    }

    public func activateFileListRoot(
        _ rootID: String?,
        cacheScope: ScriptMetaCacheScope? = .fileList
    ) async throws -> ScriptMetaKitFileListState? {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        guard let rootID else {
            try await engine.setVisibleRoot(nil)
            return nil
        }
        let registeredResult = try await engine.cachedRoots(rootIDs: [rootID], mode: .fileListOnly)
        try validateRegisteredRootIDs([rootID], in: registeredResult)
        if let cacheScope { try await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: [rootID]) }
        let result = cacheScope == nil
            ? registeredResult
            : try await engine.cachedRoots(rootIDs: [rootID], mode: .fileListOnly)
        guard let root = result.roots.first(where: { $0.rootID == rootID }) else {
            throw ScriptMetaKitWorkspaceError.unknownRootID(rootID)
        }
        let state = makeFileListState(
            root: root,
            snapshot: result.fileListSnapshots.first { $0.root.rootID == rootID }
        )
        try Task.checkCancellation()
        try await engine.setVisibleRoot(rootID)
        return state
    }

    private func validateRegisteredRootIDs(
        _ rootIDs: [String],
        in result: ScriptMetaScanResult
    ) throws {
        let registeredRootIDs = Set(result.roots.map(\.rootID))
        guard registeredRootIDs == Set(rootIDs) else {
            throw ScriptMetaKitWorkspaceError.unknownRootID(
                rootIDs.first { !registeredRootIDs.contains($0) } ?? ""
            )
        }
    }

    func makeFileListState(root: RootSnapshot, snapshot: FileListSnapshot?) -> ScriptMetaKitFileListState {
        guard var snapshot, snapshot.children != nil else {
            return ScriptMetaKitFileListState(root: root, availableSnapshot: nil, freshness: .unavailable, completeness: .unavailable, source: .none)
        }
        snapshot.root = root
        let freshness: ScriptMetaKitFileListFreshness
        if root.status == "ready" && !root.isDirty {
            freshness = persistentFileListRootIDs.contains(root.rootID) ? .cachedUnverified : .current
        } else if ["missing", "unreadable", "timed_out", "cancelled"].contains(root.status) {
            freshness = .failedRetainingLastGood
        } else {
            freshness = .cachedUnverified
        }
        return ScriptMetaKitFileListState(
            root: root,
            availableSnapshot: snapshot,
            freshness: freshness,
            completeness: snapshot.truncated ? .truncated : .complete,
            source: persistentFileListRootIDs.contains(root.rootID) ? .persistentCache : .memoryCache
        )
    }

    private func markReconciledFileListRoots(in result: ScriptMetaScanResult) {
        persistentFileListRootIDs.subtract(
            result.roots.filter { $0.status == "ready" && !$0.isDirty }.map(\.rootID)
        )
    }

    public func cachedFileListSnapshot(
        _ root: ScriptMetaKitRoot,
        insertingIntoGroup groupID: String,
        cacheScope: ScriptMetaCacheScope? = .fileList
    ) async throws -> FileListSnapshot? {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        try await mergeRoot(root, groupID: groupID)
        if let cacheScope {
            try await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: [root.rootID])
        }
        let result = try await engine.cachedRoots(rootIDs: [root.rootID], mode: .fileListOnly)
        return result.fileListSnapshots.first { $0.root.rootID == root.rootID }
    }

    public func cachedCatalogSnapshot(
        roots: [ScriptMetaKitRoot],
        replacingGroup groupID: String,
        rootIDs: [String],
        cacheScope: ScriptMetaCacheScope? = .catalog
    ) async throws -> ScriptMetaCatalogSnapshot? {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        try await replaceRoots(roots, groupID: groupID)
        if let cacheScope {
            try await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: rootIDs)
        }
        let result = try await engine.cachedRoots(rootIDs: rootIDs, mode: .metadataOnly)
        return result.catalogSnapshot
    }

    public func registerRoots(
        _ roots: [ScriptMetaKitRoot],
        replacingGroup groupID: String,
        cacheScope: ScriptMetaCacheScope? = nil
    ) async throws {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        try await replaceRoots(roots, groupID: groupID)
        if let cacheScope {
            try await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: roots.map(\.rootID))
        }
    }

    public func setVisibleRoot(_ rootID: String?) async throws {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        try await engine.setVisibleRoot(rootID)
    }

    public func startWatching(
        roots: [ScriptMetaKitRoot],
        replacingGroup groupID: String,
        cacheScope: ScriptMetaCacheScope? = nil,
        drainsInitialChanges: Bool = false,
        initialDrainDirtyOnly: Bool = false,
        onChange: @escaping @Sendable () -> Void
    ) async throws {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        invalidateActiveWatchSession()
        do {
            try await startWatchingUnlocked(
                roots: roots,
                replacingGroup: groupID,
                cacheScope: cacheScope,
                drainsInitialChanges: drainsInitialChanges,
                initialDrainDirtyOnly: initialDrainDirtyOnly,
                reconciliationDelivery: .complete,
                onChange: onChange
            )
        } catch {
            await engine.stopWatching()
            throw error
        }
    }

    private func startWatchingUnlocked(
        roots: [ScriptMetaKitRoot],
        replacingGroup groupID: String,
        cacheScope: ScriptMetaCacheScope?,
        drainsInitialChanges: Bool,
        initialDrainDirtyOnly: Bool,
        reconciliationDelivery: ScriptMetaKitReconciliationDeliveryPolicy,
        onChange: @escaping @Sendable () -> Void
    ) async throws {
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        try await replaceRoots(roots, groupID: groupID)
        if let cacheScope {
            try await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: roots.map(\.rootID))
        }
        try await engine.startWatching(
            onChange: onChange,
            reconciliationDelivery: reconciliationDelivery
        )
        if drainsInitialChanges {
            try await drainWatchChangesUnlocked(dirtyOnly: initialDrainDirtyOnly)
            if let cacheScope {
                await savePersistentCache(scope: cacheScope)
            }
        }
    }

    public func watchChanges(
        roots: [ScriptMetaKitRoot],
        replacingGroup groupID: String,
        cacheScope: ScriptMetaCacheScope? = nil,
        dirtyOnly: Bool = false
    ) async throws -> ScriptMetaKitWatchSequence {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        return ScriptMetaKitWatchSequence(updates: try await startWatchUpdateSessionUnlocked(
            roots: roots,
            replacingGroup: groupID,
            cacheScope: cacheScope,
            dirtyOnly: dirtyOnly,
            reconciliationDelivery: .complete
        ))
    }

    /// Starts a sequenced watch-update stream. Complete reconciliation delivery
    /// remains the default; progressive delivery emits root-priority batches and
    /// ends each cycle with `coversAllWatchedRoots == true`.
    public func watchUpdates(
        roots: [ScriptMetaKitRoot],
        replacingGroup groupID: String,
        cacheScope: ScriptMetaCacheScope? = nil,
        dirtyOnly: Bool = false,
        reconciliationDelivery: ScriptMetaKitReconciliationDeliveryPolicy = .complete
    ) async throws -> ScriptMetaKitWatchUpdateSequence {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        return try await startWatchUpdateSessionUnlocked(
            roots: roots,
            replacingGroup: groupID,
            cacheScope: cacheScope,
            dirtyOnly: dirtyOnly,
            reconciliationDelivery: reconciliationDelivery
        )
    }

    private func startWatchUpdateSessionUnlocked(
        roots: [ScriptMetaKitRoot],
        replacingGroup groupID: String,
        cacheScope: ScriptMetaCacheScope?,
        dirtyOnly: Bool,
        reconciliationDelivery: ScriptMetaKitReconciliationDeliveryPolicy
    ) async throws -> ScriptMetaKitWatchUpdateSequence {
        let watchedRootIDs = Set(roots.map(\.rootID))
        let streamID = UUID().uuidString
        invalidateActiveWatchSession()
        activeWatchSessionID = streamID
        let (notifications, notificationContinuation) = AsyncStream<Void>.makeStream(
            bufferingPolicy: .bufferingNewest(1)
        )
        let (updates, updateContinuation) = AsyncThrowingStream<ScriptMetaKitWatchUpdate, Error>.makeStream(
            bufferingPolicy: .bufferingNewest(1)
        )
        let pump = Task { [weak self] in
            var publicationState = WatchPublicationState()
            for await _ in notifications {
                guard let self else { break }
                do {
                    guard let polled = try await self.pollWatchChanges(
                        sessionID: streamID, dirtyOnly: dirtyOnly, cacheScope: cacheScope
                    ) else { continue }
                    if let hook = await self.takeWatchPublicationHookForTesting() { await hook() }
                    publicationState = try await self.publishWatchResult(
                        polled.result, generation: polled.generation, streamID: streamID,
                        watchedRootIDs: watchedRootIDs, state: publicationState,
                        continuation: updateContinuation
                    )
                } catch is CancellationError {
                    break
                } catch {
                    updateContinuation.finish(throwing: error)
                    return
                }
            }
            updateContinuation.finish()
        }
        updateContinuation.onTermination = { [weak self] _ in
            pump.cancel()
            notificationContinuation.finish()
            Task { await self?.stopWatching(sessionID: streamID) }
        }
        activeWatchSessionInvalidator = {
            pump.cancel()
            notificationContinuation.finish()
            updateContinuation.finish()
        }
        do {
            try await startWatchingUnlocked(
                roots: roots,
                replacingGroup: groupID,
                cacheScope: cacheScope,
                drainsInitialChanges: false,
                initialDrainDirtyOnly: false,
                reconciliationDelivery: reconciliationDelivery,
                onChange: { notificationContinuation.yield() }
            )
        } catch {
            if activeWatchSessionID == streamID {
                activeWatchSessionID = nil
                activeWatchSessionInvalidator = nil
            }
            pump.cancel()
            notificationContinuation.finish()
            updateContinuation.finish(throwing: error)
            await engine.stopWatching()
            throw error
        }
        return ScriptMetaKitWatchUpdateSequence(stream: updates)
    }

    private func pollWatchChanges(
        sessionID: String,
        dirtyOnly: Bool,
        cacheScope: ScriptMetaCacheScope?
    ) async throws -> (result: ScriptMetaScanResult, generation: UInt64)? {
        guard activeWatchSessionID == sessionID else { return nil }
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        guard activeWatchSessionID == sessionID else { return nil }
        let result = try await pollWatchChangesUnlocked(dirtyOnly: dirtyOnly, cacheScope: cacheScope)
        guard activeWatchSessionID == sessionID else { return nil }
        return result.map { ($0, watchControlGeneration) }
    }

    private func stopWatching(sessionID: String) async {
        guard activeWatchSessionID == sessionID else { return }
        invalidateActiveWatchSession()
        await stopWatching()
    }

    private func invalidateActiveWatchSession() {
        let invalidator = activeWatchSessionInvalidator
        activeWatchSessionID = nil
        activeWatchSessionInvalidator = nil
        invalidator?()
    }

    // Generation validation and publication are one actor turn. A result read
    // before a pause/resume must not be published afterward as current content.
    private func publishWatchResult(
        _ value: ScriptMetaScanResult, generation: UInt64, streamID: String,
        watchedRootIDs: Set<String>, state previous: WatchPublicationState,
        continuation: AsyncThrowingStream<ScriptMetaKitWatchUpdate, Error>.Continuation
    ) throws -> WatchPublicationState {
        guard activeWatchSessionID == streamID, generation == watchControlGeneration else { return previous }
        let activeRootIDs = activeWatchedRootIDs(watchedRootIDs)
        var result = value
        result.roots.removeAll { !activeRootIDs.contains($0.rootID) }
        result.fileListSnapshots.removeAll { !activeRootIDs.contains($0.root.rootID) }
        guard !result.roots.isEmpty else { return previous }
        var state = previous
        guard state.sequence < UInt64.max else { throw ScriptMetaKitWorkspaceError.watchSequenceOverflow }
        state.sequence += 1
        var delivery = result.watchDelivery ?? ScriptMetaKitWatchDelivery(
            isReconciliation: false,
            coversAllWatchedRoots: activeRootIDs.isSubset(of: Set(result.roots.map(\.rootID))),
            streamID: nil, sequence: nil
        )
        delivery.streamID = streamID
        delivery.sequence = state.sequence
        result.watchDelivery = delivery
        if delivery.isReconciliation {
            if !state.reconciliationIsInProgress {
                state.reconciledRootIDs.removeAll(keepingCapacity: true)
                state.reconciliationIsInProgress = true
            }
            if delivery.coversAllWatchedRoots {
                state.reconciledRootIDs = activeRootIDs
                state.reconciliationIsInProgress = false
            } else {
                state.reconciledRootIDs.formUnion(result.roots.map(\.rootID))
            }
        }
        state.reconciledRootIDs.formIntersection(activeRootIDs)
        continuation.yield(ScriptMetaKitWatchUpdate(
            streamID: streamID, sequence: state.sequence,
            kind: delivery.isReconciliation ? .reconciliation : .incremental,
            coversAllWatchedRoots: delivery.coversAllWatchedRoots,
            reconciledRootIDs: state.reconciledRootIDs.sorted(),
            pendingRootIDs: activeRootIDs.subtracting(state.reconciledRootIDs).sorted(), result: result
        ))
        return state
    }

    private func activeWatchedRootIDs(_ rootIDs: Set<String>) -> Set<String> {
        rootIDs.subtracting(pausedWatchRootIDs)
    }

    func beforeNextWatchPublicationForTesting(_ hook: @escaping @Sendable () async -> Void) {
        beforeWatchPublicationForTesting = hook
    }

    func cancelNextWatchResumeForTesting() async throws {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try await engine.cancelNextWatchResumeForTesting()
    }

    private func takeWatchPublicationHookForTesting() -> (@Sendable () async -> Void)? {
        defer { beforeWatchPublicationForTesting = nil }
        return beforeWatchPublicationForTesting
    }

    public func pollWatchChanges(
        dirtyOnly: Bool = false,
        cacheScope: ScriptMetaCacheScope? = nil
    ) async throws -> ScriptMetaScanResult? {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        return try await pollWatchChangesUnlocked(dirtyOnly: dirtyOnly, cacheScope: cacheScope)
    }

    public func drainWatchChanges(
        dirtyOnly: Bool = false,
        initialDelayMillis: UInt64 = 300,
        pollingIntervalMillis: UInt64 = 150,
        idlePollCount: Int = 3,
        maxPollCount: Int = 20
    ) async throws {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await drainWatchChangesUnlocked(
            dirtyOnly: dirtyOnly,
            initialDelayMillis: initialDelayMillis,
            pollingIntervalMillis: pollingIntervalMillis,
            idlePollCount: idlePollCount,
            maxPollCount: maxPollCount
        )
    }

    private func drainWatchChangesUnlocked(
        dirtyOnly: Bool,
        initialDelayMillis: UInt64 = 300,
        pollingIntervalMillis: UInt64 = 150,
        idlePollCount: Int = 3,
        maxPollCount: Int = 20
    ) async throws {
        if initialDelayMillis > 0 {
            try await Task.sleep(for: .milliseconds(initialDelayMillis))
        }

        let requiredIdlePolls = max(1, idlePollCount)
        let allowedPolls = max(requiredIdlePolls, maxPollCount)
        var idlePolls = 0
        for _ in 0..<allowedPolls {
            if try await pollWatchChangesUnlocked(dirtyOnly: dirtyOnly, cacheScope: nil) == nil {
                idlePolls += 1
                if idlePolls >= requiredIdlePolls {
                    return
                }
            } else {
                idlePolls = 0
            }
            if pollingIntervalMillis > 0 {
                try await Task.sleep(for: .milliseconds(pollingIntervalMillis))
            }
        }
    }

    public func stopWatching(
        operationPolicy: ScriptMetaOperationTerminationPolicy = .waitForCurrentOperation
    ) async {
        invalidateActiveWatchSession()
        if operationPolicy == .cancelCurrentOperation {
            engine.cancelCurrentOrPendingOperationForTermination()
        }
        await enterExclusiveOperationIgnoringCancellation()
        defer { leaveExclusiveOperation() }
        await engine.stopWatching()
    }

    public func cancelCurrentOperation() async {
        engine.cancelCurrentOperation()
    }

    func failNextWatcherStartForTesting() async throws {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try await configureEngineIfNeeded()
        try await engine.failNextWatcherStartForTesting()
    }

    func cancelNextPersistentCacheLoadForTesting() async throws {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try await configureEngineIfNeeded()
        try await engine.cancelNextCacheLoadForTesting()
    }

    func schedulePersistentCacheSaveForTesting(scope: ScriptMetaCacheScope) {
        schedulePersistentCacheSave(scope: scope)
    }

    public func shutdown(
        operationPolicy: ScriptMetaOperationTerminationPolicy = .waitForCurrentOperation
    ) async {
        invalidateActiveWatchSession()
        if operationPolicy == .cancelCurrentOperation {
            engine.cancelCurrentOrPendingOperationForTermination()
        }
        await enterExclusiveOperationIgnoringCancellation()
        defer { leaveExclusiveOperation() }
        await flushPendingPersistentCacheSaves()
        await engine.shutdown()
        resetStateAfterEngineShutdown()
    }

    /// Clears roots, watchers, and in-memory scan/cache state while preserving persistent cache files.
    public func clearVolatileState(
        operationPolicy: ScriptMetaOperationTerminationPolicy = .waitForCurrentOperation
    ) async {
        await shutdown(operationPolicy: operationPolicy)
    }

    private func resetStateAfterEngineShutdown() {
        // EngineBox preserves the canonical runtime configuration across recreation.
        cacheSaveGenerationByScope.removeAll(keepingCapacity: false)
        persistentCacheLoadStates.removeAll(keepingCapacity: false)
        persistentFileListRootIDs.removeAll(keepingCapacity: false)
        registeredRootIDsByGroup.removeAll(keepingCapacity: false)
        pausedWatchRootIDs.removeAll(keepingCapacity: false)
        beforeWatchPublicationForTesting = nil
    }

    public func checkUpdate(
        item: ScriptMetaItem,
        cacheScope: ScriptMetaCacheScope? = nil,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> UpdateCheckResult {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        let result = try await engine.checkUpdate(item: item, onProgress: onProgress)
        if let cacheScope {
            await savePersistentCache(scope: cacheScope)
        }
        return result
    }

    public func checkUpdates(
        items: [ScriptMetaItem],
        cacheScope: ScriptMetaCacheScope? = nil,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> UpdateCheckResult {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        let result = try await engine.checkUpdates(items: items, onProgress: onProgress)
        if let cacheScope {
            await savePersistentCache(scope: cacheScope)
        }
        return result
    }

    private func configureEngineIfNeeded() async throws {
        guard didConfigureEngine == false else { return }
        try await engine.setPreferredLanguages(configuration.preferredLanguages)
        if let options = configuration.rootPreflightOptions {
            try await engine.setRootPreflightOptions(options)
        }
        if let policy = configuration.operationalPolicy {
            try await engine.setOperationalPolicy(policy)
        }
        try await engine.setResolveMacOSAlias(configuration.resolvesMacOSAliases)
        if let nativeEventLatencyMillis = configuration.nativeEventLatencyMillis {
            try await engine.setNativeEventLatencyMillis(nativeEventLatencyMillis)
        }
        var settings = try await engine.configuration()
        if configuration.cacheStore == nil {
            settings.cache.persistentCache = false
            try await engine.applyConfiguration(settings)
        }
        persistentCacheEnabled = settings.cache.enabled && settings.cache.persistentCache
        didConfigureEngine = true
    }

    private func replaceRoots(_ roots: [ScriptMetaKitRoot], groupID: String) async throws {
        try await engine.replaceRootGroup(roots, groupID: groupID)
        if roots.isEmpty {
            registeredRootIDsByGroup.removeValue(forKey: groupID)
        } else {
            registeredRootIDsByGroup[groupID] = Set(roots.map(\.rootID))
        }
        prunePersistentFileListProvenance()
    }

    private func mergeRoot(_ root: ScriptMetaKitRoot, groupID: String) async throws {
        try await mergeRoots([root], groupID: groupID)
    }

    private func mergeRoots(_ roots: [ScriptMetaKitRoot], groupID: String) async throws {
        try await engine.insertRootsIntoGroup(roots, groupID: groupID)
        registeredRootIDsByGroup[groupID, default: []].formUnion(roots.map(\.rootID))
        prunePersistentFileListProvenance()
    }

    private func prunePersistentFileListProvenance() {
        let registeredRootIDs = registeredRootIDsByGroup.values.reduce(into: Set<String>()) {
            $0.formUnion($1)
        }
        persistentFileListRootIDs.formIntersection(registeredRootIDs)
        pausedWatchRootIDs.formIntersection(registeredRootIDs)
    }

    private func loadPersistentCacheIfNeeded(
        scope: ScriptMetaCacheScope,
        rootIDs: [String]
    ) async throws {
        guard persistentCacheEnabled, let cacheStore = configuration.cacheStore else { return }
        let canonicalScope = canonicalCacheScope(scope)
        let cacheStateKey = canonicalScope.rawValue
        guard let maximumCacheBytes = cacheStore.validatedMaximumCacheBytes else {
            emitDiagnostic(
                severity: .warning,
                code: "cache_limit_invalid",
                message: "maximumCacheBytes must be between 1 and \(ScriptMetaKitEngine.maximumCacheFileBytes)",
                path: cacheStore.directoryURL.path
            )
            return
        }
        let requestedRootIDs = Set(rootIDs)
        var registeredRootIDs = registeredRootIDsByGroup.values.reduce(into: Set<String>()) {
            $0.formUnion($1)
        }
        registeredRootIDs.formUnion(requestedRootIDs)
        let probedRootIDs = registeredRootIDs.sorted()
        guard let url = cacheStore.readableCacheFileURL(scope: scope) else { return }
        guard let cacheSignature = persistentCacheFileSignature(for: url) else { return }
        let probeMode: ScriptMetaScanMode = switch canonicalScope {
        case .catalog: .metadataOnly
        case .all: .fileListAndMetadata
        case .fileList, .root: .fileListOnly
        }
        let resident = try await engine.cacheRevisions(rootIDs: probedRootIDs, mode: probeMode)
        let residentFileListRevisions = fileListRevisions(in: resident)
        if canonicalScope == .fileList,
           Set(residentFileListRevisions.keys).isSuperset(of: requestedRootIDs) {
            return
        }
        let residentRootRevisions = rootRevisions(in: resident)
        if let loadState = persistentCacheLoadStates[cacheStateKey],
           loadState.signature == cacheSignature,
           requestedRootIDs.allSatisfy({ rootID in
               guard let revision = residentRootRevisions[rootID] else { return false }
               return loadState.rootRevisions[rootID] == revision
           }) {
            return
        }
        do {
            let loaded = try await engine.loadCacheRevisions(
                from: url, maximumBytes: maximumCacheBytes, rootIDs: probedRootIDs, mode: probeMode
            )
            persistentCacheLoadStates[cacheStateKey] = PersistentCacheLoadState(
                signature: cacheSignature,
                rootRevisions: rootRevisions(in: loaded)
            )
            if canonicalScope == .fileList || canonicalScope == .all {
                let loadedFileListRevisions = fileListRevisions(in: loaded)
                let adoptedRootIDs = loadedFileListRevisions.compactMap { rootID, revision in
                    residentFileListRevisions[rootID] != revision ? rootID : nil
                }
                persistentFileListRootIDs.formUnion(adoptedRootIDs)
            }
        } catch let error as CancellationError {
            throw error
        } catch ScriptMetaKitError.operationFailed(let status, _) where status == smkStatusCacheIncompatible {
            // Upgrades and language/read-setting changes are normal cache misses.
            cacheStore.remove(scope: scope)
            persistentCacheLoadStates[cacheStateKey] = nil
        } catch {
            emitDiagnostic(
                severity: .warning,
                code: "cache_load_failed",
                message: String(describing: error),
                path: url.path
            )
            cacheStore.remove(scope: scope)
            persistentCacheLoadStates[cacheStateKey] = nil
        }
    }

    private func savePersistentCache(scope: ScriptMetaCacheScope) async {
        guard persistentCacheEnabled, let cacheStore = configuration.cacheStore else { return }
        let scope = canonicalCacheScope(scope)
        guard let maximumCacheBytes = cacheStore.validatedMaximumCacheBytes else {
            emitDiagnostic(
                severity: .warning,
                code: "cache_limit_invalid",
                message: "maximumCacheBytes must be between 1 and \(ScriptMetaKitEngine.maximumCacheFileBytes)",
                path: cacheStore.directoryURL.path
            )
            return
        }
        do {
            try await engine.saveCache(
                to: cacheStore.writableCacheFileURL(scope: scope),
                scope: scope,
                maximumBytes: maximumCacheBytes
            )
            persistentCacheLoadStates[scope.rawValue] = nil
        } catch {
            emitDiagnostic(
                severity: .warning,
                code: "cache_save_failed",
                message: String(describing: error),
                path: cacheStore.writableCacheFileURL(scope: scope).path
            )
            return
        }
    }

    private func emitDiagnostic(
        severity: ScriptMetaKitDiagnosticSeverity,
        code: String,
        message: String,
        path: String? = nil
    ) {
        configuration.diagnosticHandler?(ScriptMetaKitDiagnostic(
            severity: severity,
            code: code,
            message: message,
            path: path
        ))
    }

    private func persistentCacheFileSignature(
        for url: URL
    ) -> PersistentCacheFileSignature? {
        guard let values = try? url.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey]),
              let fileSize = values.fileSize else {
            return nil
        }
        return PersistentCacheFileSignature(
            fileSize: fileSize,
            modificationDate: values.contentModificationDate
        )
    }

    private func rootRevisions(
        in result: [ScriptMetaCacheRevision]
    ) -> [String: ScriptMetaKitRevision] {
        Dictionary(uniqueKeysWithValues: result.map { ($0.rootID, $0.stateRevision) })
    }

    private func fileListRevisions(
        in result: [ScriptMetaCacheRevision]
    ) -> [String: ScriptMetaKitRevision] {
        Dictionary(uniqueKeysWithValues: result.compactMap { revision in
            revision.fileListRevision.map { (revision.rootID, $0) }
        })
    }

    private func schedulePersistentCacheSave(scope: ScriptMetaCacheScope) {
        guard persistentCacheEnabled, configuration.cacheStore != nil else { return }
        let scope = canonicalCacheScope(scope)
        let scopeKey = scope.rawValue
        let generation = cacheSaveGenerationByScope[scopeKey, default: 0] &+ 1
        cacheSaveGenerationByScope[scopeKey] = generation
        pendingCacheSaveTasks[scopeKey]?.cancel()
        let delay = configuration.cacheSaveDebounceMillis
        pendingCacheSaveTasks[scopeKey] = Task { [weak self] in
            if delay > 0 {
                try? await Task.sleep(for: .milliseconds(delay))
            }
            guard Task.isCancelled == false else { return }
            await self?.saveScheduledPersistentCache(
                scope: scope,
                scopeKey: scopeKey,
                generation: generation
            )
        }
    }

    private func saveScheduledPersistentCache(
        scope: ScriptMetaCacheScope,
        scopeKey: UInt32,
        generation: UInt64
    ) async {
        guard cacheSaveGenerationByScope[scopeKey] == generation else { return }
        do {
            try await enterExclusiveOperation()
        } catch {
            return
        }
        defer { leaveExclusiveOperation() }
        guard Task.isCancelled == false,
              cacheSaveGenerationByScope[scopeKey] == generation else {
            return
        }
        pendingCacheSaveTasks[scopeKey] = nil
        await savePersistentCache(scope: scope)
    }

    private func flushPendingPersistentCacheSaves() async {
        let scopes = pendingCacheSaveTasks.keys.compactMap(ScriptMetaCacheScope.init(rawValue:))
        for scope in scopes {
            let scopeKey = scope.rawValue
            cacheSaveGenerationByScope[scopeKey, default: 0] &+= 1
        }
        for task in pendingCacheSaveTasks.values {
            task.cancel()
        }
        pendingCacheSaveTasks.removeAll(keepingCapacity: false)
        for scope in scopes {
            await savePersistentCache(scope: scope)
        }
    }

    private func pollWatchChangesUnlocked(
        dirtyOnly: Bool,
        cacheScope: ScriptMetaCacheScope?
    ) async throws -> ScriptMetaScanResult? {
        try await configureEngineIfNeeded()
        let result = try await engine.pollWatchChanges(dirtyOnly: dirtyOnly)
        if let result {
            persistentFileListRootIDs.subtract(
                result.roots.filter { $0.status == "ready" && !$0.isDirty }.map(\.rootID)
            )
        }
        if result != nil, let cacheScope {
            schedulePersistentCacheSave(scope: cacheScope)
        }
        return result
    }

    private func enterExclusiveOperation() async throws {
        try Task.checkCancellation()
        if hasExclusiveOperation == false {
            hasExclusiveOperation = true
            return
        }

        let waiterID = nextExclusiveOperationWaiterID
        nextExclusiveOperationWaiterID &+= 1
        let acquired = await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                exclusiveOperationWaiters.append(ExclusiveOperationWaiter(
                    id: waiterID,
                    continuation: continuation
                ))
            }
        } onCancel: {
            Task { await self.cancelExclusiveOperationWaiter(waiterID) }
        }
        guard acquired else {
            throw CancellationError()
        }
        if Task.isCancelled {
            leaveExclusiveOperation()
            throw CancellationError()
        }
    }

    private func enterExclusiveOperationIgnoringCancellation() async {
        if hasExclusiveOperation == false {
            hasExclusiveOperation = true
            return
        }

        let waiterID = nextExclusiveOperationWaiterID
        nextExclusiveOperationWaiterID &+= 1
        _ = await withCheckedContinuation { continuation in
            exclusiveOperationWaiters.append(ExclusiveOperationWaiter(
                id: waiterID,
                continuation: continuation
            ))
        }
    }

    private func cancelExclusiveOperationWaiter(_ waiterID: UInt64) {
        guard let index = exclusiveOperationWaiters.firstIndex(where: { $0.id == waiterID }) else {
            return
        }
        let waiter = exclusiveOperationWaiters.remove(at: index)
        waiter.continuation.resume(returning: false)
    }

    private func leaveExclusiveOperation() {
        if exclusiveOperationWaiters.isEmpty {
            hasExclusiveOperation = false
        } else {
            exclusiveOperationWaiters.removeFirst().continuation.resume(returning: true)
        }
    }
}

public nonisolated enum ScriptMetaKitWorkspaceError: LocalizedError, Sendable {
    case missingCatalogSnapshot
    case unknownRootID(String)
    case watchSequenceOverflow
    case missingWatchDeliverySequence

    public var errorDescription: String? {
        switch self {
        case .missingCatalogSnapshot:
            "SCRIPTMETAKit did not return a catalog snapshot."
        case .unknownRootID(let rootID):
            "Unknown SCRIPTMETAKit root ID: \(rootID)"
        case .watchSequenceOverflow:
            "SCRIPTMETAKit watch delivery sequence overflowed."
        case .missingWatchDeliverySequence:
            "SCRIPTMETAKit watch result did not contain a delivery sequence."
        }
    }
}

public extension ScriptMetaKitWorkspace {
    func scanCatalog(
        roots: [ScriptMetaKitRoot],
        replacingGroup groupID: String,
        rootIDs: [String],
        checkUpdates: Bool = false,
        cacheScope: ScriptMetaCacheScope? = .catalog,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> ScriptMetaCatalogSnapshot {
        let result = try await scanRoots(
            roots,
            replacingGroup: groupID,
            rootIDs: rootIDs,
            mode: .metadataOnly,
            checkUpdates: checkUpdates,
            cacheScope: cacheScope,
            onProgress: onProgress
        )
        guard let catalogSnapshot = result.catalogSnapshot else {
            throw ScriptMetaKitWorkspaceError.missingCatalogSnapshot
        }
        return catalogSnapshot
    }

    func preflightRoot(_ root: ScriptMetaKitRoot) async throws -> ScriptMetaScanResult {
        try await enterExclusiveOperation()
        defer { leaveExclusiveOperation() }
        try await configureEngineIfNeeded()
        return try await engine.preflightRoot(root)
    }

    nonisolated func canReadDirectoryContents(_ url: URL) -> Bool {
        var canRead: UInt8 = 0
        let status = withWorkspaceUTF8Slice(url.standardizedFileURL.path) { pathSlice in
            smk_can_read_directory_contents(pathSlice, &canRead)
        }
        return status == smkWorkspaceStatusOK && canRead != 0
    }
}

private nonisolated func withWorkspaceUTF8Slice<Result>(
    _ value: String,
    _ body: (SmkUtf8Slice) -> Result
) -> Result {
    var mutableValue = value
    return mutableValue.withUTF8 { buffer in
        body(SmkUtf8Slice(ptr: buffer.baseAddress, len: buffer.count))
    }
}

private nonisolated func canonicalCacheScope(
    _ scope: ScriptMetaCacheScope
) -> ScriptMetaCacheScope {
    scope == .root ? .fileList : scope
}

public extension ScriptMetaScanResult {
    var scriptFileEntries: [FileSystemEntry] {
        fileListSnapshots.flatMap(\.scriptFileEntries)
    }
}

public extension FileListSnapshot {
    var scriptFileEntries: [FileSystemEntry] {
        children?.flatMap(\.scriptFileEntries) ?? []
    }
}

public extension FileSystemEntry {
    var isScriptFile: Bool {
        !isDirectory && runtimeKind != nil
    }

    var scriptFileEntries: [FileSystemEntry] {
        if isScriptFile {
            return [self]
        }
        guard children.isEmpty == false else { return [] }
        return children.flatMap(\.scriptFileEntries)
    }
}
