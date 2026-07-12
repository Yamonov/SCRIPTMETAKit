import Foundation

private nonisolated let smkPolicyStatusOK: Int32 = 0

private nonisolated struct SmkPolicyUtf8Slice {
    var ptr: UnsafePointer<UInt8>?
    var len: Int

    init(ptr: UnsafePointer<UInt8>? = nil, len: Int = 0) {
        self.ptr = ptr
        self.len = len
    }
}

private nonisolated struct SmkPolicyScriptFileInspection {
    var isSupportedScriptPath: UInt8
    var runtimeKind: SmkPolicyUtf8Slice
    var shebang: SmkPolicyUtf8Slice
    var commentSyntax: SmkPolicyUtf8Slice
    var supportsInlineScriptMetaEditing: UInt8
    var isFileLocked: UInt8
    var isReadOnly: UInt8
    var canEditScriptMeta: UInt8
    var canAppendScriptMeta: UInt8
    var scriptMetaEditState: SmkPolicyUtf8Slice

    init(
        isSupportedScriptPath: UInt8 = 0,
        runtimeKind: SmkPolicyUtf8Slice = SmkPolicyUtf8Slice(),
        shebang: SmkPolicyUtf8Slice = SmkPolicyUtf8Slice(),
        commentSyntax: SmkPolicyUtf8Slice = SmkPolicyUtf8Slice(),
        supportsInlineScriptMetaEditing: UInt8 = 0,
        isFileLocked: UInt8 = 0,
        isReadOnly: UInt8 = 0,
        canEditScriptMeta: UInt8 = 0,
        canAppendScriptMeta: UInt8 = 0,
        scriptMetaEditState: SmkPolicyUtf8Slice = SmkPolicyUtf8Slice()
    ) {
        self.isSupportedScriptPath = isSupportedScriptPath
        self.runtimeKind = runtimeKind
        self.shebang = shebang
        self.commentSyntax = commentSyntax
        self.supportsInlineScriptMetaEditing = supportsInlineScriptMetaEditing
        self.isFileLocked = isFileLocked
        self.isReadOnly = isReadOnly
        self.canEditScriptMeta = canEditScriptMeta
        self.canAppendScriptMeta = canAppendScriptMeta
        self.scriptMetaEditState = scriptMetaEditState
    }
}

private nonisolated struct SmkPolicyPathResolution {
    var displayPath = SmkPolicyUtf8Slice()
    var sourcePath = SmkPolicyUtf8Slice()
    var resolvedPath = SmkPolicyUtf8Slice()
    var pathKind = SmkPolicyUtf8Slice()
    var resolutionStatus = SmkPolicyUtf8Slice()
    var resolutionMessage = SmkPolicyUtf8Slice()
}

@_silgen_name("smk_supported_script_extensions")
private nonisolated func smk_supported_script_extensions(
    _ outExtensions: UnsafeMutablePointer<SmkPolicyUtf8Slice>
) -> Int32

@_silgen_name("smk_inspect_script_file_path")
private nonisolated func smk_inspect_script_file_path(
    _ path: SmkPolicyUtf8Slice,
    _ outInspection: UnsafeMutablePointer<SmkPolicyScriptFileInspection>
) -> Int32

@_silgen_name("smk_resolve_registered_path")
private nonisolated func smk_resolve_registered_path(
    _ path: SmkPolicyUtf8Slice,
    _ followSymlinks: UInt8,
    _ resolveMacOSAlias: UInt8,
    _ outResolution: UnsafeMutablePointer<SmkPolicyPathResolution>
) -> Int32

@_silgen_name("smk_script_path_may_affect_metadata")
private nonisolated func smk_script_path_may_affect_metadata(
    _ path: SmkPolicyUtf8Slice,
    _ outMayAffect: UnsafeMutablePointer<UInt8>
) -> Int32

public nonisolated struct ScriptMetaScriptFileInspection: Codable, Equatable, Sendable {
    public var isSupportedScriptPath: Bool
    public var runtimeKind: String?
    public var shebang: String?
    public var commentSyntax: String?
    public var supportsInlineScriptMetaEditing: Bool
    public var isFileLocked: Bool
    public var isReadOnly: Bool
    public var canEditScriptMeta: Bool
    public var canAppendScriptMeta: Bool
    public var scriptMetaEditState: String

    public init(
        isSupportedScriptPath: Bool = false,
        runtimeKind: String? = nil,
        shebang: String? = nil,
        commentSyntax: String? = nil,
        supportsInlineScriptMetaEditing: Bool = false,
        isFileLocked: Bool = false,
        isReadOnly: Bool = false,
        canEditScriptMeta: Bool = false,
        canAppendScriptMeta: Bool = false,
        scriptMetaEditState: String = "unknown"
    ) {
        self.isSupportedScriptPath = isSupportedScriptPath
        self.runtimeKind = runtimeKind
        self.shebang = shebang
        self.commentSyntax = commentSyntax
        self.supportsInlineScriptMetaEditing = supportsInlineScriptMetaEditing
        self.isFileLocked = isFileLocked
        self.isReadOnly = isReadOnly
        self.canEditScriptMeta = canEditScriptMeta
        self.canAppendScriptMeta = canAppendScriptMeta
        self.scriptMetaEditState = scriptMetaEditState
    }
}

public nonisolated struct ScriptMetaPathResolution: Codable, Equatable, Sendable {
    public var displayURL: URL
    public var sourceURL: URL
    public var resolvedURL: URL
    public var pathKind: String
    public var resolutionStatus: String
    public var resolutionMessage: String?

    public init(
        displayURL: URL,
        sourceURL: URL,
        resolvedURL: URL,
        pathKind: String,
        resolutionStatus: String,
        resolutionMessage: String? = nil
    ) {
        self.displayURL = displayURL
        self.sourceURL = sourceURL
        self.resolvedURL = resolvedURL
        self.pathKind = pathKind
        self.resolutionStatus = resolutionStatus
        self.resolutionMessage = resolutionMessage
    }
}

