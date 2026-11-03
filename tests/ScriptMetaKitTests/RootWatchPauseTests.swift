import Foundation
import XCTest
@testable import ScriptMetaKit

@MainActor
final class RootWatchPauseTests: XCTestCase {
    private actor PublicationGate {
        private var continuation: CheckedContinuation<Void, Never>?
        private var opened = false
        func wait() async {
            guard !opened else { return }
            await withCheckedContinuation { continuation = $0 }
        }
        func open() { opened = true; continuation?.resume(); continuation = nil }
    }
    @MainActor
    private final class WatchObservations {
        var receivedInitial = false
        var receivedOther = false
        var receivedAfter = false
        var pauseIsActive = false
        var streamEnded = false
    }
    private func fixture() throws -> (URL, [ScriptMetaKitRoot]) {
        let parent = FileManager.default.temporaryDirectory.appendingPathComponent("KitRootPause-\(UUID().uuidString)")
        let roots = try ["a", "b"].map { id in
            let url = parent.appendingPathComponent(id)
            try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
            try writeScript(url.appendingPathComponent("script.jsx"), id: id, version: "1.0")
            return ScriptMetaKitRoot(rootID: id, url: url, watchPolicy: .allRegistered)
        }
        return (parent, roots)
    }

    private func writeScript(_ url: URL, id: String, version: String) throws {
        let text = "// SCRIPTMETA-BEGIN\n// Script-ID=\(id)\n// Version=\(version)\n// Name=Example\n// SCRIPTMETA-END\n"
        try Data(text.utf8).write(to: url)
    }

    func testResumeReloadsSameSizeAndTimestampWithoutAnActiveWatcher() async throws {
        let (parent, roots) = try fixture()
        defer { try? FileManager.default.removeItem(at: parent) }
        let workspace = ScriptMetaKitWorkspace()
        let path = roots[0].url.appendingPathComponent("script.jsx")
        let fixedDate = Date(timeIntervalSince1970: 1_700_000_000)
        try FileManager.default.setAttributes([.modificationDate: fixedDate], ofItemAtPath: path.path)
        _ = try await workspace.scanRoots(roots, replacingGroup: "test", rootIDs: ["a", "b"], mode: .fileListAndMetadata)
        try await workspace.pauseWatching(rootIDs: ["a"])
        try await workspace.pauseWatching(rootIDs: ["a"])
        let paused = try await workspace.isWatchingPaused(rootID: "a")
        XCTAssertTrue(paused)
        let previous = try FileManager.default.attributesOfItem(atPath: path.path)
        try writeScript(path, id: "a", version: "2.0")
        try FileManager.default.setAttributes([.modificationDate: fixedDate], ofItemAtPath: path.path)
        let current = try FileManager.default.attributesOfItem(atPath: path.path)
        XCTAssertEqual(previous[.size] as? Int, current[.size] as? Int)
        XCTAssertEqual(previous[.modificationDate] as? Date, current[.modificationDate] as? Date)
        let result = try await workspace.resumeWatching(rootIDs: ["a"])
        XCTAssertEqual(result.roots.map(\.rootID), ["a"])
        XCTAssertEqual(result.catalogSnapshot?.fileItems.first { $0.scriptID == "a" }?.version, "2.0")
        let resumed = try await workspace.isWatchingPaused(rootID: "a")
        XCTAssertFalse(resumed)
        await workspace.shutdown()
    }

    func testIncompleteBatchRemainsPausedAndCanBeRetried() async throws {
        let (parent, roots) = try fixture()
        defer { try? FileManager.default.removeItem(at: parent) }
        let workspace = ScriptMetaKitWorkspace()
        try await workspace.registerRoots(roots, replacingGroup: "test")
        try await workspace.pauseWatching(rootIDs: ["a", "b"])
        let moved = parent.appendingPathComponent("moved-a")
        try FileManager.default.moveItem(at: roots[0].url, to: moved)
        let incomplete = try await workspace.resumeWatching(rootIDs: ["a", "b"])
        XCTAssertEqual(incomplete.roots.first { $0.rootID == "a" }?.status, "missing")
        for id in ["a", "b"] {
            let paused = try await workspace.isWatchingPaused(rootID: id)
            XCTAssertTrue(paused)
        }
        try FileManager.default.moveItem(at: moved, to: roots[0].url)
        let result = try await workspace.resumeWatching(rootIDs: ["a", "b"])
        XCTAssertTrue(result.roots.allSatisfy { $0.status == "ready" })
        for id in ["a", "b"] {
            let paused = try await workspace.isWatchingPaused(rootID: id)
            XCTAssertFalse(paused)
        }
        await workspace.shutdown()
    }

