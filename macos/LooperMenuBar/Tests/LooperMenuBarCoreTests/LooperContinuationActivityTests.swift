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
    func prefersCodexThreadOverNewerExternalSession() throws {
        let descriptor = LooperContinuationActivityBuilder.descriptor(from: desktopSnapshot(threads: [
            thread(id: "codex-main", title: "Codex", updatedAtMs: 1),
            thread(
                id: "devin:devin-cli:brindle-cadet",
                title: "Devin",
                updatedAtMs: 9,
                agentPath: "/Users/test/Library/Application Support/Devin/session"
            ),
        ]))

        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionID] == "codex-main")
        #expect(descriptor.targetContentIdentifier == "looper.session.codex-main")
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
    func carriesDebugHandoffURLWithoutWebFallback() throws {
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
}