public nonisolated enum ScriptMetaScriptFilePolicy {
    public static let supportedPathExtensions: Set<String> = {
        var extensions = SmkPolicyUtf8Slice()
        guard smk_supported_script_extensions(&extensions) == smkPolicyStatusOK else {
            return []
        }
        return Set(string(extensions).split(separator: "\n").map(String.init))
    }()

    public static func inspect(_ url: URL) -> ScriptMetaScriptFileInspection {
        var inspection = SmkPolicyScriptFileInspection()
        let status = withUTF8Slice(url.standardizedFileURL.path) { pathSlice in
            smk_inspect_script_file_path(pathSlice, &inspection)
        }
        guard status == smkPolicyStatusOK else {
            return ScriptMetaScriptFileInspection()
        }
        return ScriptMetaScriptFileInspection(
            isSupportedScriptPath: inspection.isSupportedScriptPath != 0,
            runtimeKind: optionalString(inspection.runtimeKind),
            shebang: optionalString(inspection.shebang),
            commentSyntax: optionalString(inspection.commentSyntax),
            supportsInlineScriptMetaEditing: inspection.supportsInlineScriptMetaEditing != 0,
            isFileLocked: inspection.isFileLocked != 0,
            isReadOnly: inspection.isReadOnly != 0,
            canEditScriptMeta: inspection.canEditScriptMeta != 0,
            canAppendScriptMeta: inspection.canAppendScriptMeta != 0,
            scriptMetaEditState: optionalString(inspection.scriptMetaEditState) ?? "unknown"
        )
    }

    public static func resolveRegisteredPath(
        _ url: URL,
        followSymlinks: Bool = true,
        resolveMacOSAlias: Bool = true
    ) -> ScriptMetaPathResolution? {
        var resolution = SmkPolicyPathResolution()
        let status = withUTF8Slice(url.standardizedFileURL.path) { pathSlice in
            smk_resolve_registered_path(
                pathSlice,
                followSymlinks ? 1 : 0,
                resolveMacOSAlias ? 1 : 0,
                &resolution
            )
        }
        guard status == smkPolicyStatusOK else { return nil }
        return ScriptMetaPathResolution(
            displayURL: URL(fileURLWithPath: string(resolution.displayPath)),
            sourceURL: URL(fileURLWithPath: string(resolution.sourcePath)),
            resolvedURL: URL(fileURLWithPath: string(resolution.resolvedPath)),
            pathKind: string(resolution.pathKind),
            resolutionStatus: string(resolution.resolutionStatus),
            resolutionMessage: optionalString(resolution.resolutionMessage)
        )
    }

    public static func isSupportedScriptPath(_ url: URL) -> Bool {
        inspect(url).isSupportedScriptPath
    }

    public static func runtimeKind(for url: URL) -> String? {
        inspect(url).runtimeKind
    }

    public static func supportsInlineScriptMetaEditing(_ url: URL) -> Bool {
        inspect(url).supportsInlineScriptMetaEditing
    }

    public static func scriptMetaEditState(for url: URL) -> String {
        inspect(url).scriptMetaEditState
    }

    public static func commentStyle(for url: URL) -> String? {
        inspect(url).commentSyntax
    }

    public static func changedPathMayAffectMetadata(_ changedPath: String) -> Bool {
        var mayAffect: UInt8 = 0
        let status = withUTF8Slice(changedPath) { pathSlice in
            smk_script_path_may_affect_metadata(pathSlice, &mayAffect)
        }
        return status == smkPolicyStatusOK && mayAffect != 0
    }

    private static func withUTF8Slice<Result>(
        _ value: String,
        _ body: (SmkPolicyUtf8Slice) -> Result
    ) -> Result {
        var mutableValue = value
        return mutableValue.withUTF8 { buffer in
            body(SmkPolicyUtf8Slice(ptr: buffer.baseAddress, len: buffer.count))
        }
    }

    private static func optionalString(_ slice: SmkPolicyUtf8Slice) -> String? {
        let value = string(slice)
        return value.isEmpty ? nil : value
    }

    private static func string(_ slice: SmkPolicyUtf8Slice) -> String {
        guard let ptr = slice.ptr, slice.len > 0 else { return "" }
        return String(decoding: UnsafeBufferPointer(start: ptr, count: slice.len), as: UTF8.self)
    }
}

public nonisolated enum ScriptMetaScanMode: UInt32, Codable, Sendable {
    case fileListOnly = 0
    case metadataOnly = 1
    case fileListAndMetadata = 2
}

public nonisolated enum ScriptMetaRootPurpose: UInt32, Codable, Sendable {
    case fileList = 0
    case metadataCatalog = 1
    case updateCheck = 2
    case fileListAndMetadata = 3
}

public nonisolated enum ScriptMetaWatchPolicy: UInt32, Codable, Sendable {
    case disabled = 0
    case visibleOnly = 1
    case allRegistered = 2
    case manual = 3
}

public nonisolated enum ScriptMetaCachePolicy: UInt32, Codable, Sendable {
    case disabled = 0
    case memoryOnly = 1
    case persistentCatalogOnly = 2
    case memoryAndPersistent = 3
}

public nonisolated enum ScriptMetaCacheScope: UInt32, Codable, Sendable {
    case all = 0
    case catalog = 1
    case fileList = 2
    case root = 3
}

public nonisolated enum ScriptMetaOperationTerminationPolicy: String, Codable, Sendable {
    case waitForCurrentOperation
    case cancelCurrentOperation
}

public nonisolated struct ScriptMetaKitOperationalPolicy: Codable, Sendable, Equatable {
    public var maxConcurrentMetaURLChecks: Int
    public var retryAttempts: Int
    public var retryInitialDelayMillis: UInt64
    public var retryBackoffMultiplier: UInt32
    public var maxRetryDelayMillis: UInt64
    public var requestTimeoutMillis: UInt64
    public var resourceTimeoutMillis: UInt64
    public var watcherDebounceDelayMillis: UInt64
    public var watcherMaxDeliveryDelayMillis: UInt64
    public var watcherMaxPendingPaths: Int

    public init(
        maxConcurrentMetaURLChecks: Int = 6,
        retryAttempts: Int = 2,
        retryInitialDelayMillis: UInt64 = 500,
        retryBackoffMultiplier: UInt32 = 3,
        maxRetryDelayMillis: UInt64 = 30_000,
        requestTimeoutMillis: UInt64 = 15_000,
        resourceTimeoutMillis: UInt64 = 15_000,
        watcherDebounceDelayMillis: UInt64 = 500,
        watcherMaxDeliveryDelayMillis: UInt64 = 2_000,
        watcherMaxPendingPaths: Int = 1_024
    ) {
        self.maxConcurrentMetaURLChecks = maxConcurrentMetaURLChecks
        self.retryAttempts = retryAttempts
        self.retryInitialDelayMillis = retryInitialDelayMillis
        self.retryBackoffMultiplier = retryBackoffMultiplier
        self.maxRetryDelayMillis = maxRetryDelayMillis
        self.requestTimeoutMillis = requestTimeoutMillis
        self.resourceTimeoutMillis = resourceTimeoutMillis
        self.watcherDebounceDelayMillis = watcherDebounceDelayMillis
        self.watcherMaxDeliveryDelayMillis = watcherMaxDeliveryDelayMillis
        self.watcherMaxPendingPaths = watcherMaxPendingPaths
    }

    public static let balanced = ScriptMetaKitOperationalPolicy()
    public static let interactive = ScriptMetaKitOperationalPolicy(
        watcherDebounceDelayMillis: 250,
        watcherMaxDeliveryDelayMillis: 1_000
    )
    public static let lowImpact = ScriptMetaKitOperationalPolicy(
        maxConcurrentMetaURLChecks: 2,
        retryInitialDelayMillis: 1_000,
        retryBackoffMultiplier: 2,
        requestTimeoutMillis: 20_000,
        resourceTimeoutMillis: 20_000,
        watcherDebounceDelayMillis: 1_000,
        watcherMaxDeliveryDelayMillis: 4_000,
        watcherMaxPendingPaths: 512
    )
}

