import Foundation
import ScriptMetaKitFFI

public enum ScriptMetaKitRuntime {
    public static let apiVersion = 1
    public static let packageVersion = (try? linkedRuntimeInfo().packageVersion) ?? "unknown"
    public static let usesLocalBinaryArtifact = true

    /// Reports the loaded native binary, including enabled capabilities.
    public nonisolated static func linkedRuntimeInfo() throws -> ScriptMetaKitRuntimeInfo {
        try ScriptMetaService.decode { smk_runtime_info_json($0) }
    }

    /// A concise component index suitable for an application's About screen.
    public static let acknowledgementsSummaryText = bundledTextResource(
        named: "THIRD_PARTY_LICENSES_SUMMARY"
    )

    /// Complete license texts and notices required for distribution.
    public static let acknowledgementsText = bundledTextResource(
        named: "THIRD_PARTY_LICENSES"
    )

    private static func bundledTextResource(named name: String) -> String {
        guard let resourceURL = Bundle.module.url(
            forResource: name,
            withExtension: "txt"
        ) else {
            return ""
        }
        return (try? String(contentsOf: resourceURL, encoding: .utf8)) ?? ""
    }
}
