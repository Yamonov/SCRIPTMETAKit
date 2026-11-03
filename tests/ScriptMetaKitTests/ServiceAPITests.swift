import XCTest
@testable import ScriptMetaKit

final class ServiceAPITests: XCTestCase {
    private let source = """
    /*
    SCRIPTMETA-BEGIN
    Script-ID=com.example.services
    Meta-Default-language=ja
    Name=日本語名
    Name[en]=English
    Name[fr]=Français
    Author=著者
    Tags=画像, image,
    export,
    Custom-Field=preserve me
    Description-BEGIN
    日本語本文
    [fr]Texte français
    Description-END
    SCRIPTMETA-END
    */
    // source stays unchanged
    """

    private func fixture() throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("ScriptMetaServices-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try source.write(to: root.appendingPathComponent("example.jsx"), atomically: true, encoding: .utf8)
        return root
    }

    func testTextParserHasPerFieldFallbackAndSelectedOnlyOutput() throws {
        let value = try ScriptMetaKitMetadata.parse(source, preferredLanguages: ["en", "fr"])
        XCTAssertEqual(value.name, "English")
        XCTAssertEqual(value.itemDescription, "Texte français")
        XCTAssertEqual(value.author, "著者")
        XCTAssertEqual(value.tags, ["画像", "image", "export"])
        XCTAssertEqual(value.presentation.nameLanguage, "en")
        XCTAssertEqual(value.presentation.descriptionLanguage, "fr")
        let serialized = String(decoding: try JSONEncoder().encode(value), as: UTF8.self)
        XCTAssertFalse(serialized.contains("日本語本文"))
        XCTAssertEqual(try ScriptMetaKitMetadata.parse(source).itemDescription, "日本語本文")
        XCTAssertEqual(try ScriptMetaKitMetadata.parse(source, preferredLanguages: ["es"]).name, "日本語名")
        XCTAssertThrowsError(try ScriptMetaKitMetadata.parse(source, preferredLanguages: ["jp"]))
        XCTAssertThrowsError(try ScriptMetaKitMetadata.parse("no metadata"))
    }

    func testTextEditingPreservesOtherLanguagesUnknownFieldsAndSource() throws {
        let read = try ScriptMetaKitMetadata.readDraft(from: source)
        XCTAssertEqual(read.commentStyle, "javascript_block")
        var draft = read.draft
        draft.extensions?.names["en"] = "Edited English"
        draft.extensions?.tags = ["image", "new"]
        let rewritten = try ScriptMetaKitMetadata.write(draft, into: source, mode: .replaceOnly)
        let reread = try ScriptMetaKitMetadata.readDraft(from: rewritten)
        XCTAssertEqual(reread.draft.extensions?.names["fr"], "Français")
        XCTAssertEqual(reread.draft.extensions?.names["en"], "Edited English")
        XCTAssertEqual(reread.draft.extensions?.tags, ["image", "new"])
        XCTAssertTrue(rewritten.contains("Custom-Field=preserve me"))
        XCTAssertTrue(rewritten.hasSuffix("// source stays unchanged"))
        XCTAssertTrue(reread.draft.itemDescription?.contains("日本語本文") == true)
        XCTAssertTrue(reread.draft.itemDescription?.contains("Texte français") == true)
        XCTAssertThrowsError(try ScriptMetaKitMetadata.write(draft, into: source, mode: .insertOnly))
        for style in [ScriptMetaCommentStyle.plain, .javaScriptBlock, .appleScriptBlock] {
            let rendered = try ScriptMetaKitMetadata.render(draft, style: style)
            XCTAssertEqual(try ScriptMetaKitMetadata.parse(rendered, preferredLanguages: ["en"]).name, "Edited English")
        }
    }