public nonisolated enum ScriptMetaRefreshPolicy: UInt32, Codable, Sendable {
    case manualOnly = 0
    case onVisible = 1
    case onFileEvent = 2
    case onFileEventDeferred = 3
    case scheduled = 4
}

public nonisolated enum ScriptMetaRootPriority: UInt32, Codable, Sendable {
    case visibleWhenSelected = 0
    case userInitiated = 1
    case background = 2
}

public nonisolated struct ScriptMetaKitRoot: Codable, Identifiable, Sendable {
    public var rootID: String
    public var url: URL
    public var displayName: String?
    public var purpose: ScriptMetaRootPurpose
    public var watchPolicy: ScriptMetaWatchPolicy
    public var cachePolicy: ScriptMetaCachePolicy
    public var refreshPolicy: ScriptMetaRefreshPolicy
    public var priority: ScriptMetaRootPriority

    public var id: String { rootID }

    public init(
        rootID: String,
        url: URL,
        displayName: String? = nil,
        purpose: ScriptMetaRootPurpose = .fileListAndMetadata,
        watchPolicy: ScriptMetaWatchPolicy = .visibleOnly,
        cachePolicy: ScriptMetaCachePolicy = .memoryAndPersistent,
        refreshPolicy: ScriptMetaRefreshPolicy = .onFileEventDeferred,
        priority: ScriptMetaRootPriority = .userInitiated
    ) {
        self.rootID = rootID
        self.url = url
        self.displayName = displayName
        self.purpose = purpose
        self.watchPolicy = watchPolicy
        self.cachePolicy = cachePolicy
        self.refreshPolicy = refreshPolicy
        self.priority = priority
    }
}

public nonisolated struct ScriptMetaRootPreflightOptions: Codable, Sendable, Equatable {
    public var rejectTrashRoots: Bool
    public var rejectRestrictedRoots: Bool
    public var rejectLowScriptDensityLargeRoots: Bool
    public var maxScannedItems: Int
    public var maxDurationMillis: UInt64
    public var minScannedFileCountForLargeRoot: Int
    public var minScriptRatioDenominator: Int
    public var minScannedItemsForTimeLimit: Int

    public init(
        rejectTrashRoots: Bool = true,
        rejectRestrictedRoots: Bool = true,
        rejectLowScriptDensityLargeRoots: Bool = true,
        maxScannedItems: Int = 10_000,
        maxDurationMillis: UInt64 = 1_000,
        minScannedFileCountForLargeRoot: Int = 1_000,
        minScriptRatioDenominator: Int = 100,
        minScannedItemsForTimeLimit: Int = 500
    ) {
        self.rejectTrashRoots = rejectTrashRoots
        self.rejectRestrictedRoots = rejectRestrictedRoots
        self.rejectLowScriptDensityLargeRoots = rejectLowScriptDensityLargeRoots
        self.maxScannedItems = maxScannedItems
        self.maxDurationMillis = maxDurationMillis
        self.minScannedFileCountForLargeRoot = minScannedFileCountForLargeRoot
        self.minScriptRatioDenominator = minScriptRatioDenominator
        self.minScannedItemsForTimeLimit = minScannedItemsForTimeLimit
    }
}

public nonisolated struct ScriptMetaScanResult: Codable, Sendable {
    public var roots: [RootSnapshot]
    public var fileListSnapshots: [FileListSnapshot]
    public var catalogSnapshot: ScriptMetaCatalogSnapshot?
    public var operation: OperationInfo?
    public var fileIssues: [FileIssue]?
    public var updateCheckResult: UpdateCheckResult?
    public var changeSummary: ScanChangeSummary?
    public var watchChangeBatch: WatchChangeBatch?
    public var watchDelivery: ScriptMetaKitWatchDelivery?

    enum CodingKeys: String, CodingKey {
        case roots
        case fileListSnapshots = "file_list_snapshots"
        case catalogSnapshot = "catalog_snapshot"
        case operation
        case fileIssues = "file_issues"
        case updateCheckResult = "update_check_result"
        case changeSummary = "change_summary"
        case watchChangeBatch = "watch_change_batch"
        case watchDelivery = "watch_delivery"
    }

    public var allItems: [ScriptMetaItem] {
        catalogSnapshot?.allItems ?? []
    }

    public var fileItems: [ScriptMetaItem] {
        catalogSnapshot?.fileItems ?? []
    }

    public var flattenedFileEntries: [FileSystemEntry] {
        fileListSnapshots.flatMap { snapshot in
            snapshot.children?.flatMap(\.flattened) ?? []
        }
    }
}

public nonisolated struct ScriptMetaKitWatchDelivery: Codable, Sendable {
    public var isReconciliation: Bool
    public var coversAllWatchedRoots: Bool
    public var streamID: String?
    public var sequence: UInt64?
}

public nonisolated struct OperationInfo: Codable, Sendable {
    public var status: String
    public var totalUnits: Int
    public var completedUnits: Int
    public var failedUnits: Int
    public var cancelled: Bool
    public var timedOut: Bool
    public var reasonCode: String?
    public var message: String?

    enum CodingKeys: String, CodingKey {
        case status
        case totalUnits = "total_units"
        case completedUnits = "completed_units"
        case failedUnits = "failed_units"
        case cancelled
        case timedOut = "timed_out"
        case reasonCode = "reason_code"
        case message
    }
}

