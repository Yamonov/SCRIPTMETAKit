import Foundation

/// Obtain defaults from the linked Kit, edit the required fields, then apply atomically.
public nonisolated struct ScriptMetaKitConfiguration: Codable, Sendable, Equatable {
    public var appID: String
    public var cacheNamespace: String
    public var parser: ScriptMetaParserConfiguration
    public var scanner: ScriptMetaScannerConfiguration
    public var watcher: ScriptMetaWatcherConfiguration
    public var cache: ScriptMetaCacheConfiguration
    public var updateCheck: ScriptMetaUpdateConfiguration
    private var extensions: ExtensionList

    private struct ExtensionList: Codable, Sendable, Equatable { var extensions: [String] }
    public var supportedExtensions: [String] {
        get { extensions.extensions }
        set { extensions.extensions = newValue }
    }

    enum CodingKeys: String, CodingKey {
        case appID = "app_id", cacheNamespace = "cache_namespace"
        case extensions = "supported_extensions", parser, scanner, watcher, cache
        case updateCheck = "update_check"
    }

    public static func defaults() throws -> Self { try ScriptMetaService.defaultConfiguration() }
}

public nonisolated struct ScriptMetaParserConfiguration: Codable, Sendable, Equatable {
    public var maxPrefixBytes: Int
    public var requireClosedBlock: Bool
    public var preferredLanguages: [String]
    enum CodingKeys: String, CodingKey {
        case maxPrefixBytes = "max_prefix_bytes", requireClosedBlock = "require_closed_block"
        case preferredLanguages = "preferred_languages"
    }
}

public nonisolated struct ScriptMetaScannerConfiguration: Codable, Sendable, Equatable {
    public var maxDepth: Int
    public var maxNodesPerRoot: Int
    public var skipHidden: Bool
    public var skipPackages: Bool
    public var followSymlinks: Bool
    public var resolveMacOSAlias: Bool
    public var reuseUnchangedRecords: Bool
    public var decompileCompiledOSADuringScan: Bool
    public var includeEmptyDirectories: Bool
    public var scanTimeoutPerRootMillis: UInt64?
    public var rootPreflight: ScriptMetaPreflightConfiguration
    enum CodingKeys: String, CodingKey {
        case maxDepth = "max_depth", maxNodesPerRoot = "max_nodes_per_root"
        case skipHidden = "skip_hidden", skipPackages = "skip_packages", followSymlinks = "follow_symlinks"
        case resolveMacOSAlias = "resolve_macos_alias", reuseUnchangedRecords = "reuse_unchanged_records"
        case decompileCompiledOSADuringScan = "decompile_compiled_osa_during_scan"
        case includeEmptyDirectories = "include_empty_directories", scanTimeoutPerRootMillis = "scan_timeout_per_root_millis"
        case rootPreflight = "root_preflight"
    }

    public func encode(to encoder: Encoder) throws {
        var values = encoder.container(keyedBy: CodingKeys.self)
        try values.encode(maxDepth, forKey: .maxDepth)
        try values.encode(maxNodesPerRoot, forKey: .maxNodesPerRoot)
        try values.encode(skipHidden, forKey: .skipHidden)
        try values.encode(skipPackages, forKey: .skipPackages)
        try values.encode(followSymlinks, forKey: .followSymlinks)
        try values.encode(resolveMacOSAlias, forKey: .resolveMacOSAlias)
        try values.encode(reuseUnchangedRecords, forKey: .reuseUnchangedRecords)
        try values.encode(decompileCompiledOSADuringScan, forKey: .decompileCompiledOSADuringScan)
        try values.encode(includeEmptyDirectories, forKey: .includeEmptyDirectories)
        // Explicit null disables the timeout; an absent field requests Rust defaults.
        try values.encode(scanTimeoutPerRootMillis, forKey: .scanTimeoutPerRootMillis)
        try values.encode(rootPreflight, forKey: .rootPreflight)
    }
}

public nonisolated struct ScriptMetaPreflightConfiguration: Codable, Sendable, Equatable {
    public var rejectTrashRoots: Bool
    public var rejectRestrictedRoots: Bool
    public var rejectLowScriptDensityLargeRoots: Bool
    public var maxScannedItems: Int
    public var maxDurationMillis: UInt64
    public var minScannedFileCountForLargeRoot: Int
    public var minScriptRatioDenominator: Int
    public var minScannedItemsForTimeLimit: Int
    enum CodingKeys: String, CodingKey {
        case rejectTrashRoots = "reject_trash_roots", rejectRestrictedRoots = "reject_restricted_roots"
        case rejectLowScriptDensityLargeRoots = "reject_low_script_density_large_roots"
        case maxScannedItems = "max_scanned_items", maxDurationMillis = "max_duration_millis"
        case minScannedFileCountForLargeRoot = "min_scanned_file_count_for_large_root"
        case minScriptRatioDenominator = "min_script_ratio_denominator"
        case minScannedItemsForTimeLimit = "min_scanned_items_for_time_limit"
    }
}

