import Foundation

public enum ScriptMetaKitRuntime {
    public static let apiVersion = 1
    public static let packageVersion = "1.1.1"
    public static let usesLocalBinaryArtifact = true

    public static let acknowledgementsText: String = {
        guard let resourceURL = Bundle.module.url(
            forResource: "THIRD_PARTY_LICENSES",
            withExtension: "txt"
        ) else {
            return ""
        }
        return (try? String(contentsOf: resourceURL, encoding: .utf8)) ?? ""
    }()
}
