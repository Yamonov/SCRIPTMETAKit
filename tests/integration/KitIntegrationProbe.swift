import Darwin
import Foundation
import ScriptMetaKit

// Headless Xcode application: validates package embedding without opening windows.
private nonisolated final class ProbeEvents: @unchecked Sendable {
    private let lock = NSLock()
    private var updates = 0
    private var changedAt: Double?
    private var mainThreadProgress = 0
    private var diagnostics: [String] = []

    func update(changed: Bool) {
        lock.lock()
        updates += 1
        if changed && changedAt == nil { changedAt = ProcessInfo.processInfo.systemUptime }
        lock.unlock()
    }
    func progress() {
        guard Thread.isMainThread else { return }
        lock.lock()
        mainThreadProgress += 1
        lock.unlock()
    }
    func diagnostic(_ value: ScriptMetaKitDiagnostic) {
        lock.lock()
        diagnostics.append(value.code)
        lock.unlock()
    }
    var snapshot: (updates: Int, changedAt: Double?, mainThreadProgress: Int, diagnostics: [String]) {
        lock.lock()
        defer { lock.unlock() }
        return (updates, changedAt, mainThreadProgress, diagnostics)
    }
}

private nonisolated struct ProbeUsage {
    var cpuSeconds: Double
    var residentBytes: UInt64
    var peakResidentBytes: Int
    var footprintBytes: UInt64
    var interruptWakeups: UInt64

    static func current() -> Self {
        var usage = rusage()
        precondition(getrusage(RUSAGE_SELF, &usage) == 0)
        var info = rusage_info_v4()
        let status = withUnsafeMutablePointer(to: &info) { pointer in
            pointer.withMemoryRebound(to: rusage_info_t?.self, capacity: 1) {
                proc_pid_rusage(getpid(), RUSAGE_INFO_V4, $0)
            }
        }
        precondition(status == 0)
        let seconds = Double(usage.ru_utime.tv_sec + usage.ru_stime.tv_sec)
            + Double(usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) / 1_000_000
        return Self(cpuSeconds: seconds, residentBytes: info.ri_resident_size,
                    peakResidentBytes: usage.ru_maxrss, footprintBytes: info.ri_phys_footprint,
                    interruptWakeups: info.ri_interrupt_wkups)
    }
}

@main
private enum KitIntegrationProbe {
    @MainActor
    static func main() async throws {
        let benchmark = CommandLine.arguments.contains("--benchmark")
        let count = benchmark ? 1_000 : 8
        let payload = String(repeating: "x", count: benchmark ? 4_096 : 64)
        let base = FileManager.default.temporaryDirectory.appendingPathComponent("KitIntegration-\(UUID().uuidString)")
        let scripts = base.appendingPathComponent("scripts")
        let distribution = base.appendingPathComponent("distribution.txt")
        try FileManager.default.createDirectory(at: scripts, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: base) }
        let body = (0..<count).map { "Script-ID=bench.\($0)\nVersion=2.0.0" }.joined(separator: "\n")
        try "SCRIPTMETA-DIST-BEGIN\n\(body)\nSCRIPTMETA-DIST-END".write(to: distribution, atomically: true, encoding: .utf8)
        for index in 0..<count {
            try autoreleasepool {
                try source(index: index, payload: payload, distribution: distribution)
                    .write(to: scriptURL(index, directory: scripts), atomically: true, encoding: .utf8)
            }
        }

        let info = try ScriptMetaKitRuntime.linkedRuntimeInfo()
        precondition(info.packageVersion == "2.1.1" && info.apiVersion == 1)
        precondition(info.nativeWatch && info.httpUpdates)
        precondition(!ScriptMetaKitRuntime.acknowledgementsText.isEmpty)
        precondition(!ScriptMetaKitRuntime.acknowledgementsSummaryText.isEmpty)

