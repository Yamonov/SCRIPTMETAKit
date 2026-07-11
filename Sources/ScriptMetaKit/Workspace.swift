import Foundation

private nonisolated let smkWorkspaceStatusOK: Int32 = 0

private nonisolated struct SmkWorkspaceUtf8Slice {
    var ptr: UnsafePointer<UInt8>?
    var len: Int

    init(ptr: UnsafePointer<UInt8>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

@_silgen_name("smk_can_read_directory_contents")
private nonisolated func smk_can_read_directory_contents(
    _ path: SmkWorkspaceUtf8Slice,
    _ outCanRead: UnsafeMutablePointer<UInt8>
) -> Int32

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
        guard isReadableCacheFile(url) else { return nil }
        return url
    }

    public func writableCacheFileURL(scope: ScriptMetaCacheScope) -> URL {
        cacheFileURL(scope: scope)
    }

    public func enforceSizeLimit(scope: ScriptMetaCacheScope) {
        let url = cacheFileURL(scope: scope)
        guard isReadableCacheFile(url) == false else { return }
        remove(scope: scope)
    }

    public func remove(scope: ScriptMetaCacheScope) {
        try? FileManager.default.removeItem(at: cacheFileURL(scope: scope))
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
            "ScriptMetaKitRootCache.cache"
        }
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
        diagnosticHandler: (@Sendable (ScriptMetaKitDiagnostic) -> Void)? = nil
    ) {
        self.cacheStore = cacheStore
        self.rootPreflightOptions = rootPreflightOptions
        self.resolvesMacOSAliases = resolvesMacOSAliases
        self.nativeEventLatencyMillis = nativeEventLatencyMillis
        self.cacheSaveDebounceMillis = cacheSaveDebounceMillis
        self.operationalPolicy = operationalPolicy
        self.diagnosticHandler = diagnosticHandler
    }
}