public nonisolated struct FileIssue: Codable, Identifiable, Sendable {
    public var rootID: String?
    public var path: String
    public var code: String
    public var message: String
    public var pathKind: String?
    public var resolutionStatus: String?
    public var isDirectory: Bool

    public var id: String { "\(rootID ?? ""):\(code):\(path)" }

    enum CodingKeys: String, CodingKey {
        case rootID = "root_id"
        case path
        case code
        case message
        case pathKind = "path_kind"
        case resolutionStatus = "resolution_status"
        case isDirectory = "is_directory"
    }
}

public nonisolated struct ScriptMetaKitRevision: Codable, Hashable, Sendable {
    public let workspaceEpoch: String
    public let sequence: UInt64

    public static let unavailable = ScriptMetaKitRevision(workspaceEpoch: "", sequence: 0)
    public var isAvailable: Bool { !workspaceEpoch.isEmpty && sequence > 0 }

    enum CodingKeys: String, CodingKey {
        case workspaceEpoch = "workspace_epoch"
        case sequence
    }
}

public nonisolated struct RootSnapshot: Codable, Identifiable, Sendable {
    public var rootID: String
    public var path: String
    public var status: String
    public var isDirty: Bool
    public var lastLoadedAt: UInt64?
    public var lastEventAt: UInt64?
    public var itemCount: Int
    public var error: RootError?
    public var stateRevision: ScriptMetaKitRevision = .unavailable

    public var id: String { rootID }

    init(
        rootID: String,
        path: String,
        status: String,
        isDirty: Bool,
        lastLoadedAt: UInt64?,
        lastEventAt: UInt64?,
        itemCount: Int,
        error: RootError?,
        stateRevision: ScriptMetaKitRevision = .unavailable
    ) {
        self.rootID = rootID
        self.path = path
        self.status = status
        self.isDirty = isDirty
        self.lastLoadedAt = lastLoadedAt
        self.lastEventAt = lastEventAt
        self.itemCount = itemCount
        self.error = error
        self.stateRevision = stateRevision
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            rootID: try values.decode(String.self, forKey: .rootID),
            path: try values.decode(String.self, forKey: .path),
            status: try values.decode(String.self, forKey: .status),
            isDirty: try values.decode(Bool.self, forKey: .isDirty),
            lastLoadedAt: try values.decodeIfPresent(UInt64.self, forKey: .lastLoadedAt),
            lastEventAt: try values.decodeIfPresent(UInt64.self, forKey: .lastEventAt),
            itemCount: try values.decode(Int.self, forKey: .itemCount),
            error: try values.decodeIfPresent(RootError.self, forKey: .error),
            stateRevision: try values.decodeIfPresent(ScriptMetaKitRevision.self, forKey: .stateRevision) ?? .unavailable
        )
    }

    enum CodingKeys: String, CodingKey {
        case rootID = "root_id"
        case path
        case status
        case isDirty = "is_dirty"
        case lastLoadedAt = "last_loaded_at"
        case lastEventAt = "last_event_at"
        case itemCount = "item_count"
        case error
        case stateRevision = "state_revision"
    }
}

public nonisolated struct RootError: Codable, Sendable {
    public var code: String
    public var message: String
}

public nonisolated struct FileListSnapshot: Codable, Sendable {
    public var root: RootSnapshot
    public var children: [FileSystemEntry]?
    public var directoryStates: [String: DirectoryState]
    public var truncated: Bool
    public var contentRevision: ScriptMetaKitRevision = .unavailable

    init(
        root: RootSnapshot,
        children: [FileSystemEntry]?,
        directoryStates: [String: DirectoryState],
        truncated: Bool,
        contentRevision: ScriptMetaKitRevision = .unavailable
    ) {
        self.root = root
        self.children = children
        self.directoryStates = directoryStates
        self.truncated = truncated
        self.contentRevision = contentRevision
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            root: try values.decode(RootSnapshot.self, forKey: .root),
            children: try values.decodeIfPresent([FileSystemEntry].self, forKey: .children),
            directoryStates: try values.decode([String: DirectoryState].self, forKey: .directoryStates),
            truncated: try values.decode(Bool.self, forKey: .truncated),
            contentRevision: try values.decodeIfPresent(ScriptMetaKitRevision.self, forKey: .contentRevision) ?? .unavailable
        )
    }

    enum CodingKeys: String, CodingKey {
        case root
        case children
        case directoryStates = "directory_states"
        case truncated
        case contentRevision = "content_revision"
    }
}

public nonisolated struct DirectoryState: Codable, Sendable {
    public var modificationTimeMillis: UInt64?
    public var childCount: Int
    public var childFingerprint: UInt64
    public var identity: FileIdentity?

    enum CodingKeys: String, CodingKey {
        case modificationTimeMillis = "modification_time_millis"
        case childCount = "child_count"
        case childFingerprint = "child_fingerprint"
        case identity
    }
}

public nonisolated struct FileIdentity: Codable, Sendable {
    public var stableID: String
    public var volumeID: String?
    public var fileID: String?
    public var fileSize: UInt64?
    public var contentModifiedAt: UInt64?

    enum CodingKeys: String, CodingKey {
        case stableID = "stable_id"
        case volumeID = "volume_id"
        case fileID = "file_id"
        case fileSize = "file_size"
        case contentModifiedAt = "content_modified_at"
    }
}

public nonisolated struct FileSystemEntry: Codable, Identifiable, Sendable {
    public var displayPath: String
    public var resolvedPath: String
    public var pathKind: String
    public var resolutionStatus: String
    public var resolutionMessage: String?
    public var isDirectory: Bool
    public var fileSize: UInt64?
    public var contentModifiedAt: UInt64?
    public var identity: FileIdentity?
    public var runtimeKind: String?
    public var shebang: String?
    public var hasScriptMeta: Bool
    public var hasScriptMetaEditPassword: Bool
    public var isFileLocked: Bool
    public var isReadOnly: Bool
    public var canEditScriptMeta: Bool
    public var canAppendScriptMeta: Bool
    public var scriptMetaEditState: String
    public var scriptMetaItem: ScriptMetaItem?
    public var children: [FileSystemEntry]

    public var id: String { displayPath }
    public var name: String { URL(fileURLWithPath: displayPath).lastPathComponent }
    public var flattened: [FileSystemEntry] { [self] + children.flatMap(\.flattened) }
    public var outlineChildren: [FileSystemEntry]? { children.isEmpty ? nil : children }

    enum CodingKeys: String, CodingKey {
        case displayPath = "display_path"
        case resolvedPath = "resolved_path"
        case pathKind = "path_kind"
        case resolutionStatus = "resolution_status"
        case resolutionMessage = "resolution_message"
        case isDirectory = "is_directory"
        case fileSize = "file_size"
        case contentModifiedAt = "content_modified_at"
        case identity
        case runtimeKind = "runtime_kind"
        case shebang
        case hasScriptMeta = "has_scriptmeta"
        case hasScriptMetaEditPassword = "has_scriptmeta_edit_password"
        case isFileLocked = "is_file_locked"
        case isReadOnly = "is_read_only"
        case canEditScriptMeta = "can_edit_scriptmeta"
        case canAppendScriptMeta = "can_append_scriptmeta"
        case scriptMetaEditState = "scriptmeta_edit_state"
        case scriptMetaItem = "scriptmeta_item"
        case children
    }

}