        let events = ProbeEvents()
        let store = ScriptMetaKitPersistentCacheStore(directoryURL: base.appendingPathComponent("cache"))
        let workspace = ScriptMetaKitWorkspace(configuration: .init(cacheStore: store,
            diagnosticHandler: { events.diagnostic($0) }, preferredLanguages: ["ja", "en", "fr"]))
        var configuration = try await workspace.engineConfiguration()
        configuration.parser.customMetadataKeys = ["X-"]
        try await workspace.applyConfiguration(configuration)
        let root = ScriptMetaKitRoot(rootID: "root", url: scripts, watchPolicy: .allRegistered)
        var report: [String: Any] = ["version": info.packageVersion, "architecture": info.architecture, "scripts": count]
        var start = ProcessInfo.processInfo.systemUptime
        try await scan(workspace, root: root, count: count, language: "ja")
        report["cold_scan_and_save_seconds"] = ProcessInfo.processInfo.systemUptime - start
        report["resident_after_scan_bytes"] = ProbeUsage.current().residentBytes
        start = ProcessInfo.processInfo.systemUptime
        try await scan(workspace, root: root, count: count, language: "ja")
        report["repeat_scan_and_save_seconds"] = ProcessInfo.processInfo.systemUptime - start
        await workspace.clearVolatileState()
        start = ProcessInfo.processInfo.systemUptime
        try await restore(workspace, root: root, count: count)
        report["cache_restore_seconds"] = ProcessInfo.processInfo.systemUptime - start
        try await workspace.setPreferredLanguages(["en", "fr"])
        start = ProcessInfo.processInfo.systemUptime
        try await scan(workspace, root: root, count: count, language: "en")
        report["language_reload_seconds"] = ProcessInfo.processInfo.systemUptime - start
        try await checkUpdates(workspace, count: count, events: events)
        precondition(events.snapshot.mainThreadProgress == 0)

        let updates = try await workspace.watchUpdates(roots: [root], replacingGroup: "probe")
        let changedFile = scriptURL(0, directory: scripts).path
        let consumer = Task { @MainActor in
            for try await update in updates {
                events.update(changed: update.result.fileItems.contains {
                    $0.filePath == changedFile && $0.name == "English 0 changed"
                })
            }
        }
        defer { consumer.cancel() }
        try await wait { events.snapshot.updates > 0 }
        try await Task.sleep(for: .seconds(1))
        let before = ProbeUsage.current()
        let beforeEvents = events.snapshot.updates
        start = ProcessInfo.processInfo.systemUptime
        try await Task.sleep(for: .seconds(benchmark ? 10 : 1))
        let elapsed = ProcessInfo.processInfo.systemUptime - start
        let after = ProbeUsage.current()
        report["idle_seconds"] = elapsed
        report["idle_cpu_seconds"] = after.cpuSeconds - before.cpuSeconds
        report["idle_cpu_percent_of_one_core"] = 100 * (after.cpuSeconds - before.cpuSeconds) / elapsed
        report["idle_interrupt_wakeups"] = after.interruptWakeups - before.interruptWakeups
        report["idle_watch_updates"] = events.snapshot.updates - beforeEvents
        report["idle_resident_bytes"] = after.residentBytes
        report["idle_footprint_bytes"] = after.footprintBytes
        precondition(events.snapshot.updates == beforeEvents)

