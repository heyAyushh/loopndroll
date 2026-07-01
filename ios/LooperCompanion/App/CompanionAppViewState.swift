import Foundation
import LooperCompanionCore
import UserNotifications

@MainActor
struct CompanionAppViewState {
    private let model: CompanionAppModel

    init(model: CompanionAppModel) {
        self.model = model
    }

    private var snapshotState: CompanionSnapshotStateStore {
        model.snapshotState
    }

    var activeSessions: [SessionSummary] {
        snapshotState.sessionSections.active
    }

    var runningSessions: [SessionSummary] {
        snapshotState.sessionSections.running
    }

    var waitingSessions: [SessionSummary] {
        snapshotState.sessionSections.waiting
    }

    var stoppedSessions: [SessionSummary] {
        snapshotState.sessionSections.stopped
    }

    var needsAttentionSessions: [SessionSummary] {
        snapshotState.sessionSections.needsAttention
    }

    var archivedSessions: [SessionSummary] {
        snapshotState.sessionSections.archived
    }

    var sessionsBadgeCount: Int {
        snapshotState.sessionSections.needsAttentionCount
    }

    var canSwitchAssistantSurface: Bool {
        snapshotState.hasSnapshot || model.connectionState == .connected
    }

    var hasSnapshot: Bool {
        snapshotState.hasSnapshot
    }

    var selectedAssistantSurface: CompanionAssistantSurface {
        snapshotState.selectedAssistantSurface
    }

    var sessionIndexIdentity: String {
        snapshotState.sessionIndexIdentity
    }

    var allSessions: [SessionSummary] {
        snapshotState.allSessions
    }

    var assistantSurfaceConnectionSummary: String? {
        switch selectedAssistantSurface {
        case .grokBuild:
            guard let grokBuild = snapshotState.snapshot?.grokBuild else {
                return nil
            }
            return "Grok hooks \(grokBuild.hooksHealthTitle.lowercased()) · \(grokBuild.activeSessionCount) active / \(grokBuild.sessionCount) total"
        case .devin:
            guard let devinDesktop = snapshotState.snapshot?.devinDesktop else {
                return nil
            }
            return "Devin \(devinDesktop.connectionTitle) · \(devinDesktop.activeSessionCount) active / \(devinDesktop.sessionCount) total · \(devinDesktop.enabledAgentCount) agents"
        case .claudeCode, .zed, .codex:
            return nil
        }
    }

    var devinEmptyStateDescription: String? {
        guard let devinDesktop = snapshotState.snapshot?.devinDesktop else {
            return nil
        }

        return "Devin \(devinDesktop.connectionTitle) · \(devinDesktop.enabledAgentCount) enabled agents · \(devinDesktop.sessionCount) indexed sessions."
    }

    var defaultPrompt: String {
        snapshotState.snapshot?.globalSettings.defaultPrompt ?? ""
    }

    var availableNotifications: [NotificationDestination] {
        snapshotState.snapshot?.notifications ?? []
    }

    var availableCompletionChecks: [CompletionCheckSummary] {
        snapshotState.snapshot?.completionChecks ?? []
    }

    var deviceHubConnectionStatusLabel: String {
        connectivityStatusLabel
    }

    var hostName: String? {
        snapshotState.snapshot?.host.name
    }

    func session(withID sessionID: String) -> SessionSummary? {
        snapshotState.session(withID: sessionID)
    }

    func session(
        withID sessionID: String,
        assistantSurface: CompanionAssistantSurface
    ) -> SessionSummary? {
        snapshotState.session(withID: sessionID, assistantSurface: assistantSurface)
    }

    func assistantSurface(for sessionID: String) -> CompanionAssistantSurface {
        snapshotState.assistantSurface(for: sessionID)
    }

    var connectivityHeadline: String {
        if isShowingUsableLocalState {
            switch model.connectionState {
            case .connecting:
                return localStateHeadline
            case .offline:
                return localStateHeadline
            case .connected, .unauthorized, .locked, .unpaired:
                break
            }
        }

        switch model.connectionState {
        case .connected:
            return snapshotState.snapshot?.host.name ?? "Connected"
        case .connecting:
            return "Connecting to your Mac"
        case .offline:
            return "Mac connection offline"
        case .unauthorized:
            return "Connection needs approval"
        case .locked:
            return "Unlock looper"
        case .unpaired:
            return "Set up your Mac link"
        }
    }

    var connectivitySummary: String {
        if hasSnapshot {
            switch model.connectionState {
            case .connected:
                return connectedStatusSummary
            case .connecting:
                return "Showing local sessions while Looper reconnects."
            case .offline:
                return "Showing local sessions; commands will retry when Looper reconnects."
            case .unauthorized, .locked, .unpaired:
                break
            }
        }

        return model.connectionState.summary
    }

    var connectivityStatusLabel: String {
        if model.isAwaitingRouteSessionProof {
            return model.connectionState.label
        }

        if isShowingUsableLocalState {
            switch model.connectionState {
            case .connecting, .offline:
                return "Local"
            case .connected, .unauthorized, .locked, .unpaired:
                break
            }
        }

        return model.connectionState.label
    }

