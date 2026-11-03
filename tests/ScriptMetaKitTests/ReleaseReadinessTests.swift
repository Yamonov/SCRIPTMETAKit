import Foundation
import XCTest
@testable import ScriptMetaKit

final class ReleaseReadinessTests: XCTestCase {
    func testLanguageAndVersionCacheMismatchesDoNotEmitFailureDiagnostics() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("KitCacheCompatibility-\(UUID().uuidString)")
        let scripts = directory.appendingPathComponent("scripts")
        try FileManager.default.createDirectory(at: scripts, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        try "/*\nSCRIPTMETA-BEGIN\nScript-ID=quiet-cache\nMeta-Default-language=ja\nName=日本語\nName[en]=English\nSCRIPTMETA-END\n*/"
            .write(to: scripts.appendingPathComponent("sample.jsx"), atomically: true, encoding: .utf8)
        let store = ScriptMetaKitPersistentCacheStore(directoryURL: directory.appendingPathComponent("cache"))
        let diagnostics = ReleaseDiagnosticRecorder()
        let workspace = ScriptMetaKitWorkspace(configuration: .init(cacheStore: store,
            diagnosticHandler: { diagnostics.append($0) }, preferredLanguages: ["ja"]))
        let root = ScriptMetaKitRoot(rootID: "root", url: scripts)
        _ = try await workspace.scanRoots([root], replacingGroup: "test", rootIDs: ["root"], mode: .fileListAndMetadata, cacheScope: .all)
        try await workspace.setPreferredLanguages(["en"])
        let english = try await workspace.scanRoots([root], replacingGroup: "test", rootIDs: ["root"], mode: .fileListAndMetadata, cacheScope: .all)
        XCTAssertEqual(english.allItems.first?.name, "English")
        await workspace.clearVolatileState()
        let cacheURL = store.writableCacheFileURL(scope: .all)
        var payload = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: cacheURL)) as? [String: Any])
        var schema = try XCTUnwrap(payload["schema"] as? [String: Any])
        schema["metadata_projection_version"] = 0
        payload["schema"] = schema
        try JSONSerialization.data(withJSONObject: payload).write(to: cacheURL)
        let restored = try await workspace.scanRoots([root], replacingGroup: "test", rootIDs: ["root"], mode: .fileListAndMetadata, cacheScope: .all)
        XCTAssertEqual(restored.allItems.first?.name, "English")
        XCTAssertTrue(diagnostics.codes.isEmpty)
        await workspace.shutdown()
    }

    @MainActor
    func testMaximumDrainDelayCanBeCancelledWithoutOverflow() async throws {
        let workspace = ScriptMetaKitWorkspace()
        let started = expectation(description: "Drain task started")
        let finished = expectation(description: "Drain cancellation returned")
        let task = Task {
            started.fulfill()
            do {
                try await workspace.drainWatchChanges(initialDelayMillis: .max)
                XCTFail("Expected cancellation")
            } catch is CancellationError {
            } catch {
                XCTFail("Unexpected drain error: \(error)")
            }
            finished.fulfill()
        }
        await fulfillment(of: [started], timeout: 2)
        task.cancel()
        await fulfillment(of: [finished], timeout: 2)
    }

    func testLightweightCacheRevisionsMatchFullCachedResults() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("KitCacheRevisions-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        try "// SCRIPTMETA-BEGIN\n// Script-ID=revisions\n// Name=Revisions\n// SCRIPTMETA-END\n"
            .write(to: directory.appendingPathComponent("sample.jsx"), atomically: true, encoding: .utf8)
        let engine = ScriptMetaKitEngine()
        let scanned = try await engine.scan(folderURL: directory, checkUpdates: false)
        let ids = scanned.roots.map(\.rootID)
        for mode in [ScriptMetaScanMode.fileListOnly, .metadataOnly, .fileListAndMetadata] {
            let revisions = try await engine.cacheRevisions(rootIDs: ids, mode: mode)
            let full = try await engine.cachedRoots(rootIDs: ids, mode: mode)
            XCTAssertEqual(revisions.count, full.roots.count)
            for value in revisions {
                XCTAssertEqual(value.stateRevision, full.roots.first { $0.rootID == value.rootID }?.stateRevision)
                XCTAssertEqual(value.fileListRevision, full.fileListSnapshots.first { $0.root.rootID == value.rootID && $0.children != nil }?.contentRevision)
            }
        }
    }

    @MainActor
    func testDroppingWorkspaceFinishesItsWatchStream() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("KitReleaseLifetime-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        try "// SCRIPTMETA-BEGIN\n// Script-ID=lifetime\n// Name=Lifetime\n// SCRIPTMETA-END\n"
            .write(to: directory.appendingPathComponent("sample.jsx"), atomically: true, encoding: .utf8)
        var workspace: ScriptMetaKitWorkspace? = ScriptMetaKitWorkspace()
        weak var releasedWorkspace = workspace
        defer { releasedWorkspace = nil }
        let updates = try await workspace!.watchUpdates(
            roots: [ScriptMetaKitRoot(rootID: "root", url: directory, watchPolicy: .allRegistered)], replacingGroup: "test"
        )
        let ready = expectation(description: "Initial reconciliation delivered")
        let finished = expectation(description: "Dropping the workspace finishes the stream")
        let consumer = Task {
            var first = true
            do {
                for try await _ in updates {
                    if first { first = false; ready.fulfill() }
                }
            } catch is CancellationError {
            } catch {
                XCTFail("Unexpected stream error: \(error)")
            }
            finished.fulfill()
        }
        defer { consumer.cancel() }
        await fulfillment(of: [ready], timeout: 5)
        workspace = nil
        await fulfillment(of: [finished], timeout: 2)
        XCTAssertNil(releasedWorkspace)
    }
}

private nonisolated final class ReleaseDiagnosticRecorder: @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String] = []
    func append(_ diagnostic: ScriptMetaKitDiagnostic) {
        lock.lock()
        values.append(diagnostic.code)
        lock.unlock()
    }
    var codes: [String] {
        lock.lock()
        defer { lock.unlock() }
        return values
    }
}