        try source(index: 0, payload: payload, distribution: distribution, changed: true)
            .write(to: scriptURL(0, directory: scripts), atomically: true, encoding: .utf8)
        let changedAt = ProcessInfo.processInfo.systemUptime
        try await wait { events.snapshot.changedAt != nil }
        report["change_delivery_seconds"] = events.snapshot.changedAt! - changedAt
        try await workspace.pauseWatching(rootIDs: [root.rootID])
        let paused = try await workspace.isWatchingPaused(rootID: root.rootID)
        precondition(paused)
        try source(index: 1, payload: payload, distribution: distribution, changed: true)
            .write(to: scriptURL(1, directory: scripts), atomically: true, encoding: .utf8)
        let resumed = try await workspace.resumeWatching(rootIDs: [root.rootID])
        precondition(resumed.roots.allSatisfy { $0.status == "ready" })
        precondition(resumed.fileItems.contains { $0.scriptID == "bench.1" && $0.name == "English 1 changed" })
        let stillPaused = try await workspace.isWatchingPaused(rootID: root.rootID)
        precondition(!stillPaused)
        report["root_pause_resume"] = true
        start = ProcessInfo.processInfo.systemUptime
        await workspace.shutdown()
        try await consumer.value
        report["shutdown_seconds"] = ProcessInfo.processInfo.systemUptime - start
        report["peak_resident_bytes"] = ProbeUsage.current().peakResidentBytes
        report["resident_after_shutdown_bytes"] = ProbeUsage.current().residentBytes
        report["diagnostics"] = events.snapshot.diagnostics
        precondition(events.snapshot.diagnostics.isEmpty)
        let data = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data("\n".utf8))
    }

    private static func scriptURL(_ index: Int, directory: URL) -> URL {
        directory.appendingPathComponent(String(format: "sample-%04d.jsx", index))
    }

    private static func source(index: Int, payload: String, distribution: URL, changed: Bool = false) -> String {
        """
        /*
        SCRIPTMETA-BEGIN
        Script-ID=bench.\(index)
        Version=1.0.0
        Meta-URL=\(distribution.absoluteString)
        Meta-Default-language=de
        Name=Deutsch \(index)
        Name[ja]=日本語 \(index)
        Name[en]=English \(index)\(changed ? " changed" : "")
        Name[fr]=Français \(index)
        Author=Autor
        Tags=image,画像,export,
        common,
        X-org.example-Order=\(index)
        Description-BEGIN
        [de]DE_BODY_\(index)_\(payload)
        [ja]JA_BODY_\(index)_\(payload)
        [en]EN_BODY_\(index)_\(payload)
        [fr]FR_BODY_\(index)_\(payload)
        Description-END
        SCRIPTMETA-END
        */
        // Synthetic test file. Not executed.
        """
    }

    private static func scan(_ workspace: ScriptMetaKitWorkspace, root: ScriptMetaKitRoot, count: Int, language: String) async throws {
        let result = try await workspace.scanRoots([root], replacingGroup: "probe", rootIDs: [root.rootID], mode: .fileListAndMetadata, cacheScope: .all)
        precondition(result.fileItems.count == count)
        precondition(result.fileItems.allSatisfy { $0.presentation.descriptionLanguage == language })
        precondition(result.fileItems.allSatisfy { !($0.itemDescription?.contains("DE_BODY_") ?? true) })
        precondition(result.fileItems.allSatisfy { $0.customValues(for: "X-org.example-Order")?.count == 1 })
    }

    private static func restore(_ workspace: ScriptMetaKitWorkspace, root: ScriptMetaKitRoot, count: Int) async throws {
        _ = try await workspace.cachedFileListSnapshot(root, insertingIntoGroup: "probe", cacheScope: .all)
        let states = try await workspace.cachedFileListStates(rootIDs: [root.rootID], cacheScope: .all)
        precondition(states[root.rootID]?.source == .persistentCache)
        precondition(states[root.rootID]?.freshness == .cachedUnverified)
        precondition(states[root.rootID]?.availableSnapshot?.children?.count == count)
    }

    private static func checkUpdates(_ workspace: ScriptMetaKitWorkspace, count: Int, events: ProbeEvents) async throws {
        let cached = try await workspace.cachedScanResult(rootIDs: ["root"], mode: .metadataOnly)
        let result = try await workspace.checkUpdates(items: cached.fileItems, onProgress: { _ in events.progress() })
        precondition(result.statusesByItemID.count == count)
        precondition(result.statusesByItemID.values.allSatisfy { $0 == "update_available" })
    }

    private static func wait(_ predicate: @escaping @Sendable () -> Bool) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(10))
        while !predicate() {
            if ContinuousClock.now >= deadline { throw CocoaError(.fileReadUnknown) }
            try await Task.sleep(for: .milliseconds(20))
        }
    }
}