    func testWatchStreamKeepsOtherRootActiveAcrossPauseAndResume() async throws {
        let (parent, roots) = try fixture()
        defer { try? FileManager.default.removeItem(at: parent) }
        let workspace = ScriptMetaKitWorkspace(configuration: .init(nativeEventLatencyMillis: 20))
        let stream = try await workspace.watchUpdates(roots: roots, replacingGroup: "test", dirtyOnly: true)
        let ready = expectation(description: "initial reconciliation")
        let otherRootChanged = expectation(description: "other root changes while a is paused")
        let afterResume = expectation(description: "same stream continues after resume")
        let state = WatchObservations()
        let consumer = Task {
            do {
                for try await update in stream {
                    if !state.receivedInitial && update.coversAllWatchedRoots {
                        state.receivedInitial = true
                        ready.fulfill()
                    }
                    if state.pauseIsActive {
                        XCTAssertFalse(update.result.roots.contains { $0.rootID == "a" })
                        XCTAssertFalse(update.pendingRootIDs.contains("a"))
                    }
                    let paths = Set(update.result.scriptFileEntries.map(\.displayPath))
                    if !state.receivedOther && paths.contains(roots[1].url.appendingPathComponent("during.jsx").path) {
                        state.receivedOther = true
                        otherRootChanged.fulfill()
                    }
                    if !state.receivedAfter && paths.contains(roots[1].url.appendingPathComponent("after.jsx").path) {
                        state.receivedAfter = true
                        afterResume.fulfill()
                    }
                }
            } catch is CancellationError {
            } catch {
                XCTFail("Unexpected watch error: \(error)")
            }
            state.streamEnded = true
        }
        defer { consumer.cancel() }
        await fulfillment(of: [ready], timeout: 5)
        try await workspace.pauseWatching(rootIDs: ["a"])
        state.pauseIsActive = true
        try writeScript(roots[0].url.appendingPathComponent("added.jsx"), id: "a-added", version: "1.0")
        try writeScript(roots[1].url.appendingPathComponent("during.jsx"), id: "during", version: "1.0")
        await fulfillment(of: [otherRootChanged], timeout: 5)
        XCTAssertFalse(state.streamEnded)
        state.pauseIsActive = false
        let result = try await workspace.resumeWatching(rootIDs: ["a"])
        XCTAssertTrue(result.scriptFileEntries.contains { $0.displayPath == roots[0].url.appendingPathComponent("added.jsx").path })
        try writeScript(roots[1].url.appendingPathComponent("after.jsx"), id: "after", version: "1.0")
        await fulfillment(of: [afterResume], timeout: 5)
        XCTAssertFalse(state.streamEnded)
        await workspace.stopWatching()
        await consumer.value
    }

    func testEmptyAndUnknownIDsDoNotChangePauseState() async throws {
        let (parent, roots) = try fixture()
        defer { try? FileManager.default.removeItem(at: parent) }
        let engine = ScriptMetaKitEngine()
        try await engine.replaceRootGroup(roots, groupID: "test")
        for ids in [[], ["a", "unknown"]] {
            do { try await engine.pauseWatching(rootIDs: ids); XCTFail("Invalid IDs were accepted") }
            catch { }
        }
        let unpaused = try await engine.isWatchingPaused(rootID: "a")
        XCTAssertFalse(unpaused)
        try await engine.pauseWatching(rootIDs: ["a"])
        do { _ = try await engine.resumeWatching(rootIDs: ["a", "unknown"]); XCTFail("Unknown ID was accepted") }
        catch { }
        let paused = try await engine.isWatchingPaused(rootID: "a")
        XCTAssertTrue(paused)
        await engine.shutdown()
    }

