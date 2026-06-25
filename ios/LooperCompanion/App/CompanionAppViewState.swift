import Foundation
import LooperCompanionCore
import UserNotifications

@MainActor
struct CompanionAppViewState {
    private let model: CompanionAppModel

    init(model: CompanionAppModel) {
        self.model = model
    }

    var activeSessions: [SessionSummary] {
        model.snapshotState.sessionSections.active
    }

    var runningSessions: [SessionSummary] {
        model.snapshotState.sessionSections.running
    }

    var waitingSessions: [SessionSummary] {
        model.snapshotState.sessionSections.waiting
    }

    var stoppedSessions: [SessionSummary] {
        model.snapshotState.sessionSections.stopped
    }

    var needsAttentionSessions: [SessionSummary] {
        model.snapshotState.sessionSections.needsAttention
    }

    var archivedSessions: [SessionSummary] {
        model.snapshotState.sessionSections.archived
    }

    var sessionsBadgeCount: Int {
        model.snapshotState.sessionSections.needsAttentionCount
    }

    var canSwitchAssistantSurface: Bool {
        model.snapshotState.hasSnapshot || model.connectionState == .connected
    }

    var hasSnapshot: Bool {
        model.snapshotState.hasSnapshot
    }

    var selectedAssistantSurface: CompanionAssistantSurface {
        model.snapshotState.selectedAssistantSurface
    }

    var sessionIndexIdentity: String {
        model.snapshotState.sessionIndexIdentity
    }

    var allSessions: [SessionSummary] {
        model.snapshotState.allSessions
    }

    var assistantSurfaceConnectionSummary: String? {
        switch selectedAssistantSurface {
        case .grokBuild:
            guard let grokBuild = model.snapshotState.snapshot?.grokBuild else {
                return nil
            }
            return "Grok hooks \(grokBuild.hooksHealthTitle.lowercased()) · \(grokBuild.activeSessionCount) active / \(grokBuild.sessionCount) total"
        case .devin:
            guard let devinDesktop = model.snapshotState.snapshot?.devinDesktop else {
                return nil
            }
            return "Devin \(devinDesktop.connectionTitle) · \(devinDesktop.activeSessionCount) active / \(devinDesktop.sessionCount) total · \(devinDesktop.enabledAgentCount) agents"
        case .claudeCode, .zed, .codex:
            return nil
        }
    }

    var devinEmptyStateDescription: String? {
        guard let devinDesktop = model.snapshotState.snapshot?.devinDesktop else {
            return nil
        }

        return "Devin \(devinDesktop.connectionTitle) · \(devinDesktop.enabledAgentCount) enabled agents · \(devinDesktop.sessionCount) indexed sessions."
    }

    var defaultPrompt: String {
        model.snapshotState.snapshot?.globalSettings.defaultPrompt ?? ""
    }

    var availableNotifications: [NotificationDestination] {
        model.snapshotState.snapshot?.notifications ?? []
    }

    var availableCompletionChecks: [CompletionCheckSummary] {
        model.snapshotState.snapshot?.completionChecks ?? []
    }

    var lastSyncedAt: String? {
        model.snapshotState.snapshot?.host.lastSyncedAt
    }

    var hostName: String? {
        model.snapshotState.snapshot?.host.name
    }

    func session(withID sessionID: String) -> SessionSummary? {
        model.snapshotState.session(withID: sessionID)
    }

    var connectivityHeadline: String {
        switch model.connectionState {
        case .connected:
            return model.snapshotState.snapshot?.host.name ?? "Connected"
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
        if model.connectionState == .connected, hasSnapshot {
            return connectedStatusSummary
        }

        return model.connectionState.summary
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
        model.snapshotState.detail(for: sessionID)
    }

    func isMutatingSession(_ sessionID: String) -> Bool {
        model.mutatingSessionIDs.contains(sessionID)
    }

    private var connectedStatusSummary: String {
        var parts: [String] = []

        if let connectionRoutePresentation {
            parts.append("\(connectionRoutePresentation.title) route at \(connectionRoutePresentation.detail)")
        } else if let serverHealth = model.serverHealth, serverHealth.ok {
            parts.append("API running at \(serverHealth.baseURL)")
        }

        if let workSummary = model.snapshotState.snapshot?.workStatus.displaySummary {
            parts.append(workSummary)
        }

        if let coverageSummary = model.snapshotState.snapshot?.workStatus.coverageSummary {
            parts.append(coverageSummary)
        }

        if let lastSyncedAt = model.snapshotState.snapshot?.host.lastSyncedAt, !lastSyncedAt.isEmpty {
            parts.append("synced \(ModelFormatting.relativeTimestamp(lastSyncedAt))")
        }

        return parts.isEmpty ? "Connected and ready to monitor sessions." : "\(parts.joined(separator: " · "))."
    }
}