    func testValidationUsesKitRules() throws {
        XCTAssertEqual(try ScriptMetaKitMetadata.normalizeLanguageTag("EN-us"), "en-US")
        XCTAssertThrowsError(try ScriptMetaKitMetadata.normalizeLanguageTag("jp"))
        XCTAssertEqual(try ScriptMetaKitMetadata.normalizeTags([" image ", "image", "画像"]), ["image", "画像"])
        XCTAssertEqual(try ScriptMetaKitMetadata.parseTagList(" image, 画像, image, "), ["image", "画像"])
        XCTAssertTrue(try ScriptMetaKitMetadata.isValidTag("image editing"))
        XCTAssertFalse(try ScriptMetaKitMetadata.isValidTag("Name=example"))
        XCTAssertThrowsError(try ScriptMetaKitMetadata.normalizeTags(["Description-END"]))
        XCTAssertEqual(try ScriptMetaKitMetadata.normalizeURL("https://example.com/path"), "https://example.com/path")
        XCTAssertNil(try ScriptMetaKitMetadata.normalizeURL("javascript:alert(1)"))
    }

    func testTextAndFileEditingUseTheSameURLAndVersionRules() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let engine = ScriptMetaKitEngine()
        let file = root.appendingPathComponent("parity.jsx")
        let source = "// unchanged source"
        for url in ["", "example.com/metadata", "https:/example.com/metadata"] {
            let draft = ScriptMetadataDraft(scriptID: "parity", version: "", metaURL: url, name: "Example")
            let editedText = try ScriptMetaKitMetadata.write(draft, into: source)
            try source.write(to: file, atomically: true, encoding: .utf8)
            _ = try await engine.writeScriptMetadata(fileURL: file, draft: draft)
            let editedFile = try String(contentsOf: file, encoding: .utf8)
            XCTAssertEqual(editedText, editedFile)
        }
        let invalid = ScriptMetadataDraft(scriptID: "invalid", metaURL: "javascript:alert(1)")
        XCTAssertThrowsError(try ScriptMetaKitMetadata.render(invalid))
        do {
            _ = try await engine.writeScriptMetadata(fileURL: file, draft: invalid)
            XCTFail("File and text operations must reject the same invalid URL")
        } catch {}
    }

    func testTextRenderingProtectsCommentBoundariesAndRejectsMalformedFields() throws {
        var draft = ScriptMetadataDraft(scriptID: "boundaries", itemDescription: "Literal (* and *) and */", name: "Example")
        for (style, end) in [(ScriptMetaCommentStyle.javaScriptBlock, "*/"), (.appleScriptBlock, "*)")] {
            let rendered = try ScriptMetaKitMetadata.render(draft, style: style)
            XCTAssertEqual(rendered.components(separatedBy: end).count - 1, 1)
        }
        draft.itemDescription = "first\rDescription-END\rsecond"
        XCTAssertThrowsError(try ScriptMetaKitMetadata.render(draft))
        draft.itemDescription = nil
        draft.scriptID = "id\nVersion=9.9"
        XCTAssertThrowsError(try ScriptMetaKitMetadata.render(draft))
    }

    func testUnreadCompiledScriptStaysUnknownAcrossNativeCacheReuse() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("unread.scpt")
        try Data("synthetic, not decompiled".utf8).write(to: file)
        let engine = ScriptMetaKitEngine()
        for _ in 0..<3 {
            let result = try await engine.scan(folderURL: root, checkUpdates: false)
            let record = try XCTUnwrap(result.catalogSnapshot?.candidateCache.records.first { $0.filePath.hasSuffix("unread.scpt") })
            XCTAssertEqual(record.scriptMetaEditState, "unknown")
            XCTAssertEqual(record.canEditScriptMeta, false)
            XCTAssertEqual(record.canAppendScriptMeta, false)
        }
    }

    func testUpdateChecksUseOnlyUpdateInputsAndKeepDisplayMetadata() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let distribution = root.appendingPathComponent("distribution.txt")
        try "SCRIPTMETA-DIST-BEGIN\nScript-ID=com.example.services\nVersion=2.0\nSCRIPTMETA-DIST-END".write(to: distribution, atomically: true, encoding: .utf8)
        let engine = ScriptMetaKitEngine()
        let scanned = try await engine.scan(folderURL: root, checkUpdates: false)
        var item = try XCTUnwrap(scanned.allItems.first)
        item.version = "1.0"
        item.metaURL = distribution.absoluteString
        item.itemDescription = String(repeating: "Large display-only text. ", count: 40_000)
        item.minTargetVersion = "unused display metadata"
        item.runtimeKind = "unused runtime"
        item.scriptMetaEditState = "unused state"
        let single = try await engine.checkUpdate(item: item)
        let batch = try await engine.checkUpdates(items: [item])
        XCTAssertEqual(single.statusesByItemID[item.filePath], "update_available")
        XCTAssertEqual(batch.statusesByItemID[item.filePath], "update_available")
        let cached = try await engine.cachedRoots(rootIDs: scanned.roots.map(\.rootID))
        XCTAssertEqual(cached.allItems.first?.itemDescription, scanned.allItems.first?.itemDescription)
    }

    func testDistributionParsingAndRuntimeInfoComeFromNativeKit() throws {
        let text = "SCRIPTMETA-DIST-BEGIN\nScript-ID=a\nVersion=2.0\nLatest-URL=https://example.com/a\nScript-ID=b\nVersion=3.0\nSCRIPTMETA-DIST-END"
        XCTAssertEqual(try ScriptMetaKitMetadata.parseDistribution(text).count, 2)
        XCTAssertEqual(try ScriptMetaKitMetadata.parseDistribution(text, scriptID: "b").first?.latestVersion, "3.0")
        let unresolved = try XCTUnwrap(ScriptMetaKitMetadata.parseDistribution(text, scriptID: "missing").first)
        XCTAssertNil(unresolved.latestVersion)
        XCTAssertTrue(unresolved.note?.contains("not found") == true)
        let runtime = try ScriptMetaKitRuntime.linkedRuntimeInfo()
        XCTAssertEqual(runtime.packageVersion, ScriptMetaKitRuntime.packageVersion)
        XCTAssertEqual(runtime.apiVersion, ScriptMetaKitRuntime.apiVersion)
        #if arch(arm64)
        XCTAssertEqual(runtime.architecture, "aarch64")
        #elseif arch(x86_64)
        XCTAssertEqual(runtime.architecture, "x86_64")
        #endif
        XCTAssertEqual(runtime.platform, "macos")
    }

    func testConfigurationIsCanonicalAtomicAndSurvivesShutdown() async throws {
        let engine = ScriptMetaKitEngine()
        let original = try await engine.configuration()
        XCTAssertEqual(original, try ScriptMetaKitConfiguration.defaults())
        var settings = original
        settings.parser.preferredLanguages = ["JA", "en", "fr"]
        settings.parser.maxPrefixBytes = 32_768
        settings.scanner.skipHidden = false
        settings.scanner.maxDepth = 12
        settings.scanner.scanTimeoutPerRootMillis = nil
        settings.cache.persistentCache = false
        settings.watcher.nativeEventLatencyMillis = 175
        settings.updateCheck.requestTimeoutMillis = nil
        settings.supportedExtensions = [".JSX", "jsx", "js"]
        let change = try await engine.applyConfiguration(settings)
        XCTAssertTrue(change.metadataInvalidated)
        let canonical = try await engine.configuration()
        XCTAssertEqual(canonical.parser.preferredLanguages, ["ja", "en", "fr"])
        XCTAssertEqual(canonical.supportedExtensions, ["js", "jsx"])
        XCTAssertNil(canonical.scanner.scanTimeoutPerRootMillis)
        XCTAssertNil(canonical.updateCheck.requestTimeoutMillis)
        let noChange = try await engine.applyConfiguration(canonical)
        XCTAssertFalse(noChange.changed)
        var invalid = canonical
        invalid.updateCheck.maxConcurrentMetaURLChecks = 0
        do {
            try await engine.applyConfiguration(invalid)
            XCTFail("Invalid configuration accepted")
        } catch { XCTAssertTrue(error.localizedDescription.contains("max_concurrent")) }
        let afterRejection = try await engine.configuration()
        XCTAssertEqual(afterRejection, canonical)
        await engine.shutdown()
        let recreated = try await engine.configuration()
        XCTAssertEqual(recreated, canonical)
    }

    func testLegacySettersSurviveShutdown() async throws {
        let engine = ScriptMetaKitEngine()
        try await engine.setResolveMacOSAlias(false)
        try await engine.setDecompileCompiledOSADuringScan(true)
        try await engine.setNativeEventLatencyMillis(250)
        try await engine.setPreferredLanguages(["fr", "en"])
        let settings = try await engine.configuration()
        await engine.shutdown()
        let restored = try await engine.configuration()
        XCTAssertEqual(restored, settings)
    }

    func testConfigurationCanStopAnActiveWatcherAndRestartExplicitly() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let engine = ScriptMetaKitEngine()
        try await engine.startWatching(folderURLs: [root], onChange: {})
        var settings = try await engine.configuration()
        let original = settings
        settings.scanner.maxNodesPerRoot = -1
        do {
            try await engine.applyConfiguration(settings)
            XCTFail("Negative limits must be rejected")
        } catch {}
        let afterRejection = try await engine.configuration()
        XCTAssertEqual(afterRejection, original)
        settings = original
        settings.watcher.enabled = false
        try await engine.applyConfiguration(settings)
        do {
            _ = try await engine.pollWatchChanges()
            XCTFail("Polling a stopped watcher must fail")
        } catch { XCTAssertTrue(error.localizedDescription.contains("watcher is not running")) }
        settings.watcher.enabled = true
        try await engine.applyConfiguration(settings)
        try await engine.startWatching(onChange: {})
        await engine.shutdown()
    }

    func testChangeNotificationRefreshAndInvalidation() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("example.jsx")
        let engine = ScriptMetaKitEngine()
        let first = try await engine.scan(folderURL: root, checkUpdates: false)
        XCTAssertEqual(first.allItems.first?.name, "日本語名")
        try source.replacingOccurrences(of: "Name=日本語名", with: "Name=変更した名前").write(to: file, atomically: true, encoding: .utf8)
        try await engine.notifyChangedPaths([file])
        let updated = try await engine.refreshDirtyRoots()
        XCTAssertEqual(updated.allItems.first?.name, "変更した名前")
        var settings = try await engine.configuration()
        settings.parser.preferredLanguages = ["en", "fr"]
        try await engine.applyConfiguration(settings)
        let localized = try await engine.refreshDirtyRoots()
        XCTAssertEqual(localized.allItems.first?.name, "English")
        try await engine.invalidateCache()
        let empty = try await engine.cachedRoots(rootIDs: first.roots.map(\.rootID))
        XCTAssertTrue(empty.allItems.isEmpty)
        let reread = try await engine.refreshDirtyRoots()
        XCTAssertEqual(reread.allItems.first?.name, "English")
        try await engine.notifyChangedPaths([], overflowed: true)
        let reconciled = try await engine.refreshDirtyRoots()
        XCTAssertEqual(reconciled.allItems.count, 1)
    }

    func testWorkspaceConfigurationSurvivesResetWithoutPersistenceRetention() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let workspace = ScriptMetaKitWorkspace(configuration: .init(preferredLanguages: ["ja"]))
        var settings = try await workspace.engineConfiguration()
        XCTAssertFalse(settings.cache.persistentCache)
        settings.parser.preferredLanguages = ["en", "fr"]
        settings.scanner.skipHidden = false
        try await workspace.applyConfiguration(settings)
        await workspace.clearVolatileState()
        let afterReset = try await workspace.engineConfiguration()
        XCTAssertEqual(afterReset, settings)
        let registration = ScriptMetaKitRoot(rootID: "test", url: root)
        let first = try await workspace.scanRoots([registration], replacingGroup: "test", rootIDs: ["test"], mode: .fileListAndMetadata)
        XCTAssertEqual(first.allItems.first?.name, "English")
        try await workspace.invalidateCache()
        let refreshed = try await workspace.refreshDirtyRoots()
        XCTAssertEqual(refreshed.allItems.first?.name, "English")
    }

    func testWorkspaceDisabledPersistenceAndExplicitInvalidation() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ScriptMetaKitPersistentCacheStore(directoryURL: root.appendingPathComponent("cache"))
        let workspace = ScriptMetaKitWorkspace(configuration: .init(cacheStore: store))
        let registration = ScriptMetaKitRoot(rootID: "test", url: root)
        _ = try await workspace.scanRoots([registration], replacingGroup: "test", rootIDs: ["test"], mode: .fileListAndMetadata, cacheScope: .all)
        XCTAssertTrue(FileManager.default.fileExists(atPath: store.writableCacheFileURL(scope: .all).path))
        try await workspace.invalidateCache(scope: .catalog)
        XCTAssertFalse(FileManager.default.fileExists(atPath: store.writableCacheFileURL(scope: .all).path))
        var settings = try await workspace.engineConfiguration()
        settings.cache.persistentCache = false
        try await workspace.applyConfiguration(settings)
        _ = try await workspace.refreshDirtyRoots(cacheScope: .all)
        XCTAssertFalse(FileManager.default.fileExists(atPath: store.writableCacheFileURL(scope: .all).path))
        await workspace.shutdown()
    }

    func testPersistenceChangesPreserveUnverifiedFileListProvenance() async throws {
        let root = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ScriptMetaKitPersistentCacheStore(directoryURL: root.appendingPathComponent("cache"))
        let workspace = ScriptMetaKitWorkspace(configuration: .init(cacheStore: store))
        let registration = ScriptMetaKitRoot(rootID: "test", url: root)
        _ = try await workspace.scanRoots([registration], replacingGroup: "test", rootIDs: ["test"], mode: .fileListAndMetadata, cacheScope: .all)
        await workspace.clearVolatileState()
        _ = try await workspace.cachedFileListSnapshot(registration, insertingIntoGroup: "test", cacheScope: .all)
        let restored = try await workspace.cachedFileListStates(rootIDs: ["test"], cacheScope: nil)
        XCTAssertEqual(restored["test"]?.source, .persistentCache)
        XCTAssertEqual(restored["test"]?.freshness, .cachedUnverified)
        var settings = try await workspace.engineConfiguration()
        settings.cache.persistentCache = false
        try await workspace.applyConfiguration(settings)
        let disabled = try await workspace.cachedFileListStates(rootIDs: ["test"], cacheScope: nil)
        XCTAssertEqual(disabled["test"]?.source, .persistentCache)
        XCTAssertEqual(disabled["test"]?.freshness, .cachedUnverified)
        try await workspace.invalidateCache(scope: .catalog)
        let catalogInvalidated = try await workspace.cachedFileListStates(rootIDs: ["test"], cacheScope: nil)
        XCTAssertEqual(catalogInvalidated["test"]?.source, .persistentCache)
        XCTAssertEqual(catalogInvalidated["test"]?.freshness, .cachedUnverified)
        _ = try await workspace.refreshDirtyRoots()
        let refreshed = try await workspace.cachedFileListStates(rootIDs: ["test"], cacheScope: nil)
        XCTAssertEqual(refreshed["test"]?.source, .memoryCache)
        XCTAssertEqual(refreshed["test"]?.freshness, .current)
    }
}