    func testWatcherReconfigurationFailurePreservesPauseState() async throws {
        let (parent, roots) = try fixture()
        defer { try? FileManager.default.removeItem(at: parent) }
        let workspace = ScriptMetaKitWorkspace()
        try await workspace.startWatching(roots: roots, replacingGroup: "test", onChange: {})
        try await workspace.failNextWatcherStartForTesting()
        do { try await workspace.pauseWatching(rootIDs: ["a"]); XCTFail("Injected failure was ignored") }
        catch { }
        let before = try await workspace.isWatchingPaused(rootID: "a")
        XCTAssertFalse(before)
        try await workspace.pauseWatching(rootIDs: ["a"])
        try await workspace.failNextWatcherStartForTesting()
        do { _ = try await workspace.resumeWatching(rootIDs: ["a"]); XCTFail("Injected failure was ignored") }
        catch { }
        let after = try await workspace.isWatchingPaused(rootID: "a")
        XCTAssertTrue(after)
        await workspace.stopWatching()
    }

    func testStaleWatchResultCannotOverwriteACompletedBatchResume() async throws {
        let (parent, roots) = try fixture()
        defer { try? FileManager.default.removeItem(at: parent) }
        let workspace = ScriptMetaKitWorkspace()
        let gate = PublicationGate()
        let readFinished = expectation(description: "old read is awaiting publication")
        await workspace.beforeNextWatchPublicationForTesting {
            readFinished.fulfill()
            await gate.wait()
        }
        let stream = try await workspace.watchUpdates(roots: roots, replacingGroup: "test")
        let fresh = expectation(description: "current content published")
        let state = WatchObservations()
        let consumer = Task {
            do {
                for try await update in stream {
                    for item in update.result.fileItems where item.scriptID == "a" || item.scriptID == "b" {
                        XCTAssertEqual(item.version, "2.0", "pre-pause result was published after resume")
                    }
                    if !state.receivedInitial {
                        XCTAssertEqual(update.sequence, 1, "discarding stale results must not manufacture a sequence gap")
                        state.receivedInitial = true
                        fresh.fulfill()
                    }
                }
            } catch is CancellationError { } catch { XCTFail("\(error)") }
        }
        defer { consumer.cancel() }
        await fulfillment(of: [readFinished], timeout: 5)
        try await workspace.pauseWatching(rootIDs: ["a", "b"])
        for root in roots { try writeScript(root.url.appendingPathComponent("script.jsx"), id: root.rootID, version: "2.0") }
        let result = try await workspace.resumeWatching(rootIDs: ["a", "b"])
        XCTAssertTrue(result.roots.allSatisfy { $0.status == "ready" })
        await gate.open()
        await fulfillment(of: [fresh], timeout: 5)
        await workspace.stopWatching()
        await consumer.value
    }

