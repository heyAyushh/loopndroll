import Foundation
import Testing
@testable import LooperMenuBarCore

struct LooperContinuationActivityTests {
    @Test
    func defaultsFocusAssistToFiveIdleMinutes() {
        let expectedFiveIdleMinutesSeconds: TimeInterval = 300
        let suiteName = "dev.looper.tests.focus-assist.default"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)

        #expect(LooperHandoffFocusAssist.stored(in: defaults) == .afterFiveIdleMinutes)
        #expect(LooperHandoffFocusAssist.defaultOption.idleThresholdSeconds == expectedFiveIdleMinutesSeconds)
        #expect(LooperHandoffFocusAssist.defaultOption.statusTitle == "After 5 min idle")
    }

    @Test
    func supportsShorterRightNowFocusOptions() {
        #expect(LooperHandoffFocusAssist.afterThirtyIdleSeconds.idleThresholdSeconds == 30)
        #expect(LooperHandoffFocusAssist.afterOneIdleMinute.idleThresholdSeconds == 60)
        #expect(LooperHandoffFocusAssist.afterTwoIdleMinutes.idleThresholdSeconds == 120)
        #expect(LooperHandoffFocusAssist.rightNow.menuTitle == "Right now")
        #expect(LooperHandoffFocusAssist.rightNow.idleThresholdSeconds == nil)
        #expect(LooperHandoffFocusAssist.rightNow.activatesWithoutIdleDelay)
        #expect(!LooperHandoffFocusAssist.never.activatesWithoutIdleDelay)
    }

    @Test
    func storesFocusAssistPreference() {
        let suiteName = "dev.looper.tests.focus-assist.stored"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)

        LooperHandoffFocusAssist.never.save(in: defaults)

        #expect(LooperHandoffFocusAssist.stored(in: defaults) == .never)
        #expect(LooperHandoffFocusAssist.never.idleThresholdSeconds == nil)
        defaults.removePersistentDomain(forName: suiteName)
    }

    @Test
    func defaultsHandoffHotkeyToCommandL() {
        let suiteName = "dev.looper.tests.handoff-hotkey.default"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)

        #expect(LooperHandoffHotkeyOption.stored(in: defaults) == .commandL)
        #expect(LooperHandoffHotkeyOption.defaultOption.menuTitle == "⌘L")
        #expect(LooperHandoffHotkeyOption.defaultOption.isEnabled)
        defaults.removePersistentDomain(forName: suiteName)
    }

    @Test
    func migratesLegacyRightNowHotkeyPreference() {
        let suiteName = "dev.looper.tests.handoff-hotkey.migration"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)
        defaults.set(
            LooperHandoffHotkeyOption.commandOptionL.rawValue,
            forKey: LooperHandoffHotkeyOption.legacyRightNowUserDefaultsKey
        )

        LooperHandoffHotkeyOption.migrateStoredPreference(in: defaults)

        #expect(LooperHandoffHotkeyOption.stored(in: defaults) == .commandOptionL)
        #expect(defaults.object(forKey: LooperHandoffHotkeyOption.legacyRightNowUserDefaultsKey) == nil)
        defaults.removePersistentDomain(forName: suiteName)
    }

    @Test
    func savingHandoffHotkeyRemovesLegacyPreference() {
        let suiteName = "dev.looper.tests.handoff-hotkey.save"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)
        defaults.set(
            LooperHandoffHotkeyOption.controlL.rawValue,
            forKey: LooperHandoffHotkeyOption.legacyRightNowUserDefaultsKey
        )

        LooperHandoffHotkeyOption.commandShiftL.save(in: defaults)

        #expect(LooperHandoffHotkeyOption.stored(in: defaults) == .commandShiftL)
        #expect(defaults.object(forKey: LooperHandoffHotkeyOption.legacyRightNowUserDefaultsKey) == nil)
        defaults.removePersistentDomain(forName: suiteName)
    }

    @Test
    func defaultsHandoffHoldToTenMinutes() {
        let expectedTenMinuteSeconds: TimeInterval = 600
        let suiteName = "dev.looper.tests.handoff-hold.default"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)

        #expect(LooperHandoffHoldDuration.stored(in: defaults) == .tenMinutes)
        #expect(LooperHandoffHoldDuration.defaultOption.durationSeconds == expectedTenMinuteSeconds)
        #expect(LooperHandoffHoldDuration.defaultOption.menuTitle == "10 min")
        defaults.removePersistentDomain(forName: suiteName)
    }

    @Test
    func handoffHoldOffUsesBriefAutoHideLease() {
        let expectedOffAutoHideSeconds: TimeInterval = 3

        #expect(LooperHandoffHoldDuration.off.durationSeconds == expectedOffAutoHideSeconds)
        #expect(LooperHandoffHoldDuration.off.menuTitle == "Off")
        #expect(!LooperHandoffHoldDuration.off.holdsHandoffAfterPresentation)
    }

    @Test
    func mobileHealthSupportsNativeHandoffOnlyWithReachableURL() {
        let reachableHealth = MobileHealthResponse(
            ok: true,
            baseURL: "http://192.168.1.4:8765",
            baseURLs: ["http://192.168.1.4:8765", "http://127.0.0.1:8765"],
            requiresAuthentication: true
        )
        let loopbackOnlyHealth = MobileHealthResponse(
            ok: true,
            baseURL: "http://127.0.0.1:8765",
            baseURLs: ["http://127.0.0.1:8765"],
            requiresAuthentication: true
        )

        #expect(reachableHealth.supportsNativeHandoff)
        #expect(reachableHealth.preferredReachableHandoffBaseURL?.host == "192.168.1.4")
        #expect(!loopbackOnlyHealth.supportsNativeHandoff)
    }

    @Test
    func mobileHealthPrefersTailscaleForNativeHandoff() {
        let health = MobileHealthResponse(
            ok: true,
            baseURL: "http://192.168.1.4:8765",
            baseURLs: [
                "http://192.168.1.4:8765",
                "http://100.119.200.69:8765",
                "http://127.0.0.1:8765",
            ],
            requiresAuthentication: true,
            tailscale: MobileTailscaleStatus(
                available: true,
                running: true,
                backendState: "Running",
                baseURL: "http://100.119.200.69:8765",
                dnsName: "ayushs-macbook-pro.tail62d9a8.ts.net",
                ipAddresses: ["100.119.200.69"],
                magicDNSEnabled: true,
                magicDNSSuffix: "tail62d9a8.ts.net",
                source: "cli",
                tailnetName: "heyayushh.github"
            )
        )

        #expect(health.supportsNativeHandoff)
        #expect(health.preferredReachableHandoffBaseURL?.absoluteString == "http://100.119.200.69:8765")
        #expect(health.routeSummaryTitle == "Tailscale: 100.119.200.69")
    }

    @Test
    func mobileHealthRoutePreferenceCanPreferLAN() {
        let health = MobileHealthResponse(
            ok: true,
            baseURL: "http://127.0.0.1:8765",
            baseURLs: [
                "http://127.0.0.1:8765",
                "http://100.119.200.69:8765",
                "http://192.168.1.4:8765",
            ],
            requiresAuthentication: true,
            tailscale: MobileTailscaleStatus(
                available: true,
                running: true,
                backendState: "Running",
                baseURL: "http://100.119.200.69:8765",
                dnsName: "ayushs-macbook-pro.tail62d9a8.ts.net",
                ipAddresses: ["100.119.200.69"],
                magicDNSEnabled: true,
                magicDNSSuffix: "tail62d9a8.ts.net",
                source: "cli",
                tailnetName: "heyayushh.github"
            )
        )

        #expect(
            health.preferredReachableHandoffBaseURL(preference: .lan)?.absoluteString ==
                "http://192.168.1.4:8765"
        )
        #expect(health.routeSummaryTitle(preference: .lan) == "LAN: 192.168.1.4")
        #expect(
            health.preferredReachableHandoffBaseURL(preference: .remote)?.absoluteString ==
                "http://100.119.200.69:8765"
        )
    }

    @Test
    func mobileRoutePreferencePersists() {
        let suiteName = "dev.looper.tests.mobile-route.stored"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)

        MobileRoutePreference.remote.save(in: defaults)

        #expect(MobileRoutePreference.stored(in: defaults) == .remote)
        #expect(MobileRoutePreference.remote.menuTitle == "Remote first")
        defaults.removePersistentDomain(forName: suiteName)
    }

    @Test
    func storesHandoffHoldPreference() {
        let suiteName = "dev.looper.tests.handoff-hold.stored"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)

        LooperHandoffHoldDuration.fiveMinutes.save(in: defaults)

        #expect(LooperHandoffHoldDuration.stored(in: defaults) == .fiveMinutes)
        #expect(LooperHandoffHoldDuration.fiveMinutes.menuTitle == "5 min")
        defaults.removePersistentDomain(forName: suiteName)
    }

    @Test
    func handoffActivationLeaseCanReplaceLongerExpiration() {
        let start = Date(timeIntervalSinceReferenceDate: 1_000)
        var lease = LooperHandoffActivationLease()

        lease.activate(now: start, holdDuration: .fiveMinutes)
        lease.replace(now: start, holdDuration: .off)
        let isActiveAfterOffAutoHide = lease.isActive(
            now: start.addingTimeInterval(LooperHandoffHoldDuration.off.durationSeconds)
        )

        #expect(!isActiveAfterOffAutoHide)
    }

    @Test
    func handoffActivationLeaseStaysActiveUntilHoldExpires() {
        let start = Date(timeIntervalSinceReferenceDate: 1_000)
        var lease = LooperHandoffActivationLease()

        lease.activate(now: start, holdDuration: .oneMinute)
        let isActiveBeforeExpiration = lease.isActive(now: start.addingTimeInterval(59))
        let isActiveAtExpiration = lease.isActive(now: start.addingTimeInterval(60))

        #expect(isActiveBeforeExpiration)
        #expect(!isActiveAtExpiration)
    }

    @Test
    func handoffActivationLeaseKeepsLongerExpiration() {
        let start = Date(timeIntervalSinceReferenceDate: 1_000)
        var lease = LooperHandoffActivationLease()

        lease.activate(now: start, holdDuration: .fiveMinutes)
        lease.activate(now: start, holdDuration: .oneMinute)
        let isActiveBeforeLongerExpiration = lease.isActive(now: start.addingTimeInterval(299))

        #expect(isActiveBeforeLongerExpiration)
    }

    @Test
    func usesNewestActiveThreadAsContinuationTarget() throws {
        let snapshot = desktopSnapshot(threads: [
            thread(id: "old", title: "Old", updatedAtMs: 1),
            thread(id: "archived-new", title: "Archived", updatedAtMs: 3, archived: true),
            thread(
                id: "new",
                title: "Fresh",
                updatedAtMs: 2,
                assistantPreview: "Current assistant text"
            ),
        ])

        let descriptor = LooperContinuationActivityBuilder.descriptor(from: snapshot)

        #expect(descriptor.title == "Fresh")
        #expect(descriptor.targetContentIdentifier == "looper.session.new")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionID] == "new")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionPreview] == "Current assistant text")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.updatedAtMilliseconds] == "2")
    }

    @Test
    func usesNewestSessionMiniAsContinuationTarget() throws {
        let runtime = try seededRuntime(latestSeq: 9, records: [
            miniRecord(id: "old-mini", title: "Old Mini", archived: false, updatedAtMs: 1),
            miniRecord(id: "archived-mini", title: "Archived Mini", archived: true, updatedAtMs: 9),
            miniRecord(
                id: "fresh-mini",
                title: "Fresh Mini",
                archived: false,
                assistantPreview: "Live mini preview",
                updatedAtMs: 4
            ),
        ])
        let snapshot = try runtime.cachedSnapshot()

        let descriptor = LooperContinuationActivityBuilder.descriptor(
            from: snapshot,
            handoffBaseURL: URL(string: "http://100.64.0.8:8765")
        )

        #expect(descriptor.title == "Fresh Mini")
        #expect(descriptor.targetContentIdentifier == "looper.session.fresh-mini")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionID] == "fresh-mini")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionPreview] == "Live mini preview")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.updatedAtMilliseconds] == "4")
        #expect(
            descriptor.userInfo[LooperContinuationActivity.UserInfoKey.handoffWebpageURL]
                == "http://100.64.0.8:8765/handoff/sessions/fresh-mini"
        )
    }

    @Test
    func blankContinuationTitleFallsBackToThreadID() throws {
        let descriptor = LooperContinuationActivityBuilder.descriptor(from: desktopSnapshot(threads: [
            thread(id: "thread-main", title: "   ", updatedAtMs: 1),
        ]))

        #expect(descriptor.title == "thread-main")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionTitle] == "thread-main")
    }

    @Test
    func prefersNewestActiveThreadAcrossAssistants() throws {
        let descriptor = LooperContinuationActivityBuilder.descriptor(from: desktopSnapshot(threads: [
            thread(id: "codex-main", title: "Codex", updatedAtMs: 1),
            thread(
                id: "devin:devin-cli:brindle-cadet",
                title: "Devin",
                updatedAtMs: 9,
                agentPath: "/Users/test/Library/Application Support/Devin/session"
            ),
        ]))

        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionID] == "devin:devin-cli:brindle-cadet")
        #expect(descriptor.targetContentIdentifier == "looper.session.devin:devin-cli:brindle-cadet")
    }

    @Test
    func keepsStableActivityIdentitySeparateFromExactSessionTarget() {
        let descriptor = LooperContinuationActivityBuilder.descriptor(from: desktopSnapshot(threads: [
            thread(id: "session-a", title: "Session A", updatedAtMs: 1),
        ]))

        #expect(LooperContinuationActivity.activityType == "dev.looper.app.continue-session")
        #expect(LooperContinuationActivity.persistentIdentifier == "dev.looper.app.continuation.current-session")
        #expect(descriptor.targetContentIdentifier == "looper.session.session-a")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionID] == "session-a")
    }

    @Test
    func carriesHandoffWebpageURL() throws {
        let descriptor = LooperContinuationActivityBuilder.descriptor(
            from: desktopSnapshot(threads: [
                thread(id: "thread-main", title: "Main", updatedAtMs: 1),
            ]),
            handoffBaseURL: URL(string: "http://192.168.1.4:8765")
        )

        #expect(
            descriptor.userInfo[LooperContinuationActivity.UserInfoKey.handoffWebpageURL]
                == "http://192.168.1.4:8765/handoff/sessions/thread-main"
        )
        #expect(descriptor.webpageURL?.absoluteString == "http://192.168.1.4:8765/handoff/sessions/thread-main")
    }

    @Test
    func encodesSlashSeparatedSessionIDsInHandoffURL() throws {
        let descriptor = LooperContinuationActivityBuilder.descriptor(
            from: desktopSnapshot(threads: [
                thread(id: "acp/devin-cli/brindle-cadet", title: "Devin", updatedAtMs: 1),
            ]),
            handoffBaseURL: URL(string: "http://192.168.1.4:8765")
        )

        #expect(
            descriptor.userInfo[LooperContinuationActivity.UserInfoKey.handoffWebpageURL]
                == "http://192.168.1.4:8765/handoff/sessions/acp%2Fdevin-cli%2Fbrindle-cadet"
        )
        #expect(
            descriptor.webpageURL?.absoluteString
                == "http://192.168.1.4:8765/handoff/sessions/acp%2Fdevin-cli%2Fbrindle-cadet"
        )
    }

    @Test
    func extractsSessionIDFromContinuationActivity() {
        let activity = NSUserActivity(activityType: LooperContinuationActivity.activityType)
        activity.userInfo = [
            LooperContinuationActivity.UserInfoKey.sessionID: "thread-main",
        ]
        activity.targetContentIdentifier = "looper.session.thread-fallback"

        #expect(LooperContinuationActivity.sessionID(from: activity) == "thread-main")
    }

    @Test
    func extractsSessionIDFromTargetContentIdentifier() {
        let activity = NSUserActivity(activityType: LooperContinuationActivity.activityType)
        activity.targetContentIdentifier = "looper.session.thread-target"

        #expect(LooperContinuationActivity.sessionID(from: activity) == "thread-target")
    }

    @Test
    func fallsBackToGenericActivityWhenNoThreadsExist() {
        let descriptor = LooperContinuationActivityBuilder.descriptor(from: desktopSnapshot(threads: []))

        #expect(descriptor.title == "looper")
        #expect(descriptor.targetContentIdentifier == "looper")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.kind] == "app")
    }

    @Test
    func fallsBackToArchivedThreadWhenItIsOnlyVisibleTarget() {
        let descriptor = LooperContinuationActivityBuilder.descriptor(
            from: desktopSnapshot(threads: [
                thread(id: "archived", title: "Only visible", updatedAtMs: 7, archived: true),
            ])
        )

        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionID] == "archived")
    }

    private func desktopSnapshot(threads: [DesktopThreadSummary]) -> DesktopSnapshotResponse {
        DesktopSnapshotResponse(
            controlPlane: ControlPlaneStatusResponse(
                hooks: HookStatusSummary(
                    enabled: true,
                    registeredEvents: [],
                    activeCommand: nil,
                    owner: "rust",
                    health: "healthy",
                    issues: [],
                    recentFailuresCount: 0
                ),
                codexServers: [],
                source: SourceStatusSummary(
                    codexHome: "/tmp/codex",
                    stateDb: nil,
                    logsDb: nil,
                    sessionsRoot: "/tmp/sessions",
                    health: "healthy",
                    degradedReason: nil
                )
            ),
            devinDesktop: DevinDesktopStatus(
                acpBridge: DevinAcpBridgeStatus(
                    available: false,
                    controlLevel: "visibility-only",
                    summary: "Unavailable",
                    actions: [],
                    agents: []
                )
            ),
            threadCount: threads.count,
            activeThreadCount: threads.filter { !$0.archived }.count,
            archivedThreadCount: threads.filter(\.archived).count,
            threads: threads,
            automations: [],
            goals: [],
            compactions: [],
            assistantAdapters: []
        )
    }

    private func thread(
        id: String,
        title: String?,
        updatedAtMs: Int64?,
        assistantPreview: String? = nil,
        archived: Bool = false,
        agentPath: String? = nil
    ) -> DesktopThreadSummary {
        DesktopThreadSummary(
            threadId: id,
            title: title,
            cwd: "/tmp/looper",
            transcriptPath: nil,
            source: "desktop",
            model: nil,
            reasoningEffort: nil,
            updatedAtMs: updatedAtMs,
            assistantPreview: assistantPreview,
            archived: archived,
            capabilities: ThreadCapabilitiesSummary(
                threadId: id,
                mcpTools: [],
                appTools: [],
                automationTools: [],
                spawn: SpawnGraphSummary(
                    parentThreadId: nil,
                    rootThreadId: id,
                    children: [],
                    launchKind: "main"
                ),
                agentNickname: nil,
                agentRole: nil,
                agentPath: agentPath
            )
        )
    }

    private func temporaryStoreFileURL() -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("LooperContinuationTests-\(UUID().uuidString)", isDirectory: true)
            .appendingPathComponent(MenuBarSessionRuntime.defaultFileName)
    }

    private func seededRuntime(
        latestSeq: Int64,
        records: [TestContinuationMiniFixture]
    ) throws -> MenuBarSessionRuntime {
        let fileURL = temporaryStoreFileURL()
        try seedMiniCache(at: fileURL, latestSeq: latestSeq, records: records)
        return try MenuBarSessionRuntime(fileURL: fileURL)
    }

    private func seedMiniCache(
        at fileURL: URL,
        latestSeq: Int64,
        records: [TestContinuationMiniFixture]
    ) throws {
        let payload: [String: Any] = [
            "latestSeq": latestSeq,
            "sessions": records.map { record in
                [
                    "sessionId": record.sessionID,
                    "assistantSurface": record.assistantSurface,
                    "seq": record.seq,
                    "revision": record.revision,
                    "payloadJson": record.payloadJSON,
                ]
            },
            "pendingCommands": [],
            "serverTime": "",
        ]
        let data = try JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])
        try FileManager.default.createDirectory(
            at: fileURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try data.write(to: fileURL, options: .atomic)
    }

    private func miniRecord(
        id: String,
        title: String,
        archived: Bool,
        assistantPreview: String = "Ready",
        updatedAtMs: Int64
    ) throws -> TestContinuationMiniFixture {
        let payload = ContinuationMiniPayload(
            id: id,
            sessionId: id,
            ref: id,
            title: title,
            status: archived ? "archived" : "active",
            effectiveMode: "await-reply",
            canSendPrompt: true,
            replyable: true,
            queueCount: 0,
            lifecycle: "active",
            isArchived: archived,
            assistantPreview: assistantPreview,
            metadata: ContinuationMiniMetadata(
                projectName: "looper",
                projectPath: "/Users/test/looper"
            ),
            lastActivityAtMs: updatedAtMs,
            updatedAtMs: updatedAtMs
        )
        let data = try JSONEncoder().encode(payload)
        return TestContinuationMiniFixture(
            sessionID: id,
            assistantSurface: "codex",
            seq: updatedAtMs,
            revision: "rev-\(updatedAtMs)",
            payloadJSON: String(decoding: data, as: UTF8.self)
        )
    }
}

private struct TestContinuationMiniFixture: Equatable, Sendable {
    let sessionID: String
    let assistantSurface: String
    let seq: Int64
    let revision: String
    let payloadJSON: String
}

private struct ContinuationMiniPayload: Encodable {
    let id: String
    let sessionId: String
    let ref: String
    let title: String
    let status: String
    let effectiveMode: String
    let canSendPrompt: Bool
    let replyable: Bool
    let queueCount: Int
    let lifecycle: String
    let isArchived: Bool
    let assistantPreview: String
    let metadata: ContinuationMiniMetadata
    let lastActivityAtMs: Int64
    let updatedAtMs: Int64
}

private struct ContinuationMiniMetadata: Encodable {
    let projectName: String
    let projectPath: String
}
