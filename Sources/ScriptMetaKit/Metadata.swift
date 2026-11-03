import Foundation

/// The selected language only. Use readDraft when editing all translations.
public nonisolated struct ScriptMetadata: Codable, Sendable, Equatable {
    public var scriptID: String
    public var version: String?
    public var itemDescription: String?
    public var targetApp: String?
    public var minTargetVersion: String?
    public var metaURL: String?
    public var latestURL: String?
    public var latestVersion: String?
    public var latestPageURL: String?
    public var name: String?
    public var author: String?
    public var releaseDate: String?
    public var editPasswordSHA256: String?
    public var presentation: MetadataPresentation
    public var tags: [String] { presentation.tags }
    enum CodingKeys: String, CodingKey {
        case scriptID = "script_id", version, itemDescription = "description"
        case targetApp = "target_app", minTargetVersion = "min_target_version", metaURL = "meta_url"
        case latestURL = "latest_url", latestVersion = "latest_version", latestPageURL = "latest_page_url"
        case name, author, releaseDate = "release_date", editPasswordSHA256 = "edit_password_sha256", presentation
    }
}

public nonisolated struct DistributionMetadata: Codable, Sendable, Equatable {
    public var scriptID: String?
    public var latestVersion: String?
    public var latestURL: String?
    public var latestPageURL: String?
    public var note: String?
    enum CodingKeys: String, CodingKey {
        case scriptID = "script_id", latestVersion = "latest_version", latestURL = "latest_url"
        case latestPageURL = "latest_page_url", note
    }
}

public nonisolated enum ScriptMetaCommentStyle: UInt32, Sendable {
    case plain = 0, javaScriptBlock = 1, appleScriptBlock = 2
}

/// Pure text operations; no engine, OS locale lookup, or file writes.
/// Calls are synchronous. For large source text, call from the application's worker task.
public nonisolated enum ScriptMetaKitMetadata {
    public static func parse(_ source: String, preferredLanguages: [String] = [], requireClosedBlock: Bool = true) throws -> ScriptMetadata {
        try ScriptMetaService.withUTF8(source) { text in
            try ScriptMetaService.withStrings(preferredLanguages) { languages, count in
                try ScriptMetaService.decode { smk_metadata_parse_text(text, languages, count, requireClosedBlock ? 1 : 0, $0) }
            }
        }
    }

    /// Includes every translation and unknown line for an editing session.
    public static func readDraft(from source: String, fileName: String = "script.jsx") throws -> ScriptMetadataEditReadResult {
        try ScriptMetaService.withUTF8(source) { text in
            try ScriptMetaService.withUTF8(fileName) { name in
                try ScriptMetaService.decode { smk_metadata_read_draft_text(text, name, $0) }
            }
        }
    }

    public static func render(_ draft: ScriptMetadataDraft, style: ScriptMetaCommentStyle = .plain) throws -> String {
        try ScriptMetaService.withJSON(draft) { value in
            try ScriptMetaService.text { smk_metadata_render_text(value, style.rawValue, $0) }
        }
    }

    /// Returns rewritten source text and preserves unedited translations/unknown fields.
    public static func write(_ draft: ScriptMetadataDraft, into source: String, fileName: String = "script.jsx", mode: ScriptMetaWriteMode = .insertOrReplace) throws -> String {
        try ScriptMetaService.withUTF8(source) { text in
            try ScriptMetaService.withUTF8(fileName) { name in
                try ScriptMetaService.withJSON(draft) { value in
                    try ScriptMetaService.text { smk_metadata_write_text(text, name, value, mode.ffiValue, $0) }
                }
            }
        }
    }

    public static func parseDistribution(_ source: String, scriptID: String? = nil) throws -> [DistributionMetadata] {
        try ScriptMetaService.withUTF8(source) { text in
            try ScriptMetaService.withUTF8(scriptID ?? "") { id in
                try ScriptMetaService.decode { smk_distribution_parse_text(text, id, $0) }
            }
        }
    }

    public static func normalizeLanguageTag(_ value: String) throws -> String {
        try ScriptMetaService.withUTF8(value) { input in
            try ScriptMetaService.text { smk_normalize_language_tag(input, $0) }
        }
    }

    /// Trims, validates and removes duplicates, preserving first occurrence order.
    public static func normalizeTags(_ values: [String]) throws -> [String] {
        try ScriptMetaService.withStrings(values) { items, count in
            try ScriptMetaService.decode { smk_normalize_metadata_tags(items, count, $0) }
        }
    }

    public static func parseTagList(_ value: String) throws -> [String] {
        try ScriptMetaService.withUTF8(value) { input in
            try ScriptMetaService.decode { smk_parse_metadata_tag_list(input, $0) }
        }
    }

    public static func isValidTag(_ value: String) throws -> Bool {
        try ScriptMetaService.withUTF8(value) { input in
            try ScriptMetaService.decode { smk_validate_metadata_tag(input, $0) }
        }
    }

    public static func normalizeURL(_ value: String) throws -> String? {
        let result = try ScriptMetaService.withUTF8(value) { input in
            try ScriptMetaService.text { smk_normalize_metadata_url(input, $0) }
        }
        return result.isEmpty ? nil : result
    }
}

public nonisolated struct ScriptMetaKitRuntimeInfo: Decodable, Sendable {
    public let apiVersion: Int
    public let packageName: String
    public let packageVersion: String
    public let architecture: String
    public let platform: String
    public let nativeWatch: Bool
    public let httpUpdates: Bool
    enum CodingKeys: String, CodingKey {
        case apiVersion = "api_version", packageName = "package_name", packageVersion = "package_version"
        case architecture, platform, nativeWatch = "native_watch", httpUpdates = "http_updates"
    }
}
