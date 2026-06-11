import XCTest
import ScriptMetaKit

final class ScriptMetaKitAPITests: XCTestCase {
    func testRuntimeVersionIsOne() {
        XCTAssertEqual(ScriptMetaKitRuntime.apiVersion, 1)
        XCTAssertEqual(ScriptMetaKitRuntime.packageVersion, "1.0.4")
    }

    func testFolderScanFindsScriptMetadata() async throws {
        let root = try makeTemporaryScriptRoot()
        defer { try? FileManager.default.removeItem(at: root) }

        let engine = ScriptMetaKitEngine()
        let result = try await engine.scan(folderURL: root, checkUpdates: false)

        XCTAssertTrue(result.fileItems.contains { $0.scriptID == "com.example.swiftapi.scan" })
        XCTAssertTrue(result.flattenedFileEntries.contains { $0.name == "sample.jsx" && $0.hasScriptMeta })
        let fileEntry = try XCTUnwrap(result.flattenedFileEntries.first { $0.name == "sample.jsx" })
        XCTAssertFalse(try XCTUnwrap(fileEntry.identity).stableID.isEmpty)
        XCTAssertEqual(result.operation?.status, "finished")
        XCTAssertEqual(result.operation?.totalUnits, 1)
        XCTAssertTrue(result.fileIssues?.isEmpty ?? false)
    }

    func testRegisteredRootScanFindsScriptMetadata() async throws {
        let root = try makeTemporaryScriptRoot()
        defer { try? FileManager.default.removeItem(at: root) }

        let engine = ScriptMetaKitEngine()
        try await engine.setRoots([
            ScriptMetaKitRoot(
                rootID: "primary",
                url: root,
                displayName: "Primary",
                purpose: .fileListAndMetadata,
                watchPolicy: .visibleOnly,
                cachePolicy: .memoryAndPersistent,
                refreshPolicy: .onFileEventDeferred,
                priority: .userInitiated
            )
        ])

        let result = try await engine.scanRegisteredRoots(mode: .fileListAndMetadata, checkUpdates: false)

        XCTAssertEqual(result.roots.first?.rootID, "primary")
        XCTAssertTrue(result.fileItems.contains { $0.scriptID == "com.example.swiftapi.scan" })
    }

    func testRootPreflightOptionsAreConfigurable() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitAPITests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        for index in 0..<12 {
            try "plain text".write(
                to: root.appendingPathComponent("Document\(index).txt"),
                atomically: true,
                encoding: .utf8
            )
        }

        let engine = ScriptMetaKitEngine()
        let strictOptions = ScriptMetaRootPreflightOptions(
            maxScannedItems: 8,
            maxDurationMillis: 0,
            minScannedFileCountForLargeRoot: 8,
            minScriptRatioDenominator: 2,
            minScannedItemsForTimeLimit: 8
        )
        try await engine.setRootPreflightOptions(strictOptions)
        let rejected = try await engine.scan(folderURL: root, checkUpdates: false)

        XCTAssertEqual(rejected.roots.first?.status, "overflowed")
        XCTAssertEqual(rejected.roots.first?.error?.code, "too_large_for_script_folder")

        var disabledOptions = strictOptions
        disabledOptions.rejectLowScriptDensityLargeRoots = false
        try await engine.setRootPreflightOptions(disabledOptions)
        let accepted = try await engine.scan(folderURL: root, checkUpdates: false)