public nonisolated struct ScanChangeSummary: Codable, Sendable {
    public var addedCount: Int
    public var removedCount: Int
    public var modifiedCount: Int
    public var changes: [FileEntryChange]

    enum CodingKeys: String, CodingKey {
        case addedCount = "added_count"
        case removedCount = "removed_count"
        case modifiedCount = "modified_count"
        case changes
    }

    public var totalCount: Int { addedCount + removedCount + modifiedCount }
}

public nonisolated struct FileEntryChange: Codable, Identifiable, Sendable {
    public var rootID: String
    public var kind: String
    public var displayPath: String
    public var resolvedPath: String
    public var pathKind: String
    public var resolutionStatus: String
    public var resolutionMessage: String?
    public var isDirectory: Bool
    public var fileSize: UInt64?
    public var contentModifiedAt: UInt64?
    public var identity: FileIdentity?
    public var runtimeKind: String?
    public var shebang: String?
    public var hasScriptMeta: Bool
    public var hasScriptMetaEditPassword: Bool
    public var isFileLocked: Bool
    public var isReadOnly: Bool
    public var canEditScriptMeta: Bool
    public var canAppendScriptMeta: Bool
    public var scriptMetaEditState: String

    public var id: String { "\(kind):\(displayPath)" }
    public var name: String { URL(fileURLWithPath: displayPath).lastPathComponent }

    enum CodingKeys: String, CodingKey {
        case rootID = "root_id"
        case kind
        case displayPath = "display_path"
        case resolvedPath = "resolved_path"
        case pathKind = "path_kind"
        case resolutionStatus = "resolution_status"
        case resolutionMessage = "resolution_message"
        case isDirectory = "is_directory"
        case fileSize = "file_size"
        case contentModifiedAt = "content_modified_at"
        case identity
        case runtimeKind = "runtime_kind"
        case shebang
        case hasScriptMeta = "has_scriptmeta"
        case hasScriptMetaEditPassword = "has_scriptmeta_edit_password"
        case isFileLocked = "is_file_locked"
        case isReadOnly = "is_read_only"
        case canEditScriptMeta = "can_edit_scriptmeta"
        case canAppendScriptMeta = "can_append_scriptmeta"
        case scriptMetaEditState = "scriptmeta_edit_state"
    }
}

public nonisolated struct WatchChangeBatch: Codable, Sendable {
    public var overflowed: Bool
    public var events: [WatchPathEvent]
    public var ignoredPaths: [IgnoredWatchPath]
    public var renameCandidates: [WatchRenameCandidate]
    public var rescanTargets: [WatchRescanTarget]

    enum CodingKeys: String, CodingKey {
        case overflowed
        case events
        case ignoredPaths = "ignored_paths"
        case renameCandidates = "rename_candidates"
        case rescanTargets = "rescan_targets"
    }
}

public nonisolated struct WatchPathEvent: Codable, Identifiable, Sendable {
    public var rootID: String
    public var path: String
    public var kind: String
    public var isDirectory: Bool
    public var rescanDirectory: String

    public var id: String { "\(rootID):\(kind):\(path)" }

    enum CodingKeys: String, CodingKey {
        case rootID = "root_id"
        case path
        case kind
        case isDirectory = "is_directory"
        case rescanDirectory = "rescan_directory"
    }
}

public nonisolated struct IgnoredWatchPath: Codable, Identifiable, Sendable {
    public var rootID: String?
    public var path: String
    public var reason: String

    public var id: String { "\(rootID ?? ""):\(reason):\(path)" }

    enum CodingKeys: String, CodingKey {
        case rootID = "root_id"
        case path
        case reason
    }
}

public nonisolated struct WatchRenameCandidate: Codable, Identifiable, Sendable {
    public var rootID: String
    public var oldPath: String
    public var newPath: String
    public var confidence: String

    public var id: String { "\(rootID):\(oldPath):\(newPath)" }

    enum CodingKeys: String, CodingKey {
        case rootID = "root_id"
        case oldPath = "old_path"
        case newPath = "new_path"
        case confidence
    }
}

public nonisolated struct WatchRescanTarget: Codable, Identifiable, Sendable {
    public var rootID: String
    public var path: String
    public var reason: String

    public var id: String { "\(rootID):\(reason):\(path)" }

    enum CodingKeys: String, CodingKey {
        case rootID = "root_id"
        case path
        case reason
    }
}

public nonisolated struct ScriptMetaCatalogSnapshot: Codable, Sendable {
    public var sourceRevision: String
    public var roots: [RootSnapshot]
    public var allItems: [ScriptMetaItem]
    public var fileItems: [ScriptMetaItem]
    public var candidateCache: CandidateCache
    public var updateCheckResult: UpdateCheckResult?

    enum CodingKeys: String, CodingKey {
        case sourceRevision = "source_revision"
        case roots
        case allItems = "all_items"
        case fileItems = "file_items"
        case candidateCache = "candidate_cache"
        case updateCheckResult = "update_check_result"
    }
}

public nonisolated struct CandidateCache: Codable, Sendable {
    public var schemaVersion: UInt32
    public var builtAt: UInt64
    public var registeredRoots: [RegisteredRootSignature]
    public var records: [CandidateRecord]

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case builtAt = "built_at"
        case registeredRoots = "registered_roots"
        case records
    }
}

public nonisolated struct RegisteredRootSignature: Codable, Sendable {
    public var rootID: String
    public var path: String

    enum CodingKeys: String, CodingKey {
        case rootID = "root_id"
        case path
    }
}

