import XCTest
@testable import ScriptMetaKit

final class ScriptMetaKitAPITests: XCTestCase {
    func testOperationalPolicyPresetsAndCacheLimitArePublic() async throws {
        XCTAssertEqual(ScriptMetaKitPersistentCacheStore.defaultMaximumCacheBytes, 64 * 1024 * 1024)
        XCTAssertLessThan(
            ScriptMetaKitOperationalPolicy.lowImpact.maxConcurrentMetaURLChecks,
            ScriptMetaKitOperationalPolicy.balanced.maxConcurrentMetaURLChecks
        )
        XCTAssertLessThan(
            ScriptMetaKitOperationalPolicy.interactive.watcherDebounceDelayMillis,
            ScriptMetaKitOperationalPolicy.balanced.watcherDebounceDelayMillis
        )

        let engine = ScriptMetaKitEngine()
        try await engine.setOperationalPolicy(.lowImpact)
        var invalid = ScriptMetaKitOperationalPolicy.balanced
        invalid.maxConcurrentMetaURLChecks = 0
        do {
            try await engine.setOperationalPolicy(invalid)
            XCTFail("invalid operational policy was accepted")
        } catch let ScriptMetaKitError.operationFailed(status, _) {
            XCTAssertEqual(status, 3)
        }
    }

    func testSafeEditSessionRejectsExternalChangeAndUnconditionalAPIIsExplicit() async throws {
        let root = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.edit-session")
        defer { try? FileManager.default.removeItem(at: root) }
        let script = root.appendingPathComponent("sample.jsx")
        let engine = ScriptMetaKitEngine()
        let session = try await engine.beginScriptMetadataEditSession(fileURL: script)
        try "alert('changed outside');\n".write(to: script, atomically: true, encoding: .utf8)
        var draft = session.draft
        draft.scriptID = "com.example.swiftapi.edit-session"
        draft.version = "2.0.0"

        do {
            _ = try await engine.commitScriptMetadataEditSession(session, draft: draft)
            XCTFail("stale edit session overwrote an external change")
        } catch is ScriptMetaKitError {
            // Expected.
        }

        _ = try await engine.writeScriptMetadataUnconditionally(fileURL: script, draft: draft)
        XCTAssertTrue(
            try String(contentsOf: script, encoding: .utf8).contains("Version=2.0.0")
        )
    }

    func testIdleExplicitCancellationDoesNotAffectTheNextOperation() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitIdleCancel-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let script = root.appendingPathComponent("Example.jsx")
        try "alert('ok');\n".write(to: script, atomically: false, encoding: .utf8)

        let engine = ScriptMetaKitEngine()
        _ = try await engine.scan(folderURL: root, checkUpdates: false)
        engine.cancelCurrentOperation()
        let result = try await engine.scan(folderURL: root, checkUpdates: false)