    func testAlreadyCancelledCallsCannotChangeSuspension() async throws {
        let (parent, roots) = try fixture()
        defer { try? FileManager.default.removeItem(at: parent) }
        let engine = ScriptMetaKitEngine()
        try await engine.replaceRootGroup(roots, groupID: "test")
        let pause = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            try await engine.pauseWatching(rootIDs: ["a", "b"])
        }
        do { try await pause.value; XCTFail("Cancelled pause succeeded") } catch is CancellationError { }
        for root in roots { let paused = try await engine.isWatchingPaused(rootID: root.rootID); XCTAssertFalse(paused) }
        try await engine.pauseWatching(rootIDs: ["a", "b"])
        let resume = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            return try await engine.resumeWatching(rootIDs: ["a", "b"])
        }
        do { _ = try await resume.value; XCTFail("Cancelled resume succeeded") } catch is CancellationError { }
        for root in roots { let paused = try await engine.isWatchingPaused(rootID: root.rootID); XCTAssertTrue(paused) }
        await engine.shutdown()
    }

    func testCancellationDuringNativeHandoffKeepsTheWholeBatchPaused() async throws {
        let (parent, roots) = try fixture()
        defer { try? FileManager.default.removeItem(at: parent) }
        let workspace = ScriptMetaKitWorkspace()
        try await workspace.startWatching(roots: roots, replacingGroup: "test", onChange: {})
        try await workspace.pauseWatching(rootIDs: ["a", "b"])
        for root in roots { try writeScript(root.url.appendingPathComponent("script.jsx"), id: root.rootID, version: "2.0") }
        try await workspace.cancelNextWatchResumeForTesting()
        let cancelled = try await workspace.resumeWatching(rootIDs: ["a", "b"])
        XCTAssertTrue(cancelled.roots.allSatisfy { $0.status == "cancelled" })
        for root in roots { let paused = try await workspace.isWatchingPaused(rootID: root.rootID); XCTAssertTrue(paused) }
        let retried = try await workspace.resumeWatching(rootIDs: ["a", "b"])
        XCTAssertEqual(retried.fileItems.count, 2)
        XCTAssertTrue(retried.fileItems.allSatisfy { $0.version == "2.0" })
        await workspace.stopWatching()
    }

    func testConcurrentDisjointRootOperationsPreserveEachResult() async throws {
        let (parent, roots) = try fixture()
        defer { try? FileManager.default.removeItem(at: parent) }
        let workspace = ScriptMetaKitWorkspace()
        try await workspace.startWatching(roots: roots, replacingGroup: "test", onChange: {})
        async let pauseA: Void = workspace.pauseWatching(rootIDs: ["a"])
        async let pauseB: Void = workspace.pauseWatching(rootIDs: ["b"])
        _ = try await (pauseA, pauseB)
        for root in roots { let paused = try await workspace.isWatchingPaused(rootID: root.rootID); XCTAssertTrue(paused) }
        for root in roots { try writeScript(root.url.appendingPathComponent("script.jsx"), id: root.rootID, version: "2.0") }
        async let resumeA = workspace.resumeWatching(rootIDs: ["a"])
        async let resumeB = workspace.resumeWatching(rootIDs: ["b"])
        let (a, b) = try await (resumeA, resumeB)
        XCTAssertEqual(a.roots.map(\.rootID), ["a"])
        XCTAssertEqual(b.roots.map(\.rootID), ["b"])
        for result in [a, b] { XCTAssertTrue(result.fileItems.allSatisfy { $0.version == "2.0" }) }
        for root in roots { let paused = try await workspace.isWatchingPaused(rootID: root.rootID); XCTAssertFalse(paused) }
        await workspace.stopWatching()
    }

    func testRepeatingAPauseDoesNotDiscardAnotherRootsPendingPublication() async throws {
        let (parent, roots) = try fixture()
        defer { try? FileManager.default.removeItem(at: parent) }
        let workspace = ScriptMetaKitWorkspace()
        try await workspace.registerRoots(roots, replacingGroup: "test")
        try await workspace.pauseWatching(rootIDs: ["a"])
        let gate = PublicationGate()
        let pending = expectation(description: "root b awaiting publication")
        await workspace.beforeNextWatchPublicationForTesting { pending.fulfill(); await gate.wait() }
        let stream = try await workspace.watchUpdates(roots: roots, replacingGroup: "test", dirtyOnly: true)
        let published = expectation(description: "root b remains publishable")
        let consumer = Task {
            do {
                var iterator = stream.makeAsyncIterator()
                let update = try await iterator.next()
                XCTAssertEqual(update?.result.roots.map(\.rootID), ["b"])
                published.fulfill()
            } catch { XCTFail("\(error)") }
        }
        defer { consumer.cancel() }
        await fulfillment(of: [pending], timeout: 5)
        try await workspace.pauseWatching(rootIDs: ["a"])
        await gate.open()
        await fulfillment(of: [published], timeout: 5)
        await workspace.stopWatching()
        await consumer.value
    }
}
