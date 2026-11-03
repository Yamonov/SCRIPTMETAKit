// Internal cache bookkeeping. Availability/revisions do not authorize execution.
nonisolated struct ScriptMetaCacheRevision: Decodable, Sendable {
    let rootID: String
    let stateRevision: ScriptMetaKitRevision
    let fileListRevision: ScriptMetaKitRevision?
    enum CodingKeys: String, CodingKey {
        case rootID = "root_id", stateRevision = "state_revision", fileListRevision = "file_list_revision"
    }
}