        XCTAssertEqual(accepted.roots.first?.status, "ready")
    }

    func testSelectedRootScanFindsOnlyRequestedRoot() async throws {
        let firstRoot = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.first")
        let secondRoot = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.second")
        defer {
            try? FileManager.default.removeItem(at: firstRoot)
            try? FileManager.default.removeItem(at: secondRoot)
        }

        let engine = ScriptMetaKitEngine()
        try await engine.setRoots([
            ScriptMetaKitRoot(rootID: "first", url: firstRoot),
            ScriptMetaKitRoot(rootID: "second", url: secondRoot)
        ])
        try await engine.setVisibleRoot("second")

        let result = try await engine.scanRoot(rootID: "second", mode: .fileListAndMetadata, checkUpdates: false)

        XCTAssertEqual(result.roots.map(\.rootID), ["second"])
        XCTAssertEqual(result.fileItems.map(\.scriptID), ["com.example.swiftapi.second"])
        try await engine.clearVisibleRoot()
    }

    func testSingleItemUpdateCheckReturnsSelectedStatus() async throws {
        let root = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.single")
        defer { try? FileManager.default.removeItem(at: root) }
        let dist = root.appendingPathComponent("SCRIPTMETA.txt")
        try """
        SCRIPTMETA-DIST-BEGIN
        Script-ID: com.example.swiftapi.single
        Latest-Version: 2.0.0
        SCRIPTMETA-DIST-END
        """.write(to: dist, atomically: true, encoding: .utf8)
        let script = root.appendingPathComponent("sample.jsx")
        try """
        // SCRIPTMETA-BEGIN
        // Script-ID: com.example.swiftapi.single
        // Version: 1.0.0
        // Name: Swift API Single
        // Meta-URL: \(dist.absoluteString)
        // SCRIPTMETA-END
        alert("test");
        """.write(to: script, atomically: true, encoding: .utf8)

        let engine = ScriptMetaKitEngine()
        let scanResult = try await engine.scan(folderURL: root, checkUpdates: false)
        let item = try XCTUnwrap(scanResult.fileItems.first { $0.scriptID == "com.example.swiftapi.single" })
        let updateResult = try await engine.checkUpdate(item: item)

        XCTAssertEqual(updateResult.statusesByItemID[item.filePath], "update_available")
        XCTAssertEqual(updateResult.statusesByItemID.count, 1)
    }

    func testBatchUpdateCheckReturnsSelectedStatuses() async throws {
        let root = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.batch.first")
        defer { try? FileManager.default.removeItem(at: root) }
        let dist = root.appendingPathComponent("SCRIPTMETA.txt")
        try """
        SCRIPTMETA-DIST-BEGIN
        Script-ID: com.example.swiftapi.batch.first
        Latest-Version: 2.0.0

        Script-ID: com.example.swiftapi.batch.second
        Latest-Version: 1.0.0
        SCRIPTMETA-DIST-END
        """.write(to: dist, atomically: true, encoding: .utf8)
        let secondScript = root.appendingPathComponent("second.jsx")
        try """
        // SCRIPTMETA-BEGIN
        // Script-ID: com.example.swiftapi.batch.second
        // Version: 1.0.0
        // Name: Swift API Batch Second
        // Meta-URL: \(dist.absoluteString)
        // SCRIPTMETA-END
        alert("test");
        """.write(to: secondScript, atomically: true, encoding: .utf8)
        let firstScript = root.appendingPathComponent("sample.jsx")
        try """
        // SCRIPTMETA-BEGIN
        // Script-ID: com.example.swiftapi.batch.first
        // Version: 1.0.0
        // Name: Swift API Batch First
        // Meta-URL: \(dist.absoluteString)
        // SCRIPTMETA-END
        alert("test");
        """.write(to: firstScript, atomically: true, encoding: .utf8)

        let engine = ScriptMetaKitEngine()
        let scanResult = try await engine.scan(folderURL: root, checkUpdates: false)
        let items = scanResult.fileItems.sorted { $0.scriptID < $1.scriptID }
        let updateResult = try await engine.checkUpdates(items: items)

        XCTAssertEqual(updateResult.statusesByItemID[firstScript.path], "update_available")
        XCTAssertEqual(updateResult.statusesByItemID[secondScript.path], "up_to_date")
        XCTAssertEqual(updateResult.statusesByItemID.count, 2)
    }

    func testScanWithUpdatesKeepsDuplicateScriptIDFileStatuses() async throws {
        let base = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitAPITests-\(UUID().uuidString)", isDirectory: true)
        let firstRoot = base.appendingPathComponent("First", isDirectory: true)
        let secondRoot = base.appendingPathComponent("Second", isDirectory: true)
        let distRoot = base.appendingPathComponent("Dist", isDirectory: true)
        try FileManager.default.createDirectory(at: firstRoot, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: secondRoot, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: distRoot, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: base) }

        let dist = distRoot.appendingPathComponent("SCRIPTMETA.txt")
        try """
        SCRIPTMETA-DIST-BEGIN
        Script-ID: com.example.swiftapi.duplicate
        Latest-Version: 2.0.0
        SCRIPTMETA-DIST-END
        """.write(to: dist, atomically: true, encoding: .utf8)

        let firstScript = firstRoot.appendingPathComponent("Duplicate.jsx")
        let secondScript = secondRoot.appendingPathComponent("Duplicate.jsx")
        for script in [firstScript, secondScript] {
            try """
            // SCRIPTMETA-BEGIN
            // Script-ID: com.example.swiftapi.duplicate
            // Version: 2.0.0
            // Name: Swift API Duplicate
            // Meta-URL: \(dist.absoluteString)
            // SCRIPTMETA-END
            alert("test");
            """.write(to: script, atomically: true, encoding: .utf8)
        }

        let engine = ScriptMetaKitEngine()
        let result = try await engine.scan(folderURLs: [firstRoot, secondRoot], checkUpdates: true)

        XCTAssertEqual(result.catalogSnapshot?.allItems.count, 1)
        XCTAssertEqual(result.fileItems.count, 2)
        XCTAssertEqual(result.updateCheckResult?.statusesByItemID[firstScript.path], "up_to_date")
        XCTAssertEqual(result.updateCheckResult?.statusesByItemID[secondScript.path], "up_to_date")
        XCTAssertEqual(result.updateCheckResult?.statusesByItemID.count, 2)
    }

    func testEditAndDistributionHelpersArePublic() async throws {
        let engine = ScriptMetaKitEngine()

        let draft = ScriptMetadataDraft(
            scriptID: "com.example.swiftapi.draft",
            version: "1.0.0",
            name: "Draft",
            author: "Author"
        )
        XCTAssertEqual(draft.scriptID, "com.example.swiftapi.draft")

        let block = try await engine.renderDistributionMetadata(records: [
            DistributionMetadataDraft(
                scriptID: "com.example.swiftapi.draft",
                version: "1.0.0",
                latestURL: "https://example.com/SCRIPTMETA.txt"
            )
        ])

        XCTAssertTrue(block.contains("SCRIPTMETA-DIST-BEGIN"))
        XCTAssertTrue(block.contains("Script-ID=com.example.swiftapi.draft"))
    }

    func testReadScriptMetadataDraftPreservesUnknownLines() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitAPITests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }

        let script = root.appendingPathComponent("editable.jsx")
        try """
        /*
        SCRIPTMETA-BEGIN
        Script-ID=com.example.swiftapi.editread
        Version=v1. 2
        Unknown-Key=keep
        Description-BEGIN
        Line 1
        Line 2
        Description-END
        SCRIPTMETA-END
        */
        alert("test");
        """.write(to: script, atomically: true, encoding: .utf8)

        let engine = ScriptMetaKitEngine()
        let result = try await engine.readScriptMetadataDraft(fileURL: script)

        XCTAssertTrue(result.hasExistingBlock)
        XCTAssertEqual(result.draft.scriptID, "com.example.swiftapi.editread")
        XCTAssertEqual(result.draft.version, "1.2")
        XCTAssertEqual(result.draft.itemDescription, "Line 1\nLine 2")
        XCTAssertEqual(result.commentStyle, "javascript_block")
        XCTAssertTrue(result.unknownLines.contains("Unknown-Key=keep"))
        XCTAssertTrue(result.existingLines.contains("Unknown-Key=keep"))
        XCTAssertFalse(result.sourceFingerprint.isEmpty)
    }

    func testReadScriptMetadataEditPreviewIsBounded() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitAPITests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }

        let script = root.appendingPathComponent("preview.jsx")
        let prefix = String(repeating: "alert('x');\n", count: 512)
        let source = """
        \(prefix)
        /*
        SCRIPTMETA-BEGIN
        Script-ID=com.example.swiftapi.preview
        SCRIPTMETA-END
        */
        """
        try source.write(to: script, atomically: true, encoding: .utf8)

        let engine = ScriptMetaKitEngine()
        let result = try await engine.readScriptMetadataEditPreview(fileURL: script, maxBytes: 128)

        XCTAssertEqual(result.previewByteCount, 128)
        XCTAssertEqual(result.fileSize, UInt64(source.utf8.count))
        XCTAssertEqual(result.commentStyle, "javascript_block")
        XCTAssertTrue(result.isTruncated)
        XCTAssertTrue(result.requiresFullRead)
        XCTAssertFalse(result.hasScriptmetaMarkerInPreview)
        XCTAssertFalse(result.fileStateFingerprint.isEmpty)
    }

    func testScanReportsObfuscatedEditStateThroughSwiftAPI() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitAPITests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }

        let script = root.appendingPathComponent("obfuscated.jsxbin")
        try "@JSXBIN@ES@2.0@script".write(to: script, atomically: true, encoding: .utf8)

        let engine = ScriptMetaKitEngine()
        engine.cancelCurrentOperation()
        let result = try await engine.scan(folderURL: root, checkUpdates: false)

        let entry = try XCTUnwrap(result.flattenedFileEntries.first { $0.displayPath == script.path })
        XCTAssertEqual(entry.scriptMetaEditState, "obfuscated")
        XCTAssertFalse(entry.canEditScriptMeta)
        XCTAssertFalse(entry.canAppendScriptMeta)
    }

    func testDirtyOnlyWatchPollReturnsAffectedRootSnapshot() async throws {
        let firstRoot = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.watch.first")
        let secondRoot = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.watch.second")
        defer {
            try? FileManager.default.removeItem(at: firstRoot)
            try? FileManager.default.removeItem(at: secondRoot)
        }

        let workspace = ScriptMetaKitWorkspace(configuration: ScriptMetaKitWorkspaceConfiguration(
            nativeEventLatencyMillis: 50
        ))
        let roots = [
            ScriptMetaKitRoot(
                rootID: "first",
                url: firstRoot,
                purpose: .fileListAndMetadata,
                watchPolicy: .allRegistered,
                refreshPolicy: .onFileEvent
            ),
            ScriptMetaKitRoot(
                rootID: "second",
                url: secondRoot,
                purpose: .fileListAndMetadata,
                watchPolicy: .allRegistered,
                refreshPolicy: .onFileEvent
            )
        ]

        _ = try await workspace.scanRoots(
            roots,
            replacingGroup: "test.watch",
            rootIDs: roots.map(\.rootID),
            mode: .fileListAndMetadata
        )
        try await workspace.startWatching(
            roots: roots,
            replacingGroup: "test.watch",
            onChange: {}
        )
        defer {
            Task {
                await workspace.stopWatching()
            }
        }
        try await drainWatchChanges(workspace: workspace, dirtyOnly: true)

        let addedScript = firstRoot.appendingPathComponent("added.jsx")
        try """
        // SCRIPTMETA-BEGIN
        // Script-ID: com.example.swiftapi.watch.added
        // Version: 1.0.0
        // SCRIPTMETA-END
        alert("test");
        """.write(to: addedScript, atomically: true, encoding: .utf8)

        let result = try await waitForWatchChange(workspace: workspace, dirtyOnly: true)
        await workspace.stopWatching()

        XCTAssertEqual(result.fileListSnapshots.map(\.root.rootID), ["first"])
        XCTAssertTrue(result.fileListSnapshots.first?.children?.contains { $0.name == "added.jsx" } ?? false)
    }

    private func waitForWatchChange(
        workspace: ScriptMetaKitWorkspace,
        dirtyOnly: Bool,
        attempts: Int = 40
    ) async throws -> ScriptMetaScanResult {
        for _ in 0..<attempts {
            if let result = try await workspace.pollWatchChanges(dirtyOnly: dirtyOnly) {
                return result
            }
            try await Task.sleep(nanoseconds: 150_000_000)
        }
        XCTFail("watch change was not observed")
        throw CancellationError()
    }

    private func drainWatchChanges(
        workspace: ScriptMetaKitWorkspace,
        dirtyOnly: Bool
    ) async throws {
        try await Task.sleep(nanoseconds: 300_000_000)
        var idlePolls = 0
        for _ in 0..<20 {
            if try await workspace.pollWatchChanges(dirtyOnly: dirtyOnly) == nil {
                idlePolls += 1
                if idlePolls >= 3 { return }
            } else {
                idlePolls = 0
            }
            try await Task.sleep(nanoseconds: 150_000_000)
        }
    }

    private func makeTemporaryScriptRoot(scriptID: String = "com.example.swiftapi.scan") throws -> URL {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitAPITests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)

        let script = root.appendingPathComponent("sample.jsx")
        try """
        // SCRIPTMETA-BEGIN
        // Script-ID: \(scriptID)
        // Version: 1.0.0
        // Name: Swift API Scan
        // SCRIPTMETA-END
        alert("test");
        """.write(to: script, atomically: true, encoding: .utf8)

        return root
    }
}