    var sessionsUnavailableTitle: String {
        if !shouldUseSessionListEmptyState {
            switch model.connectionState {
            case .connecting:
                return "Connecting to Your Mac"
            case .offline:
                return "Mac Offline"
            case .unauthorized:
                return "Connection Needs Approval"
            case .locked:
                return "Unlock Required"
            case .unpaired:
                return "Set Up Your Mac Link"
            case .connected:
                return selectedSurfaceEmptyTitle
            }
        }

        return selectedSurfaceEmptyTitle
    }

    var sessionsUnavailableSystemImage: String {
        shouldUseSessionListEmptyState ? "tray" : model.connectionState.symbolName
    }

    var sessionsEmptyDescription: String {
        guard shouldUseSessionListEmptyState else {
            return connectivitySummary
        }

        return selectedSurfaceEmptyDescription
    }

    var deviceHubAccessStatusLabel: String {
        if isShowingUsableLocalState {
            return connectivityStatusLabel
        }

        switch model.connectionState {
        case .connected:
            return "Approved"
        case .connecting:
            return "Checking"
        case .offline:
            return "Offline"
        case .unauthorized:
            return "Needs Approval"
        case .locked:
            return "Locked"
        case .unpaired:
            return "Not Linked"
        }
    }

    var deviceHubAPIStatusLabel: String {
        guard model.realtimeStreamIsLive else {
            if isShowingUsableLocalState {
                return connectivityStatusLabel
            }

            return model.connectionState == .connected ? "Unknown" : "Offline"
        }

        guard let serverHealth = model.serverHealth else {
            return "Unknown"
        }

        return serverHealth.ok ? "Running" : "Unavailable"
    }

    var isShowingUsableLocalState: Bool {
        hasSnapshot && (model.connectionState == .connecting || model.connectionState == .offline)
    }

    var connectionRoutePresentation: CompanionConnectionRoutePresentation? {
        guard let baseURL = model.activeConnectionRouteBaseURL else {
            return nil
        }

        return CompanionConnectionRoutePresentation(
            baseURL: baseURL,
            tailscaleDetail: model.serverHealth?.tailscale?.detailLabel
        )
    }

    var activeConnectionRouteBaseURLString: String? {
        model.activeConnectionRouteBaseURL?.absoluteString
    }

    var localNotificationStatusLabel: String {
        switch model.localNotificationStatus {
        case .authorized:
            return "Allowed"
        case .provisional:
            return "Provisional"
        case .ephemeral:
            return "Temporary"
        case .denied:
            return "Off"
        case .notDetermined:
            return "Not set"
        @unknown default:
            return "Unknown"
        }
    }

    var remotePushStatusLabel: String {
        if model.isRegisteringRemotePush {
            return "Registering"
        }

        if let remotePushRegistration = model.remotePushRegistration {
            return remotePushRegistration.state.label
        }

        if model.remotePushFailureMessage != nil {
            return "Registration failed"
        }

        return canSendLocalNotifications ? "Waiting for APNs" : "Not set"
    }

    var remotePushDetailMessage: String {
        if let remotePushFailureMessage = model.remotePushFailureMessage {
            return remotePushFailureMessage
        }

        if let remotePushRegistration = model.remotePushRegistration {
            return remotePushRegistration.message
        }

        return canSendLocalNotifications
            ? "looper will keep local fallback alerts until APNs is ready on the Mac."
            : "Enable notifications on iPhone to receive stop alerts."
    }

    var shouldUseLocalFallbackNotifications: Bool {
        model.remotePushRegistration?.state != .enabled
    }

    var canSendLocalNotifications: Bool {
        switch model.localNotificationStatus {
        case .authorized, .ephemeral, .provisional:
            return true
        case .denied, .notDetermined:
            return false
        @unknown default:
            return false
        }
    }

    var areLocalNotificationsDenied: Bool {
        model.localNotificationStatus == .denied
    }

    func detail(for sessionID: String) -> SessionDetail? {
        snapshotState.detail(for: sessionID)
    }

    func detail(for route: SessionDetailRoute) -> SessionDetail? {
        model.sessionDetail(
            for: route.sessionID,
            assistantSurface: route.assistantSurface
        )
    }

    private var connectedStatusSummary: String {
        "Connected."
    }

    private var localStateHeadline: String {
        snapshotState.snapshot?.host.name ?? "Looper"
    }

    private var shouldUseSessionListEmptyState: Bool {
        model.connectionState == .connected || isShowingUsableLocalState
    }

    private var selectedSurfaceEmptyTitle: String {
        switch selectedAssistantSurface {
        case .claudeCode:
            return "No Claude Code Sessions"
        case .zed:
            return "No Zed Sessions"
        case .grokBuild:
            return "No Grok Build Sessions"
        case .devin:
            return "No Devin Sessions"
        case .codex:
            return "No Sessions"
        }
    }

    private var selectedSurfaceEmptyDescription: String {
        switch selectedAssistantSurface {
        case .claudeCode:
            return "Claude Code sessions appear here separately from Codex when Claude is running on your Mac."
        case .zed:
            return "Zed ACP targets are read-only in Looper. Zed session import is not available yet."
        case .grokBuild:
            return "Start a Grok Build session on your Mac or install the Grok CLI. Looper reads sessions from ~/.grok/sessions/ and hooks at ~/.grok/hooks/looper.json."
        case .devin:
            if let devinEmptyStateDescription {
                return devinEmptyStateDescription
            }
            return "Devin Desktop sessions appear here when Devin is running on your Mac."
        case .codex:
            return connectivitySummary
        }
    }
}
