import Foundation

public enum ScriptMetaKitRuntime {
    public static let apiVersion = 1
    public static let packageVersion = "1.3.0"
    public static let usesLocalBinaryArtifact = true

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
