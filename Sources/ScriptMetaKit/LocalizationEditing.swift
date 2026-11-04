import Foundation
import ScriptMetaKitFFI

public nonisolated enum MetadataLocalizedField: String, Codable, Sendable {
    case name, author, description
}

/// Exact source values. Missing, empty, and whitespace-only fields do not fall back.
public nonisolated struct LocalizedMetadataText: Codable, Equatable, Sendable {
    public var name: String?
    public var author: String?
    public var description: String?

    public init(name: String? = nil, author: String? = nil, description: String? = nil) {
        self.name = name; self.author = author; self.description = description
    }

    fileprivate subscript(field: MetadataLocalizedField) -> String? {
        get {
            switch field { case .name: name; case .author: author; case .description: description }
        }
        set {
            switch field { case .name: name = newValue; case .author: author = newValue; case .description: description = newValue }
        }
    }
}

/// A value owned by an editing session, not a resident scan result.
/// Description syntax is parsed/reconstructed by the shared native Kit.
public nonisolated struct MetadataLocalizationEditor: Codable, Sendable {
    public private(set) var defaultLanguage: String?
    public private(set) var translations: [String: LocalizedMetadataText]
    public private(set) var unqualified: LocalizedMetadataText
    private var originalDescription: String?
    private var originalDefaultLanguage: String?
    private var originalTags: [String]

    enum CodingKeys: String, CodingKey {
        case defaultLanguage = "default_language", translations, unqualified
        case originalDescription = "original_description", originalDefaultLanguage = "original_default_language"
        case originalTags = "original_tags"
    }

    public var languages: [String] {
        (defaultLanguage.map { [$0] } ?? []) + translations.keys.filter { $0 != defaultLanguage }.sorted()
    }

    public func text(for field: MetadataLocalizedField, language: String? = nil) throws -> String? {
        guard let language else { return unqualified[field] }
        return translations[try ScriptMetaKitMetadata.normalizeLanguageTag(language)]?[field]
    }

    /// Changes only the editable value; no Description parsing or document serialization per keystroke.
    public mutating func setText(_ value: String?, for field: MetadataLocalizedField, language: String? = nil) throws {
        if let language {
            let key = try ScriptMetaKitMetadata.normalizeLanguageTag(language)
            guard var text = translations[key] else {
                throw ScriptMetaKitError.operationFailed(3, "Add the language before editing its text.")
            }
            text[field] = value
            translations[key] = text
        } else {
            guard defaultLanguage == nil else {
                throw ScriptMetaKitError.operationFailed(3, "Specify the language when editing localized metadata.")
            }
            unqualified[field] = value
        }
    }

    public mutating func addLanguage(_ language: String) throws {
        try perform(Operation(operation: "add_language", language: language))
    }

    public mutating func setDefaultLanguage(_ language: String) throws {
        try perform(Operation(operation: "set_default_language", language: language))
    }

    /// Retains the last language's values as ordinary fields. Other languages must be removed explicitly first.
    public mutating func clearDefaultLanguage() throws {
        try perform(Operation(operation: "clear_default_language"))
    }

    /// The last language becomes ordinary text; deleting a default among other languages leaves it unset.
    public mutating func removeLanguage(_ language: String) throws {
        try perform(Operation(operation: "remove_language", language: language))
    }

    /// Preserves common metadata and Tags. Commit the returned draft through the original edit session.
    public func applying(to draft: ScriptMetadataDraft) throws -> ScriptMetadataDraft {
        try ScriptMetaService.withJSON(self) { editor in
            try ScriptMetaService.withJSON(draft) { draft in
                try ScriptMetaService.decode { smk_metadata_localization_apply(editor, draft, $0) }
            }
        }
    }

    private struct Operation: Encodable {
        var operation: String
        var language: String? = nil
    }

    private mutating func perform(_ operation: Operation) throws {
        let updated: Self = try ScriptMetaService.withJSON(self) { editor in
            try ScriptMetaService.withJSON(operation) { operation in
                try ScriptMetaService.decode { smk_metadata_localization_edit(editor, operation, $0) }
            }
        }
        self = updated
    }
}

public extension ScriptMetaKitMetadata {
    static func localizationEditor(from read: ScriptMetadataEditReadResult) throws -> MetadataLocalizationEditor {
        try localizationEditor(from: read.draft)
    }

    static func localizationEditor(from draft: ScriptMetadataDraft) throws -> MetadataLocalizationEditor {
        try ScriptMetaService.withJSON(draft) { draft in
            try ScriptMetaService.decode { smk_metadata_localization_read(draft, $0) }
        }
    }
}