public actor ScriptMetaKitWorkspace {
    private struct ExclusiveOperationWaiter {
        let id: UInt64
        let continuation: CheckedContinuation<Bool, Never>
    }

    private let engine = ScriptMetaKitEngine()
    private let configuration: ScriptMetaKitWorkspaceConfiguration
    private var didConfigureEngine = false
    private var loadedCacheRootIDsByScope: [UInt32: Set<String>] = [:]
    private var hasExclusiveOperation = false
    private var nextExclusiveOperationWaiterID: UInt64 = 0
    private var exclusiveOperationWaiters: [ExclusiveOperationWaiter] = []
    private var cacheSaveGenerationByScope: [UInt32: UInt64] = [:]
    private var pendingCacheSaveTasks: [UInt32: Task<Void, Never>] = [:]

    public init(configuration: ScriptMetaKitWorkspaceConfiguration = ScriptMetaKitWorkspaceConfiguration()) {
        self.configuration = configuration
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
            await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: rootIDs)
        }
        let result = try await engine.scanRoots(
            rootIDs: rootIDs,
            mode: mode,
            checkUpdates: checkUpdates,
            onProgress: onProgress
        )
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
            await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: [root.rootID])
        }
        let result = try await engine.scanRoot(
            rootID: root.rootID,
            mode: mode,
            checkUpdates: checkUpdates,
            onProgress: onProgress
        )
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
        try await engine.insertRootsIntoGroup(roots, groupID: groupID)
        let rootIDs = roots.map(\.rootID)
        if let cacheScope {
            await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: rootIDs)
        }
        let result = try await engine.scanRoots(
            rootIDs: rootIDs,
            mode: mode,
            checkUpdates: checkUpdates,
            onProgress: onProgress
        )
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
            await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: resultRootIDs)
        }
        let scannedResult = try await engine.scanRoots(
            rootIDs: scanningRootIDs,
            mode: mode,
            checkUpdates: checkUpdates,
            onProgress: onProgress
        )
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
            await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: rootIDs)
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
            await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: [root.rootID])
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
            await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: rootIDs)
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
            await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: roots.map(\.rootID))
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
        try Task.checkCancellation()
        try await configureEngineIfNeeded()
        try await replaceRoots(roots, groupID: groupID)
        if let cacheScope {
            await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: roots.map(\.rootID))
        }
        try await engine.startWatching(onChange: onChange)
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
        let (notifications, notificationContinuation) = AsyncStream<Void>.makeStream(
            bufferingPolicy: .bufferingNewest(1)
        )
        let (changes, changeContinuation) = AsyncThrowingStream<ScriptMetaScanResult, Error>.makeStream(
            bufferingPolicy: .bufferingNewest(1)
        )
        let pump = Task { [weak self] in
            for await _ in notifications {
                guard let self else { break }
                do {
                    if let result = try await self.pollWatchChanges(
                        dirtyOnly: dirtyOnly,
                        cacheScope: cacheScope
                    ) {
                        changeContinuation.yield(result)
                    }
                } catch is CancellationError {
                    break
                } catch {
                    changeContinuation.finish(throwing: error)
                    return
                }
            }
            changeContinuation.finish()
        }
        changeContinuation.onTermination = { [weak self] _ in
            pump.cancel()
            notificationContinuation.finish()
            Task { await self?.stopWatching() }
        }

        do {
            try await startWatching(
                roots: roots,
                replacingGroup: groupID,
                cacheScope: cacheScope,
                drainsInitialChanges: false,
                onChange: { notificationContinuation.yield() }
            )
        } catch {
            pump.cancel()
            notificationContinuation.finish()
            changeContinuation.finish(throwing: error)
            throw error
        }
        return ScriptMetaKitWatchSequence(stream: changes)
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
            try await Task.sleep(nanoseconds: initialDelayMillis * 1_000_000)
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
                try await Task.sleep(nanoseconds: pollingIntervalMillis * 1_000_000)
            }
        }
    }

    public func stopWatching(
        operationPolicy: ScriptMetaOperationTerminationPolicy = .waitForCurrentOperation
    ) async {
        if operationPolicy == .cancelCurrentOperation {
            engine.cancelCurrentOrPendingOperationForTermination()
        }
        do {
            try await enterExclusiveOperation()
        } catch {
            return
        }
        defer { leaveExclusiveOperation() }
        await engine.stopWatching()
    }

    public func cancelCurrentOperation() async {
        engine.cancelCurrentOperation()
    }

    public func shutdown(
        operationPolicy: ScriptMetaOperationTerminationPolicy = .waitForCurrentOperation
    ) async {
        if operationPolicy == .cancelCurrentOperation {
            engine.cancelCurrentOrPendingOperationForTermination()
        }
        do {
            try await enterExclusiveOperation()
        } catch {
            return
        }
        defer { leaveExclusiveOperation() }
        await flushPendingPersistentCacheSaves()
        await engine.shutdown()
    }

    /// Clears roots, watchers, and in-memory scan/cache state while preserving persistent cache files.
    public func clearVolatileState(
        operationPolicy: ScriptMetaOperationTerminationPolicy = .waitForCurrentOperation
    ) async {
        if operationPolicy == .cancelCurrentOperation {
            engine.cancelCurrentOrPendingOperationForTermination()
        }
        do {
            try await enterExclusiveOperation()
        } catch {
            return
        }
        defer { leaveExclusiveOperation() }
        await flushPendingPersistentCacheSaves()
        await engine.shutdown()
        didConfigureEngine = false
        loadedCacheRootIDsByScope.removeAll(keepingCapacity: false)
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
        didConfigureEngine = true
    }

    private func replaceRoots(_ roots: [ScriptMetaKitRoot], groupID: String) async throws {
        try await engine.replaceRootGroup(roots, groupID: groupID)
    }

    private func mergeRoot(_ root: ScriptMetaKitRoot, groupID: String) async throws {
        try await engine.insertRootsIntoGroup([root], groupID: groupID)
    }

    private func loadPersistentCacheIfNeeded(scope: ScriptMetaCacheScope, rootIDs: [String]) async {
        guard let cacheStore = configuration.cacheStore else { return }
        let requestedRootIDs = Set(rootIDs)
        let scopeKey = scope.rawValue
        if loadedCacheRootIDsByScope[scopeKey, default: []].isSuperset(of: requestedRootIDs) {
            return
        }
        loadedCacheRootIDsByScope[scopeKey, default: []].formUnion(requestedRootIDs)
        guard let url = cacheStore.readableCacheFileURL(scope: scope) else { return }
        do {
            try await engine.loadCache(from: url)
        } catch {
            emitDiagnostic(
                severity: .warning,
                code: "cache_load_failed",
                message: String(describing: error),
                path: url.path
            )
            cacheStore.remove(scope: scope)
        }
    }

    private func savePersistentCache(scope: ScriptMetaCacheScope) async {
        guard let cacheStore = configuration.cacheStore else { return }
        do {
            try await engine.saveCache(to: cacheStore.writableCacheFileURL(scope: scope), scope: scope)
            cacheStore.enforceSizeLimit(scope: scope)
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

    private func schedulePersistentCacheSave(scope: ScriptMetaCacheScope) {
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
        pendingCacheSaveTasks[scopeKey] = nil
        do {
            try await enterExclusiveOperation()
        } catch {
            return
        }
        defer { leaveExclusiveOperation() }
        await savePersistentCache(scope: scope)
    }

    private func flushPendingPersistentCacheSaves() async {
        let scopes = pendingCacheSaveTasks.keys.compactMap(ScriptMetaCacheScope.init(rawValue:))
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

public nonisolated enum ScriptMetaKitWorkspaceError: LocalizedError {
    case missingCatalogSnapshot

    public var errorDescription: String? {
        switch self {
        case .missingCatalogSnapshot:
            "SCRIPTMETAKit did not return a catalog snapshot."
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

    func preflightRoot(
        _ root: ScriptMetaKitRoot,
        mode: ScriptMetaScanMode = .fileListOnly
    ) async throws -> ScriptMetaScanResult {
        try await scanRoots(
            [root],
            replacingGroup: "scriptmetakit.preflight",
            rootIDs: [root.rootID],
            mode: mode,
            checkUpdates: false,
            cacheScope: nil
        )
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
    _ body: (SmkWorkspaceUtf8Slice) -> Result
) -> Result {
    var mutableValue = value
    return mutableValue.withUTF8 { buffer in
        body(SmkWorkspaceUtf8Slice(ptr: buffer.baseAddress, len: buffer.count))
    }
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
