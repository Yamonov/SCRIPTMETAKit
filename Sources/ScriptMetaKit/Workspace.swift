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

public nonisolated struct ScriptMetaKitPersistentCacheStore: Sendable {
    public var directoryURL: URL
    public var maximumCacheBytes: Int

    public init(directoryURL: URL, maximumCacheBytes: Int = 80 * 1024 * 1024) {
        self.directoryURL = directoryURL
        self.maximumCacheBytes = maximumCacheBytes
    }

    public static func applicationSupport(
        bundleIdentifier: String,
        maximumCacheBytes: Int = 80 * 1024 * 1024,
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

    public init(
        cacheStore: ScriptMetaKitPersistentCacheStore? = nil,
        rootPreflightOptions: ScriptMetaRootPreflightOptions? = nil,
        resolvesMacOSAliases: Bool = true,
        nativeEventLatencyMillis: UInt64? = nil
    ) {
        self.cacheStore = cacheStore
        self.rootPreflightOptions = rootPreflightOptions
        self.resolvesMacOSAliases = resolvesMacOSAliases
        self.nativeEventLatencyMillis = nativeEventLatencyMillis
    }
}

public actor ScriptMetaKitWorkspace {
    private let engine = ScriptMetaKitEngine()
    private let configuration: ScriptMetaKitWorkspaceConfiguration
    private var didConfigureEngine = false
    private var loadedCacheRootIDsByScope: [UInt32: Set<String>] = [:]

    public init(configuration: ScriptMetaKitWorkspaceConfiguration = ScriptMetaKitWorkspaceConfiguration()) {
        self.configuration = configuration
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

    public func cachedScanResult(
        rootIDs: [String],
        mode: ScriptMetaScanMode,
        cacheScope: ScriptMetaCacheScope? = nil
    ) async throws -> ScriptMetaScanResult {
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
        try await configureEngineIfNeeded()
        try await replaceRoots(roots, groupID: groupID)
        if let cacheScope {
            await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: roots.map(\.rootID))
        }
    }

    public func setVisibleRoot(_ rootID: String?) async throws {
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
        try await configureEngineIfNeeded()
        try await replaceRoots(roots, groupID: groupID)
        if let cacheScope {
            await loadPersistentCacheIfNeeded(scope: cacheScope, rootIDs: roots.map(\.rootID))
        }
        try await engine.startWatching(onChange: onChange)
        if drainsInitialChanges {
            try await drainWatchChanges(dirtyOnly: initialDrainDirtyOnly)
        }
    }

    public func pollWatchChanges(
        dirtyOnly: Bool = false,
        cacheScope: ScriptMetaCacheScope? = nil
    ) async throws -> ScriptMetaScanResult? {
        try await configureEngineIfNeeded()
        let result = try await engine.pollWatchChanges(dirtyOnly: dirtyOnly)
        if result != nil, let cacheScope {
            await savePersistentCache(scope: cacheScope)
        }
        return result
    }

    public func drainWatchChanges(
        dirtyOnly: Bool = false,
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
            if try await pollWatchChanges(dirtyOnly: dirtyOnly) == nil {
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

    public func stopWatching() async {
        await engine.stopWatching()
    }

    public func cancelCurrentOperation() async {
        engine.cancelCurrentOperation()
    }

    public func shutdown() async {
        await engine.shutdown()
    }

    public func checkUpdate(
        item: ScriptMetaItem,
        cacheScope: ScriptMetaCacheScope? = nil,
        onProgress: (@Sendable (UpdateCheckProgress) -> Void)? = nil
    ) async throws -> UpdateCheckResult {
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
            cacheStore.remove(scope: scope)
        }
    }

    private func savePersistentCache(scope: ScriptMetaCacheScope) async {
        guard let cacheStore = configuration.cacheStore else { return }
        do {
            try await engine.saveCache(to: cacheStore.writableCacheFileURL(scope: scope), scope: scope)
            cacheStore.enforceSizeLimit(scope: scope)
        } catch {
            return
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