public nonisolated struct CandidateRecord: Codable, Sendable {
    public var rootID: String
    public var rootPath: String
    public var filePath: String
    public var identityPath: String
    public var pathKind: String?
    public var resolutionStatus: String?
    public var resolutionMessage: String?
    public var runtimeKind: String?
    public var shebang: String?
    public var hasScriptMeta: Bool?
    public var hasScriptMetaEditPassword: Bool?
    public var isFileLocked: Bool?
    public var isReadOnly: Bool?
    public var canEditScriptMeta: Bool?
    public var canAppendScriptMeta: Bool?
    public var scriptMetaEditState: String?
    public var fileSize: UInt64?
    public var contentModifiedAt: UInt64?
    public var item: ScriptMetaItem?

    enum CodingKeys: String, CodingKey {
        case rootID = "root_id"
        case rootPath = "root_path"
        case filePath = "file_path"
        case identityPath = "identity_path"
        case pathKind = "path_kind"
        case resolutionStatus = "resolution_status"
        case resolutionMessage = "resolution_message"
        case runtimeKind = "runtime_kind"
        case shebang
        case hasScriptMeta = "has_scriptmeta"
        case hasScriptMetaEditPassword = "has_scriptmeta_edit_password"
        case isFileLocked = "is_file_locked"
        case isReadOnly = "is_read_only"
        case canEditScriptMeta = "can_edit_scriptmeta"
        case canAppendScriptMeta = "can_append_scriptmeta"
        case scriptMetaEditState = "scriptmeta_edit_state"
        case fileSize = "file_size"
        case contentModifiedAt = "content_modified_at"
        case item
    }

}

public nonisolated struct ScriptMetaItem: Codable, Identifiable, Sendable {
    public var rootID: String
    public var filePath: String
    public var identityPath: String
    public var runtimeKind: String?
    public var shebang: String?
    public var scriptID: String
    public var version: String?
    public var itemDescription: String?
    public var targetApp: String?
    public var minTargetVersion: String?
    public var metaURL: String?
    public var name: String?
    public var author: String?
    public var releaseDate: String?
    public var editPasswordSHA256: String?
    public var hasScriptMeta: Bool
    public var hasScriptMetaEditPassword: Bool
    public var isFileLocked: Bool
    public var isReadOnly: Bool
    public var canEditScriptMeta: Bool
    public var canAppendScriptMeta: Bool
    public var scriptMetaEditState: String

    public var id: String { "\(rootID):\(filePath)" }
    public var displayName: String { name ?? URL(fileURLWithPath: filePath).lastPathComponent }

    enum CodingKeys: String, CodingKey {
        case rootID = "root_id"
        case filePath = "file_path"
        case identityPath = "identity_path"
        case runtimeKind = "runtime_kind"
        case shebang
        case scriptID = "script_id"
        case version
        case itemDescription = "description"
        case targetApp = "target_app"
        case minTargetVersion = "min_target_version"
        case metaURL = "meta_url"
        case name
        case author
        case releaseDate = "release_date"
        case editPasswordSHA256 = "edit_password_sha256"
        case hasScriptMeta = "has_scriptmeta"
        case hasScriptMetaEditPassword = "has_scriptmeta_edit_password"
        case isFileLocked = "is_file_locked"
        case isReadOnly = "is_read_only"
        case canEditScriptMeta = "can_edit_scriptmeta"
        case canAppendScriptMeta = "can_append_scriptmeta"
        case scriptMetaEditState = "scriptmeta_edit_state"
    }

    public init(
        rootID: String,
        filePath: String,
        identityPath: String,
        runtimeKind: String? = nil,
        shebang: String? = nil,
        scriptID: String,
        version: String? = nil,
        itemDescription: String? = nil,
        targetApp: String? = nil,
        minTargetVersion: String? = nil,
        metaURL: String? = nil,
        name: String? = nil,
        author: String? = nil,
        releaseDate: String? = nil,
        editPasswordSHA256: String? = nil,
        hasScriptMeta: Bool = true,
        hasScriptMetaEditPassword: Bool = false,
        isFileLocked: Bool = false,
        isReadOnly: Bool = false,
        canEditScriptMeta: Bool = true,
        canAppendScriptMeta: Bool = true,
        scriptMetaEditState: String = "editable"
    ) {
        self.rootID = rootID
        self.filePath = filePath
        self.identityPath = identityPath
        self.runtimeKind = runtimeKind
        self.shebang = shebang
        self.scriptID = scriptID
        self.version = version
        self.itemDescription = itemDescription
        self.targetApp = targetApp
        self.minTargetVersion = minTargetVersion
        self.metaURL = metaURL
        self.name = name
        self.author = author
        self.releaseDate = releaseDate
        self.editPasswordSHA256 = editPasswordSHA256
        self.hasScriptMeta = hasScriptMeta
        self.hasScriptMetaEditPassword = hasScriptMetaEditPassword
        self.isFileLocked = isFileLocked
        self.isReadOnly = isReadOnly
        self.canEditScriptMeta = canEditScriptMeta
        self.canAppendScriptMeta = canAppendScriptMeta
        self.scriptMetaEditState = scriptMetaEditState
    }
}

public nonisolated struct UpdateCheckResult: Codable, Sendable {
    public var checkedAt: UInt64
    public var operation: OperationInfo?
    public var resolutionsByItemID: [String: DistributionResolution]
    public var failuresByItemID: [String: UpdateFailure]
    public var errorsByItemID: [String: String]
    public var statusesByItemID: [String: String]

    enum CodingKeys: String, CodingKey {
        case checkedAt = "checked_at"
        case operation
        case resolutionsByItemID = "resolutions_by_item_id"
        case failuresByItemID = "failures_by_item_id"
        case errorsByItemID = "errors_by_item_id"
        case statusesByItemID = "statuses_by_item_id"
    }

    public init(
        checkedAt: UInt64,
        operation: OperationInfo? = nil,
        resolutionsByItemID: [String: DistributionResolution],
        failuresByItemID: [String: UpdateFailure],
        errorsByItemID: [String: String],
        statusesByItemID: [String: String]
    ) {
        self.checkedAt = checkedAt
        self.operation = operation
        self.resolutionsByItemID = resolutionsByItemID
        self.failuresByItemID = failuresByItemID
        self.errorsByItemID = errorsByItemID
        self.statusesByItemID = statusesByItemID
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        checkedAt = try container.decode(UInt64.self, forKey: .checkedAt)
        operation = try container.decodeIfPresent(OperationInfo.self, forKey: .operation)
        resolutionsByItemID = try container.decode([String: DistributionResolution].self, forKey: .resolutionsByItemID)
        failuresByItemID = try container.decodeIfPresent([String: UpdateFailure].self, forKey: .failuresByItemID) ?? [:]
        errorsByItemID = try container.decode([String: String].self, forKey: .errorsByItemID)
        statusesByItemID = try container.decode([String: String].self, forKey: .statusesByItemID)
    }
}