        XCTAssertFalse(try XCTUnwrap(result.operation).cancelled)
        XCTAssertTrue(result.flattenedFileEntries.contains { $0.displayPath == script.path })
    }

    func testParentTaskCancellationStopsAnActiveScan() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitCancellation-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        for index in 0..<4_000 {
            try "alert(\(index));\n".write(
                to: root.appendingPathComponent("script-\(index).jsx"),
                atomically: false,
                encoding: .utf8
            )
        }

        let engine = ScriptMetaKitEngine()
        let scanTask = Task {
            try await engine.scan(folderURL: root, checkUpdates: false)
        }
        try await Task.sleep(nanoseconds: 1_000_000)
        scanTask.cancel()

        do {
            _ = try await scanTask.value
            XCTFail("cancelled parent task returned a scan result")
        } catch is CancellationError {
            // Expected.
        }
    }

    func testShutdownCanCancelTheCurrentOperation() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitShutdownCancel-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        for index in 0..<3_000 {
            try "alert(\(index));\n".write(
                to: root.appendingPathComponent("script-\(index).jsx"),
                atomically: false,
                encoding: .utf8
            )
        }

        let engine = ScriptMetaKitEngine()
        let scan = Task { try await engine.scan(folderURL: root, checkUpdates: false) }
        try await Task.sleep(for: .milliseconds(1))
        let clock = ContinuousClock()
        let started = clock.now
        await engine.shutdown(cancelCurrentOperation: true)
        let elapsed = started.duration(to: clock.now)
        do {
            let result = try await scan.value
            XCTAssertTrue(result.operation?.cancelled ?? false)
        } catch is CancellationError {
            // Expected.
        }
        XCTAssertLessThan(elapsed, .seconds(1))
    }

    func testRegisteredPathResolutionUsesThePublicSwiftWrapper() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitPathResolution-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let target = root.appendingPathComponent("target.jsx")
        let link = root.appendingPathComponent("alias.jsx")
        try "alert('target');\n".write(to: target, atomically: true, encoding: .utf8)
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: target)

        let followed = try XCTUnwrap(
            ScriptMetaScriptFilePolicy.resolveRegisteredPath(link, followSymlinks: true)
        )
        XCTAssertEqual(followed.pathKind, "symlink")
        XCTAssertEqual(followed.resolutionStatus, "resolved")
        XCTAssertEqual(
            followed.resolvedURL.resolvingSymlinksInPath(),
            target.resolvingSymlinksInPath()
        )

        let unfollowed = try XCTUnwrap(
            ScriptMetaScriptFilePolicy.resolveRegisteredPath(link, followSymlinks: false)
        )
        XCTAssertEqual(unfollowed.pathKind, "symlink")
        XCTAssertEqual(unfollowed.resolutionStatus, "not_requested")
        XCTAssertEqual(unfollowed.resolvedURL.standardizedFileURL, link.standardizedFileURL)
    }

    func testRuntimeVersionIsOne() {
        XCTAssertEqual(ScriptMetaKitRuntime.apiVersion, 1)
        XCTAssertEqual(ScriptMetaKitRuntime.packageVersion, "1.1.1")
    }

    func testRuntimeAcknowledgementsContainKitAndRustDependencyLicenses() {
        let acknowledgements = ScriptMetaKitRuntime.acknowledgementsText

        XCTAssertFalse(acknowledgements.isEmpty)
        XCTAssertTrue(acknowledgements.contains("SCRIPTMETAKit Acknowledgements"))
        XCTAssertTrue(acknowledgements.contains("Rust Dependency Licenses"))
        XCTAssertTrue(acknowledgements.contains("Apache License"))
        XCTAssertTrue(acknowledgements.contains("- reqwest "))
    }

    func testVersionAndEditPasswordUtilityAPIsArePublic() throws {
        XCTAssertEqual(try ScriptMetaKitEngine.normalizeVersionString("v1. 2 .3"), "1.2.3")
        XCTAssertNil(try ScriptMetaKitEngine.normalizeVersionString("version x"))
        XCTAssertTrue(try ScriptMetaKitEngine.validateVersionString("build 12 beta"))
        XCTAssertFalse(try ScriptMetaKitEngine.validateVersionString("version x"))
        XCTAssertEqual(try ScriptMetaKitEngine.compareVersions("1.0b", "1.0A"), .greater)
        XCTAssertEqual(try ScriptMetaKitEngine.compareVersions("2.0", "2.0.1"), .less)
        XCTAssertEqual(try ScriptMetaKitEngine.compareVersions("1.2", "1.2.0").comparisonResult, .orderedSame)

        let stored = "salt:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        XCTAssertTrue(try ScriptMetaKitEngine.validateEditPasswordSHA256Format(stored))
        XCTAssertFalse(try ScriptMetaKitEngine.validateEditPasswordSHA256Format("invalid"))
    }

    func testDistributionMetadataRendererIsAvailableSynchronously() throws {
        let block = try ScriptMetaKitEngine.renderDistributionMetadata(records: [
            DistributionMetadataDraft(scriptID: "com.example.public", version: "1.2.3")
        ])

        XCTAssertTrue(block.contains("SCRIPTMETA-DIST-BEGIN"))
        XCTAssertTrue(block.contains("Script-ID=com.example.public"))
        XCTAssertTrue(block.contains("Version=1.2.3"))
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
        XCTAssertFalse(try XCTUnwrap(result.fileListSnapshots.first).directoryStates.isEmpty)
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

    func testConditionalMetadataWriteRejectsAnExternalChange() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitConflict-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }

        let script = root.appendingPathComponent("editable.jsx")
        try """
        /*
        SCRIPTMETA-BEGIN
        Script-ID=com.example.swiftapi.conflict
        Version=1.0
        SCRIPTMETA-END
        */
        alert("original");
        """.write(to: script, atomically: true, encoding: .utf8)

        let engine = ScriptMetaKitEngine()
        let readResult = try await engine.readScriptMetadataDraft(fileURL: script)
        XCTAssertEqual(readResult.draft.scriptID, "com.example.swiftapi.conflict")
        try "alert(\"changed outside ScriptMetaKit\");\n".write(
            to: script,
            atomically: true,
            encoding: .utf8
        )

        do {
            _ = try await engine.writeScriptMetadata(
                fileURL: script,
                draft: readResult.draft,
                mode: .insertOrReplace,
                expectedSourceFingerprint: readResult.sourceFingerprint
            )
            XCTFail("a stale edit must not overwrite the external change")
        } catch let ScriptMetaKitError.sourceConflict(message) {
            XCTAssertFalse(message.isEmpty)
        } catch let ScriptMetaKitError.operationFailed(status, message) {
            XCTFail("expected source conflict, received status \(status): \(message)")
        } catch {
            XCTFail("expected source conflict, received \(error)")
        }

        XCTAssertEqual(
            try String(contentsOf: script, encoding: .utf8),
            "alert(\"changed outside ScriptMetaKit\");\n"
        )
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

    func testReadScriptMetadataEditPreviewRejectsNegativeLimit() async throws {
        let script = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitNegativePreview-\(UUID().uuidString).jsx")
        try "alert('x');\n".write(to: script, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: script) }

        do {
            _ = try await ScriptMetaKitEngine().readScriptMetadataEditPreview(
                fileURL: script,
                maxBytes: -1
            )
            XCTFail("negative maxBytes must fail")
        } catch let ScriptMetaKitError.operationFailed(status, _) {
            XCTAssertEqual(status, 3)
        }
    }

    func testReadCompiledScriptMetadataEditPreviewThroughSwiftAPI() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitAPITests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }

        let script = root.appendingPathComponent("preview.scpt")
        try compileOSA(source: "display dialog \"hello\"\n", outputURL: script)

        let engine = ScriptMetaKitEngine()
        _ = try await engine.writeScriptMetadata(
            fileURL: script,
            draft: ScriptMetadataDraft(
                scriptID: "com.example.swiftapi.compiled-preview",
                version: "1.0"
            ),
            mode: .insertOrReplace
        )
        let result = try await engine.readScriptMetadataEditPreview(fileURL: script, maxBytes: 4096)

        XCTAssertTrue(result.previewText.contains("display dialog \"hello\""))
        XCTAssertTrue(result.previewText.contains("SCRIPTMETA-BEGIN"))
        XCTAssertGreaterThan(result.previewByteCount, 0)
        XCTAssertFalse(result.isTruncated)
        XCTAssertFalse(result.requiresFullRead)
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

    func testWorkspaceClearVolatileStatePreservesPersistentCache() async throws {
        let root = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.clearvolatile")
        let cacheDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitAPITests-Cache-\(UUID().uuidString)", isDirectory: true)
        defer {
            try? FileManager.default.removeItem(at: root)
            try? FileManager.default.removeItem(at: cacheDirectory)
        }

        let cacheStore = ScriptMetaKitPersistentCacheStore(directoryURL: cacheDirectory)
        let workspace = ScriptMetaKitWorkspace(configuration: ScriptMetaKitWorkspaceConfiguration(
            cacheStore: cacheStore
        ))
        let roots = [
            ScriptMetaKitRoot(
                rootID: "clear-volatile",
                url: root,
                purpose: .updateCheck,
                watchPolicy: .disabled,
                cachePolicy: .memoryAndPersistent,
                refreshPolicy: .scheduled,
                priority: .userInitiated
            )
        ]

        let scanned = try await workspace.scanCatalog(
            roots: roots,
            replacingGroup: "test.clearvolatile",
            rootIDs: roots.map(\.rootID),
            cacheScope: .catalog
        )
        XCTAssertEqual(scanned.allItems.map(\.scriptID), ["com.example.swiftapi.clearvolatile"])
        XCTAssertNotNil(cacheStore.readableCacheFileURL(scope: .catalog))

        await workspace.clearVolatileState()

        XCTAssertNotNil(cacheStore.readableCacheFileURL(scope: .catalog))
        let cached = try await workspace.cachedCatalogSnapshot(
            roots: roots,
            replacingGroup: "test.clearvolatile",
            rootIDs: roots.map(\.rootID),
            cacheScope: .catalog
        )
        XCTAssertEqual(cached?.allItems.map(\.scriptID), ["com.example.swiftapi.clearvolatile"])
    }

    func testWorkspaceReportsNonfatalCacheDiagnostics() async throws {
        let root = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.diagnostic")
        let cacheDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitDiagnostic-\(UUID().uuidString)", isDirectory: true)
        defer {
            try? FileManager.default.removeItem(at: root)
            try? FileManager.default.removeItem(at: cacheDirectory)
        }
        try FileManager.default.createDirectory(at: cacheDirectory, withIntermediateDirectories: true)
        let cacheStore = ScriptMetaKitPersistentCacheStore(directoryURL: cacheDirectory)
        try Data("{invalid".utf8).write(to: cacheStore.writableCacheFileURL(scope: .catalog))
        let recorder = DiagnosticRecorder()
        let workspace = ScriptMetaKitWorkspace(configuration: .init(
            cacheStore: cacheStore,
            diagnosticHandler: { diagnostic in
                Task { await recorder.append(diagnostic) }
            }
        ))

        _ = try await workspace.scanRoots(
            [ScriptMetaKitRoot(rootID: "diagnostic", url: root)],
            replacingGroup: "test.diagnostic",
            rootIDs: ["diagnostic"],
            mode: .metadataOnly,
            cacheScope: .catalog
        )
        for _ in 0..<20 {
            if await recorder.diagnostics.isEmpty == false {
                break
            }
            try await Task.sleep(for: .milliseconds(10))
        }
        let diagnostics = await recorder.diagnostics
        XCTAssertTrue(diagnostics.contains { $0.code == "cache_load_failed" })
    }

    func testWorkspaceSerializesConcurrentReplacementScansForTheSameGroup() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitWorkspaceRace-\(UUID().uuidString)", isDirectory: true)
        let directoryA = root.appendingPathComponent("A", isDirectory: true)
        let directoryB = root.appendingPathComponent("B", isDirectory: true)
        try FileManager.default.createDirectory(at: directoryA, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: directoryB, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.a\n// SCRIPTMETA-END\n"
            .write(to: directoryA.appendingPathComponent("A.jsx"), atomically: true, encoding: .utf8)
        try "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.b\n// SCRIPTMETA-END\n"
            .write(to: directoryB.appendingPathComponent("B.jsx"), atomically: true, encoding: .utf8)

        let workspace = ScriptMetaKitWorkspace()
        let rootA = ScriptMetaKitRoot(rootID: "A", url: directoryA)
        let rootB = ScriptMetaKitRoot(rootID: "B", url: directoryB)
        async let resultA = workspace.scanRoots(
            [rootA],
            replacingGroup: "shared",
            rootIDs: ["A"],
            mode: .metadataOnly
        )
        async let resultB = workspace.scanRoots(
            [rootB],
            replacingGroup: "shared",
            rootIDs: ["B"],
            mode: .metadataOnly
        )

        let (scanA, scanB) = try await (resultA, resultB)
        XCTAssertEqual(scanA.roots.map(\.rootID), ["A"])
        XCTAssertEqual(scanB.roots.map(\.rootID), ["B"])
        XCTAssertEqual(scanA.allItems.map(\.scriptID), ["com.example.a"])
        XCTAssertEqual(scanB.allItems.map(\.scriptID), ["com.example.b"])
    }

    func testCancelledWorkspaceWaiterReturnsBeforeTheActiveOperationFinishes() async throws {
        let workspace = ScriptMetaKitWorkspace()
        let (started, startedContinuation) = AsyncStream<Void>.makeStream()
        let (release, releaseContinuation) = AsyncStream<Void>.makeStream()
        let active = Task {
            try await workspace.performExclusiveOperation {
                startedContinuation.yield()
                for await _ in release {
                    break
                }
            }
        }
        var startedIterator = started.makeAsyncIterator()
        _ = await startedIterator.next()

        let waiter = Task {
            try await workspace.performExclusiveOperation {}
        }
        try await Task.sleep(for: .milliseconds(20))
        let delayedRelease = Task {
            try? await Task.sleep(for: .seconds(1))
            releaseContinuation.yield()
        }
        let clock = ContinuousClock()
        let cancelledAt = clock.now
        waiter.cancel()
        do {
            try await waiter.value
            XCTFail("cancelled waiter completed normally")
        } catch is CancellationError {
            // Expected.
        }
        let cancellationDelay = cancelledAt.duration(to: clock.now)

        delayedRelease.cancel()
        releaseContinuation.yield()
        try await active.value
        XCTAssertLessThan(cancellationDelay, .milliseconds(250))
    }

    func testWorkspaceCompositePartialScanReturnsTheCompleteRegisteredCatalog() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ScriptMetaKitCompositeScan-\(UUID().uuidString)", isDirectory: true)
        let directoryA = root.appendingPathComponent("A", isDirectory: true)
        let directoryB = root.appendingPathComponent("B", isDirectory: true)
        try FileManager.default.createDirectory(at: directoryA, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: directoryB, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.a\n// SCRIPTMETA-END\n"
            .write(to: directoryA.appendingPathComponent("A.jsx"), atomically: true, encoding: .utf8)
        try "// SCRIPTMETA-BEGIN\n// Script-ID=com.example.b\n// SCRIPTMETA-END\n"
            .write(to: directoryB.appendingPathComponent("B.jsx"), atomically: true, encoding: .utf8)

        let workspace = ScriptMetaKitWorkspace()
        let roots = [
            ScriptMetaKitRoot(rootID: "A", url: directoryA),
            ScriptMetaKitRoot(rootID: "B", url: directoryB),
        ]
        _ = try await workspace.scanRoots(
            roots,
            replacingGroup: "registered",
            rootIDs: ["A", "B"],
            mode: .metadataOnly
        )
        let result = try await workspace.scanRegisteredRoots(
            roots,
            replacingGroup: "registered",
            scanningRootIDs: ["A"],
            resultRootIDs: ["A", "B"],
            mode: .metadataOnly
        )

        XCTAssertEqual(Set(result.roots.map(\.rootID)), Set(["A", "B"]))
        XCTAssertEqual(Set(result.allItems.map(\.scriptID)), Set(["com.example.a", "com.example.b"]))
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
        let roots = makeWatchRoots(firstRoot: firstRoot, secondRoot: secondRoot)

        _ = try await workspace.scanRoots(
            roots,
            replacingGroup: "test.watch",
            rootIDs: roots.map(\.rootID),
            mode: .fileListAndMetadata
        )
        try await workspace.startWatching(
            roots: roots,
            replacingGroup: "test.watch",
            drainsInitialChanges: true,
            initialDrainDirtyOnly: true,
            onChange: {}
        )
        defer {
            Task {
                await workspace.stopWatching()
            }
        }

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
        XCTAssertEqual(result.roots.map(\.rootID), ["first"])
        XCTAssertNil(result.catalogSnapshot)
        XCTAssertNil(result.updateCheckResult)
        XCTAssertTrue(result.fileListSnapshots.first?.children?.contains { $0.name == "added.jsx" } ?? false)
    }

    func testWatchSequenceDeliversInitialReconcileAndFileChange() async throws {
        let root = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.sequence")
        defer { try? FileManager.default.removeItem(at: root) }
        let workspace = ScriptMetaKitWorkspace(configuration: .init(nativeEventLatencyMillis: 50))
        let sequence = try await workspace.watchChanges(
            roots: [ScriptMetaKitRoot(
                rootID: "sequence",
                url: root,
                watchPolicy: .allRegistered
            )],
            replacingGroup: "test.sequence",
            dirtyOnly: true
        )
        let recorder = WatchResultRecorder()
        let consumer = Task {
            do {
                for try await result in sequence {
                    await recorder.append(result)
                }
            } catch {
                await recorder.record(error)
            }
        }
        let initial = try await waitForRecordedWatchResult(recorder, at: 0)
        XCTAssertEqual(initial.roots.map(\.rootID), ["sequence"])

        try "alert('added');\n".write(
            to: root.appendingPathComponent("sequence-added.jsx"),
            atomically: true,
            encoding: .utf8
        )
        let changed = try await waitForRecordedWatchResult(recorder, at: 1)
        consumer.cancel()
        await workspace.stopWatching()
        XCTAssertTrue(
            changed.fileListSnapshots.first?.children?.contains {
                $0.name == "sequence-added.jsx"
            } ?? false
        )
    }

    func testRequiredWatchPlanRestartSchedulesAReconcile() async throws {
        let firstRoot = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.restart.first")
        let secondRoot = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.restart.second")
        defer {
            try? FileManager.default.removeItem(at: firstRoot)
            try? FileManager.default.removeItem(at: secondRoot)
        }
        let workspace = ScriptMetaKitWorkspace(configuration: .init(nativeEventLatencyMillis: 50))
        let first = ScriptMetaKitRoot(
            rootID: "restart-first",
            url: firstRoot,
            watchPolicy: .allRegistered
        )
        let second = ScriptMetaKitRoot(
            rootID: "restart-second",
            url: secondRoot,
            watchPolicy: .allRegistered
        )
        try await workspace.startWatching(
            roots: [first],
            replacingGroup: "test.restart",
            drainsInitialChanges: true,
            onChange: {}
        )
        try await workspace.registerRoots([first, second], replacingGroup: "test.restart")

        let reconciled = try await waitForWatchChange(workspace: workspace, dirtyOnly: true)
        await workspace.stopWatching()
        XCTAssertEqual(
            Set(reconciled.roots.map(\.rootID)),
            Set(["restart-first", "restart-second"])
        )
        XCTAssertTrue(
            reconciled.fileListSnapshots.contains {
                $0.root.rootID == "restart-second"
                    && ($0.children?.contains { $0.name == "sample.jsx" } ?? false)
            }
        )
    }

    func testDirtyOnlyWatchPollHandlesDeletedFile() async throws {
        let firstRoot = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.watch.delete.first")
        let secondRoot = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.watch.delete.second")
        defer {
            try? FileManager.default.removeItem(at: firstRoot)
            try? FileManager.default.removeItem(at: secondRoot)
        }

        let workspace = ScriptMetaKitWorkspace(configuration: ScriptMetaKitWorkspaceConfiguration(
            nativeEventLatencyMillis: 50
        ))
        let roots = makeWatchRoots(firstRoot: firstRoot, secondRoot: secondRoot)

        _ = try await workspace.scanRoots(
            roots,
            replacingGroup: "test.watch",
            rootIDs: roots.map(\.rootID),
            mode: .fileListAndMetadata
        )
        try await workspace.startWatching(
            roots: roots,
            replacingGroup: "test.watch",
            drainsInitialChanges: true,
            initialDrainDirtyOnly: true,
            onChange: {}
        )
        defer {
            Task {
                await workspace.stopWatching()
            }
        }

        try FileManager.default.removeItem(at: firstRoot.appendingPathComponent("sample.jsx"))

        let result = try await waitForWatchChange(workspace: workspace, dirtyOnly: true)
        await workspace.stopWatching()

        XCTAssertEqual(result.fileListSnapshots.map(\.root.rootID), ["first"])
        XCTAssertFalse(result.fileListSnapshots.first?.children?.contains { $0.name == "sample.jsx" } ?? true)
        XCTAssertEqual(result.changeSummary?.removedCount, 1)
    }

    func testDirtyOnlyWatchPollReturnsMultipleAffectedRoots() async throws {
        let firstRoot = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.watch.multi.first")
        let secondRoot = try makeTemporaryScriptRoot(scriptID: "com.example.swiftapi.watch.multi.second")
        defer {
            try? FileManager.default.removeItem(at: firstRoot)
            try? FileManager.default.removeItem(at: secondRoot)
        }

        let workspace = ScriptMetaKitWorkspace(configuration: ScriptMetaKitWorkspaceConfiguration(
            nativeEventLatencyMillis: 50
        ))
        let roots = makeWatchRoots(firstRoot: firstRoot, secondRoot: secondRoot)

        _ = try await workspace.scanRoots(
            roots,
            replacingGroup: "test.watch",
            rootIDs: roots.map(\.rootID),
            mode: .fileListAndMetadata
        )
        try await workspace.startWatching(
            roots: roots,
            replacingGroup: "test.watch",
            drainsInitialChanges: true,
            initialDrainDirtyOnly: true,
            onChange: {}
        )
        defer {
            Task {
                await workspace.stopWatching()
            }
        }

        try "alert('first');".write(
            to: firstRoot.appendingPathComponent("first-added.jsx"),
            atomically: true,
            encoding: .utf8
        )
        try "alert('second');".write(
            to: secondRoot.appendingPathComponent("second-added.jsx"),
            atomically: true,
            encoding: .utf8
        )

        let result = try await waitForWatchChange(workspace: workspace, dirtyOnly: true)
        await workspace.stopWatching()

        XCTAssertEqual(Set(result.fileListSnapshots.map(\.root.rootID)), Set(["first", "second"]))
        XCTAssertEqual(Set(result.roots.map(\.rootID)), Set(["first", "second"]))
        XCTAssertNil(result.catalogSnapshot)
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

    private func waitForRecordedWatchResult(
        _ recorder: WatchResultRecorder,
        at index: Int,
        attempts: Int = 50
    ) async throws -> ScriptMetaScanResult {
        for _ in 0..<attempts {
            if let result = await recorder.result(at: index) {
                return result
            }
            if let errorMessage = await recorder.errorMessage {
                XCTFail(errorMessage)
                throw CancellationError()
            }
            try await Task.sleep(for: .milliseconds(100))
        }
        XCTFail("watch sequence result \(index) was not observed")
        throw CancellationError()
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

    private func compileOSA(source: String, outputURL: URL) throws {
        let osacompileURL = URL(fileURLWithPath: "/usr/bin/osacompile")
        guard FileManager.default.isExecutableFile(atPath: osacompileURL.path) else {
            throw XCTSkip("osacompile is unavailable")
        }

        let process = Process()
        process.executableURL = osacompileURL
        process.arguments = ["-o", outputURL.path]

        let input = Pipe()
        let error = Pipe()
        process.standardInput = input
        process.standardError = error

        try process.run()
        input.fileHandleForWriting.write(Data(source.utf8))
        input.fileHandleForWriting.closeFile()
        process.waitUntilExit()

        if process.terminationStatus != 0 {
            let errorData = error.fileHandleForReading.readDataToEndOfFile()
            let message = String(data: errorData, encoding: .utf8) ?? "osacompile failed"
            XCTFail(message)
            throw NSError(
                domain: "ScriptMetaKitAPITests",
                code: Int(process.terminationStatus),
                userInfo: [NSLocalizedDescriptionKey: message]
            )
        }
    }

    private func makeWatchRoots(firstRoot: URL, secondRoot: URL) -> [ScriptMetaKitRoot] {
        [
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
    }
}

private actor WatchResultRecorder {
    private var results: [ScriptMetaScanResult] = []
    private(set) var errorMessage: String?

    func append(_ result: ScriptMetaScanResult) {
        results.append(result)
    }

    func record(_ error: Error) {
        errorMessage = String(describing: error)
    }

    func result(at index: Int) -> ScriptMetaScanResult? {
        results.indices.contains(index) ? results[index] : nil
    }
}

private actor DiagnosticRecorder {
    private(set) var diagnostics: [ScriptMetaKitDiagnostic] = []

    func append(_ diagnostic: ScriptMetaKitDiagnostic) {
        diagnostics.append(diagnostic)
    }
}