public nonisolated enum ScriptMetaWatcherMode: String, Codable, Sendable {
    case disabled, visibleOnly = "visible_only", allRegistered = "all_registered", manual
}
public nonisolated enum ScriptMetaMonitorRootStrategy: String, Codable, Sendable {
    case exactRoots = "exact_roots", deduplicateNestedRoots = "deduplicate_nested_roots", platformRecommended = "platform_recommended"
}
public nonisolated enum ScriptMetaOverflowPolicy: String, Codable, Sendable {
    case markAffectedRootsDirty = "mark_affected_roots_dirty", markAllRootsDirty = "mark_all_roots_dirty"
}
public nonisolated struct ScriptMetaWatcherConfiguration: Codable, Sendable, Equatable {
    public var enabled: Bool
    public var watchPolicy: ScriptMetaWatcherMode
    public var debounceDelayMillis: UInt64
    public var maxDeliveryDelayMillis: UInt64
    public var nativeEventLatencyMillis: UInt64
    public var maxPendingPaths: Int
    public var overflowPolicy: ScriptMetaOverflowPolicy
    public var monitorRootStrategy: ScriptMetaMonitorRootStrategy
    var allowsWatching: Bool { enabled && watchPolicy != .disabled && watchPolicy != .manual }
    enum CodingKeys: String, CodingKey {
        case enabled, watchPolicy = "watch_policy", debounceDelayMillis = "debounce_delay_millis"
        case maxDeliveryDelayMillis = "max_delivery_delay_millis", nativeEventLatencyMillis = "native_event_latency_millis"
        case maxPendingPaths = "max_pending_paths", overflowPolicy = "overflow_policy", monitorRootStrategy = "monitor_root_strategy"
    }
}

public nonisolated struct ScriptMetaCacheConfiguration: Codable, Sendable, Equatable {
    public var enabled: Bool
    public var memoryCache: Bool
    public var persistentCache: Bool
    public var idleLifetimeMillis: UInt64
    public var maxMemoryNodes: Int
    public var maxDeferredDirtyDirectories: Int
    public var preserveUpdateResults: Bool
    enum CodingKeys: String, CodingKey {
        case enabled, memoryCache = "memory_cache", persistentCache = "persistent_cache"
        case idleLifetimeMillis = "idle_lifetime_millis", maxMemoryNodes = "max_memory_nodes"
        case maxDeferredDirtyDirectories = "max_deferred_dirty_directories", preserveUpdateResults = "preserve_update_results"
    }
}

public nonisolated struct ScriptMetaUpdateConfiguration: Codable, Sendable, Equatable {
    public var enabled: Bool
    public var maxConcurrentMetaURLChecks: Int
    public var retryAttempts: Int
    public var retryInitialDelayMillis: UInt64
    public var retryBackoffMultiplier: UInt32
    public var maxRetryDelayMillis: UInt64
    public var requestTimeoutMillis: UInt64?
    public var resourceTimeoutMillis: UInt64?
    /// Retain bounded HTTP validators/bodies between update checks. Defaults to true;
    /// global memory-cache disablement also disables this cross-operation retention.
    public var cacheNetworkResponses: Bool
    enum CodingKeys: String, CodingKey {
        case enabled, maxConcurrentMetaURLChecks = "max_concurrent_meta_url_checks", retryAttempts = "retry_attempts"
        case retryInitialDelayMillis = "retry_initial_delay_millis", retryBackoffMultiplier = "retry_backoff_multiplier"
        case maxRetryDelayMillis = "max_retry_delay_millis", requestTimeoutMillis = "request_timeout_millis"
        case resourceTimeoutMillis = "resource_timeout_millis", cacheNetworkResponses = "cache_network_responses"
    }

    public func encode(to encoder: Encoder) throws {
        var values = encoder.container(keyedBy: CodingKeys.self)
        try values.encode(enabled, forKey: .enabled)
        try values.encode(maxConcurrentMetaURLChecks, forKey: .maxConcurrentMetaURLChecks)
        try values.encode(retryAttempts, forKey: .retryAttempts)
        try values.encode(retryInitialDelayMillis, forKey: .retryInitialDelayMillis)
        try values.encode(retryBackoffMultiplier, forKey: .retryBackoffMultiplier)
        try values.encode(maxRetryDelayMillis, forKey: .maxRetryDelayMillis)
        try values.encode(requestTimeoutMillis, forKey: .requestTimeoutMillis)
        try values.encode(resourceTimeoutMillis, forKey: .resourceTimeoutMillis)
        try values.encode(cacheNetworkResponses, forKey: .cacheNetworkResponses)
    }
}

public nonisolated struct ScriptMetaKitConfigurationChange: Codable, Sendable, Equatable {
    public var changed: Bool
    public var metadataInvalidated: Bool
    public var refreshRequired: Bool
    public var persistenceChanged: Bool
    public var watcherChanged: Bool
    enum CodingKeys: String, CodingKey {
        case changed, metadataInvalidated = "metadata_invalidated", refreshRequired = "refresh_required"
        case persistenceChanged = "persistence_changed", watcherChanged = "watcher_changed"
    }
}

nonisolated struct AppliedScriptMetaConfiguration: Decodable {
    var configuration: ScriptMetaKitConfiguration
    var change: ScriptMetaKitConfigurationChange
}
