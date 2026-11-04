import Foundation
import ScriptMetaKitFFI

nonisolated let smkStatusCacheIncompatible = Int32(SMK_STATUS_CACHE_INCOMPATIBLE.rawValue)

// Infrequent configuration/editing calls use owned UTF-8 or JSON responses.
// Scan results continue to use the typed ABI and shared string storage.

nonisolated enum ScriptMetaService {
    typealias Call = (UnsafeMutablePointer<OpaquePointer?>) -> SmkStatus

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
            guard readStatus == SMK_STATUS_OK else {
                throw ScriptMetaKitError.operationFailed(readStatus, "Cannot read the Kit response.")
            }
        }
        let bytes = UnsafeBufferPointer(start: slice.ptr, count: slice.len)
        guard status == SMK_STATUS_OK else {
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
