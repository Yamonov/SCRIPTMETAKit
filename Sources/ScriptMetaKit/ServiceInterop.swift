import Foundation

nonisolated let smkStatusCacheIncompatible: Int32 = 7

// Infrequent configuration/editing calls use owned UTF-8 or JSON responses.
// Scan results continue to use the typed ABI and shared string storage.
@_silgen_name("smk_configuration_default_json")
nonisolated func smk_configuration_default_json(_ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_engine_configuration_json")
nonisolated func smk_engine_configuration_json(_ engine: OpaquePointer?, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_engine_cache_revisions_json")
nonisolated func smk_engine_cache_revisions_json(_ engine: OpaquePointer?, _ roots: UnsafePointer<SmkUtf8Slice>?, _ count: Int, _ mode: UInt32, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_engine_apply_configuration_json")
nonisolated func smk_engine_apply_configuration_json(_ engine: OpaquePointer?, _ configuration: SmkUtf8Slice, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_metadata_parse_text")
nonisolated func smk_metadata_parse_text(_ source: SmkUtf8Slice, _ languages: UnsafePointer<SmkUtf8Slice>?, _ count: Int, _ requireClosed: UInt8, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_metadata_read_draft_text")
nonisolated func smk_metadata_read_draft_text(_ source: SmkUtf8Slice, _ fileName: SmkUtf8Slice, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_metadata_render_text")
nonisolated func smk_metadata_render_text(_ draft: SmkUtf8Slice, _ style: UInt32, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_metadata_write_text")
nonisolated func smk_metadata_write_text(_ source: SmkUtf8Slice, _ fileName: SmkUtf8Slice, _ draft: SmkUtf8Slice, _ mode: UInt32, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_distribution_parse_text")
nonisolated func smk_distribution_parse_text(_ source: SmkUtf8Slice, _ scriptID: SmkUtf8Slice, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_normalize_language_tag")
nonisolated func smk_normalize_language_tag(_ value: SmkUtf8Slice, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_normalize_metadata_tags")
nonisolated func smk_normalize_metadata_tags(_ values: UnsafePointer<SmkUtf8Slice>?, _ count: Int, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_parse_metadata_tag_list")
nonisolated func smk_parse_metadata_tag_list(_ value: SmkUtf8Slice, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_validate_metadata_tag")
nonisolated func smk_validate_metadata_tag(_ value: SmkUtf8Slice, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_normalize_metadata_url")
nonisolated func smk_normalize_metadata_url(_ value: SmkUtf8Slice, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_runtime_info_json")
nonisolated func smk_runtime_info_json(_ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_engine_invalidate_cache")
nonisolated func smk_engine_invalidate_cache(_ engine: OpaquePointer?, _ scope: UInt32, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_engine_notify_changed_paths")
nonisolated func smk_engine_notify_changed_paths(_ engine: OpaquePointer?, _ paths: UnsafePointer<SmkUtf8Slice>?, _ count: Int, _ overflowed: UInt8, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_engine_refresh_dirty_roots")
nonisolated func smk_engine_refresh_dirty_roots(_ engine: OpaquePointer?, _ mode: UInt32, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_engine_pause_root_watching")
nonisolated func smk_engine_pause_root_watching(_ engine: OpaquePointer?, _ roots: UnsafePointer<SmkUtf8Slice>?, _ count: Int) -> Int32
@_silgen_name("smk_engine_resume_root_watching")
nonisolated func smk_engine_resume_root_watching(_ engine: OpaquePointer?, _ roots: UnsafePointer<SmkUtf8Slice>?, _ count: Int, _ output: UnsafeMutablePointer<OpaquePointer?>) -> Int32
@_silgen_name("smk_engine_is_root_watching_paused")
nonisolated func smk_engine_is_root_watching_paused(_ engine: OpaquePointer?, _ root: SmkUtf8Slice, _ output: UnsafeMutablePointer<UInt8>) -> Int32

nonisolated enum ScriptMetaService {
    typealias Call = (UnsafeMutablePointer<OpaquePointer?>) -> Int32

    /// Borrow inputs only for the synchronous native call.
    static func withUTF8<T>(_ value: String, _ body: (SmkUtf8Slice) throws -> T) rethrows -> T {
        var value = value
        return try value.withUTF8 { try body(SmkUtf8Slice(ptr: $0.baseAddress, len: $0.count)) }
    }

    static func withJSON<T: Encodable, R>(_ value: T, _ body: (SmkUtf8Slice) throws -> R) throws -> R {
        let data = try JSONEncoder().encode(value)
        return try data.withUnsafeBytes {
            try body(SmkUtf8Slice(ptr: $0.bindMemory(to: UInt8.self).baseAddress, len: $0.count))
        }
    }

    static func withStrings<T>(_ values: [String], _ body: (UnsafePointer<SmkUtf8Slice>?, Int) throws -> T) rethrows -> T {
        let arena = SmkInputStringArena()
        let slices = values.map { arena.slice($0) }
        return try withExtendedLifetime(arena) {
            try slices.withUnsafeBufferPointer { try body($0.baseAddress, $0.count) }
        }
    }

    private static func response<T>(_ call: Call, _ body: (UnsafeBufferPointer<UInt8>) throws -> T) throws -> T {
        var result: OpaquePointer?
        let status = call(&result)
        defer { smk_edit_result_free(result) }
        var slice = SmkUtf8Slice()
        if let result {
            let readStatus = smk_edit_result_text(result, &slice)
            guard readStatus == 0 else {
                throw ScriptMetaKitError.operationFailed(readStatus, "Cannot read the Kit response.")
            }
        }
        let bytes = UnsafeBufferPointer(start: slice.ptr, count: slice.len)
        guard status == 0 else {
            throw ScriptMetaKitError.operationFailed(status, String(decoding: bytes, as: UTF8.self))
        }
        return try body(bytes)
    }

    static func text(_ call: Call) throws -> String {
        try response(call) { String(decoding: $0, as: UTF8.self) }
    }

    static func decode<T: Decodable>(_ call: Call) throws -> T {
        // One owned byte copy avoids a String-to-Data conversion and keeps all
        // Foundation storage independent of the native handle's lifetime.
        try response(call) { try JSONDecoder().decode(T.self, from: Data($0)) }
    }

    static func complete(_ call: Call) throws { try response(call) { _ in () } }
    static func defaultConfiguration() throws -> ScriptMetaKitConfiguration {
        try decode { smk_configuration_default_json($0) }
    }
}