public nonisolated struct UpdateCheckProgress: Sendable {
    public var completedItems: Int
    public var totalItems: Int
    public var itemID: String?
    public var scriptID: String?
    public var phase: String
    public var message: String

    public init(
        completedItems: Int,
        totalItems: Int,
        itemID: String?,
        scriptID: String?,
        phase: String,
        message: String
    ) {
        self.completedItems = completedItems
        self.totalItems = totalItems
        self.itemID = itemID
        self.scriptID = scriptID
        self.phase = phase
        self.message = message
    }
}

public nonisolated struct UpdateFailureSummary: Identifiable, Sendable {
    public var id: String
    public var displayName: String
    public var reason: String
    public var code: String

    public init(id: String, displayName: String, reason: String, code: String) {
        self.id = id
        self.displayName = displayName
        self.reason = reason
        self.code = code
    }
}

public nonisolated enum ScriptMetaOperationStatus: String, Codable, Sendable {
    case finished
    case cancelled
    case timedOut = "timed_out"
    case partial
}

public nonisolated enum ScriptMetaRootStatus: String, Codable, Sendable {
    case notLoaded = "not_loaded"
    case ready
    case dirty
    case loading
    case missing
    case unreadable
    case timedOut = "timed_out"
    case overflowed
    case cancelled
}

public nonisolated enum ScriptMetaUpdateStatus: String, Codable, Sendable {
    case idle
    case checking
    case upToDate = "up_to_date"
    case updateAvailable = "update_available"
    case failed
    case cancelled
    case notCheckable = "not_checkable"
}

public nonisolated enum ScriptMetaUpdateProgressPhase: String, Codable, Sendable {
    case started
    case checking
    case retrying
    case finishedItem = "finished_item"
    case failedItem = "failed_item"
    case finished
    case cancelled
}

public extension OperationInfo {
    var typedStatus: ScriptMetaOperationStatus? {
        ScriptMetaOperationStatus(rawValue: status)
    }
}

public extension RootSnapshot {
    var typedStatus: ScriptMetaRootStatus? {
        ScriptMetaRootStatus(rawValue: status)
    }
}

public extension UpdateCheckResult {
    var typedStatusesByItemID: [String: ScriptMetaUpdateStatus] {
        statusesByItemID.compactMapValues(ScriptMetaUpdateStatus.init(rawValue:))
    }
}

public extension UpdateCheckProgress {
    var typedPhase: ScriptMetaUpdateProgressPhase? {
        ScriptMetaUpdateProgressPhase(rawValue: phase)
    }
}

public nonisolated struct UpdateFailure: Codable, Sendable {
    public var code: String
    public var message: String
    public var itemID: String
    public var filePath: String
    public var scriptID: String
    public var currentVersion: String?
    public var metaURL: String?
    public var sourceURL: String?
    public var checkedAt: UInt64

    enum CodingKeys: String, CodingKey {
        case code
        case message
        case itemID = "item_id"
        case filePath = "file_path"
        case scriptID = "script_id"
        case currentVersion = "current_version"
        case metaURL = "meta_url"
        case sourceURL = "source_url"
        case checkedAt = "checked_at"
    }
}

public nonisolated struct DistributionResolution: Codable, Sendable {
    public var latestVersion: String?
    public var latestPageURL: String?
    public var finalPageURL: String
    public var latestURLHistory: [String]
    public var checkedAt: UInt64
    public var isUnresolved: Bool
    public var note: String?
    public var redirectCount: UInt32?

    enum CodingKeys: String, CodingKey {
        case latestVersion = "latest_version"
        case latestPageURL = "latest_page_url"
        case finalPageURL = "final_page_url"
        case latestURLHistory = "latest_url_history"
        case checkedAt = "checked_at"
        case isUnresolved = "is_unresolved"
        case note
        case redirectCount = "redirect_count"
    }

    public init(
        latestVersion: String?,
        latestPageURL: String?,
        finalPageURL: String,
        latestURLHistory: [String],
        checkedAt: UInt64,
        isUnresolved: Bool,
        note: String?,
        redirectCount: UInt32?
    ) {
        self.latestVersion = latestVersion
        self.latestPageURL = latestPageURL
        self.finalPageURL = finalPageURL
        self.latestURLHistory = latestURLHistory
        self.checkedAt = checkedAt
        self.isUnresolved = isUnresolved
        self.note = note
        self.redirectCount = redirectCount
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        latestVersion = try container.decodeIfPresent(String.self, forKey: .latestVersion)
        latestPageURL = try container.decodeIfPresent(String.self, forKey: .latestPageURL)
        finalPageURL = try container.decode(String.self, forKey: .finalPageURL)
        latestURLHistory = try container.decodeIfPresent([String].self, forKey: .latestURLHistory) ?? []
        checkedAt = try container.decode(UInt64.self, forKey: .checkedAt)
        isUnresolved = try container.decode(Bool.self, forKey: .isUnresolved)
        note = try container.decodeIfPresent(String.self, forKey: .note)
        redirectCount = try container.decodeIfPresent(UInt32.self, forKey: .redirectCount)
    }
}

public nonisolated struct ScriptMetadataDraft: Codable, Sendable {
    public var scriptID: String
    public var version: String?
    public var itemDescription: String?
    public var targetApp: String?
    public var minTargetVersion: String?
    public var metaURL: String?
    public var name: String?
    public var author: String?
    public var releaseDate: String?
    public var editPasswordSHA256: String?

    enum CodingKeys: String, CodingKey {
        case scriptID = "script_id"
        case version
        case itemDescription = "description"
        case targetApp = "target_app"
        case minTargetVersion = "min_target_version"
        case metaURL = "meta_url"
        case name
        case author
        case releaseDate = "release_date"
        case editPasswordSHA256 = "edit_password_sha256"
    }

    public init(
        scriptID: String,
        version: String? = nil,
        itemDescription: String? = nil,
        targetApp: String? = nil,
        minTargetVersion: String? = nil,
        metaURL: String? = nil,
        name: String? = nil,
        author: String? = nil,
        releaseDate: String? = nil,
        editPasswordSHA256: String? = nil
    ) {
        self.scriptID = scriptID
        self.version = version
        self.itemDescription = itemDescription
        self.targetApp = targetApp
        self.minTargetVersion = minTargetVersion
        self.metaURL = metaURL
        self.name = name
        self.author = author
        self.releaseDate = releaseDate
        self.editPasswordSHA256 = editPasswordSHA256
    }
}

public nonisolated struct ScriptMetadataEditReadResult: Codable, Sendable {
    public var filePath: String
    public var draft: ScriptMetadataDraft
    public var commentStyle: String
    public var lineEnding: String
    public var hasExistingBlock: Bool
    public var existingLines: [String]
    public var unknownLines: [String]
    public var existingBlockText: String?
    public var sourceFingerprint: String

    enum CodingKeys: String, CodingKey {
        case filePath = "file_path"
        case draft
        case commentStyle = "comment_style"
        case lineEnding = "line_ending"
        case hasExistingBlock = "has_existing_block"
        case existingLines = "existing_lines"
        case unknownLines = "unknown_lines"
        case existingBlockText = "existing_block_text"
        case sourceFingerprint = "source_fingerprint"
    }
}

public nonisolated struct ScriptMetadataEditSession: Codable, Sendable {
    public var fileURL: URL
    public var readResult: ScriptMetadataEditReadResult

    public init(fileURL: URL, readResult: ScriptMetadataEditReadResult) {
        self.fileURL = fileURL
        self.readResult = readResult
    }

    public var draft: ScriptMetadataDraft {
        readResult.draft
    }

    public var sourceFingerprint: String {
        readResult.sourceFingerprint
    }
}

public nonisolated struct ScriptMetadataEditPreviewResult: Codable, Sendable {
    public var filePath: String
    public var previewText: String
    public var previewByteCount: Int
    public var fileSize: UInt64?
    public var commentStyle: String?
    public var lineEnding: String
    public var hasScriptmetaMarkerInPreview: Bool
    public var isTruncated: Bool
    public var requiresFullRead: Bool
    public var fileStateFingerprint: String

    enum CodingKeys: String, CodingKey {
        case filePath = "file_path"
        case previewText = "preview_text"
        case previewByteCount = "preview_byte_count"
        case fileSize = "file_size"
        case commentStyle = "comment_style"
        case lineEnding = "line_ending"
        case hasScriptmetaMarkerInPreview = "has_scriptmeta_marker_in_preview"
        case isTruncated = "is_truncated"
        case requiresFullRead = "requires_full_read"
        case fileStateFingerprint = "file_state_fingerprint"
    }
}

public nonisolated struct DistributionMetadataDraft: Codable, Sendable {
    public var scriptID: String
    public var version: String?
    public var latestURL: String?
    public var latestPageURL: String?

    enum CodingKeys: String, CodingKey {
        case scriptID = "script_id"
        case version
        case latestURL = "latest_url"
        case latestPageURL = "latest_page_url"
    }

    public init(
        scriptID: String,
        version: String? = nil,
        latestURL: String? = nil,
        latestPageURL: String? = nil
    ) {
        self.scriptID = scriptID
        self.version = version
        self.latestURL = latestURL
        self.latestPageURL = latestPageURL
    }
}

public nonisolated enum ScriptMetaWriteMode: String, Codable, Sendable {
    case insertOrReplace = "insert_or_replace"
    case insertOnly = "insert_only"
    case replaceOnly = "replace_only"
}

public nonisolated enum ScriptMetaWriteOperation: String, Codable, Sendable {
    case inserted
    case replaced
    case unknown
}

public nonisolated enum ScriptMetaBackupReason: String, Codable, Sendable {
    case beforeSave = "before_save"
    case beforeRestore = "before_restore"
    case resetInitial = "reset_initial"
    case unknown
}

public nonisolated struct ScriptMetaBackupRecord: Codable, Identifiable, Sendable {
    public var id: String
    public var createdAtMillis: UInt64
    public var backupFileName: String
    public var backupFilePath: String
    public var fileSize: UInt64
    public var reason: ScriptMetaBackupReason

    enum CodingKeys: String, CodingKey {
        case id
        case createdAtMillis = "created_at_millis"
        case backupFileName = "backup_file_name"
        case backupFilePath = "backup_file_path"
        case fileSize = "file_size"
        case reason
    }
}

public nonisolated struct ScriptMetaBackupGeneration: Codable, Identifiable, Sendable {
    public var id: String
    public var sequenceNumber: Int
    public var createdAtMillis: UInt64
    public var filePath: String
    public var fileSize: UInt64
    public var reason: ScriptMetaBackupReason
    public var isCurrentFile: Bool

    enum CodingKeys: String, CodingKey {
        case id
        case sequenceNumber = "sequence_number"
        case createdAtMillis = "created_at_millis"
        case filePath = "file_path"
        case fileSize = "file_size"
        case reason
        case isCurrentFile = "is_current_file"
    }
}

public nonisolated struct ScriptMetadataFileWriteResult: Codable, Sendable {
    public var filePath: String
    public var operation: ScriptMetaWriteOperation
    public var backup: ScriptMetaBackupRecord?

    enum CodingKeys: String, CodingKey {
        case filePath = "file_path"
        case operation
        case backup
    }
}

public nonisolated struct ScriptIdUniquenessReport: Codable, Sendable {
    public var totalItems: Int
    public var uniqueScriptIDs: Int
    public var duplicates: [ScriptIdDuplicate]

    enum CodingKeys: String, CodingKey {
        case totalItems = "total_items"
        case uniqueScriptIDs = "unique_script_ids"
        case duplicates
    }

    public var isUnique: Bool { duplicates.isEmpty }
}

public nonisolated struct ScriptIdUniquenessItem: Codable, Identifiable, Sendable {
    public var itemID: String
    public var filePath: String
    public var scriptID: String

    public var id: String { itemID }

    enum CodingKeys: String, CodingKey {
        case itemID = "item_id"
        case filePath = "file_path"
        case scriptID = "script_id"
    }

    public init(itemID: String, filePath: String, scriptID: String) {
        self.itemID = itemID
        self.filePath = filePath
        self.scriptID = scriptID
    }
}

public nonisolated struct ScriptIdDuplicate: Codable, Identifiable, Sendable {
    public var scriptID: String
    public var itemIDs: [String]
    public var filePaths: [String]

    public var id: String { scriptID }

    enum CodingKeys: String, CodingKey {
        case scriptID = "script_id"
        case itemIDs = "item_ids"
        case filePaths = "file_paths"
    }
}
