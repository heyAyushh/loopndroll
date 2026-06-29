import Foundation

public enum LooperDiagnosticsContent {
    private enum Layout {
        static let maxRevisionPreviewCharacters = 96
        static let maxThreadTimingRows = 5
        static let maxAcpSessionTimingRows = 5
        static let maxClassificationRows = 8
    }

    public static func report(
        from result: MenuRefreshResult,
        mobileRoutePreference: MobileRoutePreference = .defaultOption,
        generatedAt: Date = Date()
    ) -> String {
        var lines = [
            "Looper Diagnostics",
            "State: \(stateTitle(for: result))",
            "diagnostics_generated_at=\(ISO8601DateFormatter().string(from: generatedAt))",
        ]

        if let error = result.error {
            appendSection("Error", to: &lines)
            lines.append("refresh_error=\(error.message)")
        }

        appendSessionMini(result.sessionMiniSnapshot, to: &lines)

        guard let snapshot = result.snapshot else {
            return lines.joined(separator: "\n")
        }

        appendControlPlane(snapshot.controlPlane, to: &lines)
        appendMobile(result.mobileHealth, mobileRoutePreference: mobileRoutePreference, to: &lines)
        appendAcpHosts(result.acpClientHosts, to: &lines)
        appendAcpTargets(snapshot.acpTargets, to: &lines)
        appendRoutes(result: result, to: &lines)
        appendTiming(snapshot, acpHosts: result.acpClientHosts, to: &lines)
        appendClassification(snapshot, to: &lines)

        return lines.joined(separator: "\n")
    }

    private static func stateTitle(for result: MenuRefreshResult) -> String {
        if result.sessionMiniSnapshot != nil {
            if result.error != nil {
                return "local-state+degraded-http"
            }
            return result.snapshot == nil ? "local-state" : "local-state+http-enrichment"
        }
        if result.snapshot != nil {
            return "http-recovery"
        }
        return "unavailable"
    }

    private static func appendSessionMini(
        _ snapshot: MenuBarSessionMiniLocalSnapshot?,
        to lines: inout [String]
    ) {
        appendSection("SessionMini", to: &lines)
        guard let snapshot else {
            lines.append("session_mini=missing")
            return
        }

        let activeCount = snapshot.sessions.filter { !$0.isArchived }.count
        let archivedCount = snapshot.sessions.count - activeCount
        lines.append(
            [
                "source=client-core",
                "seq=\(snapshot.latestSeq)",
                "sessions=\(snapshot.sessions.count)",
                "active=\(activeCount)",
                "archived=\(archivedCount)",
                "pending_commands=\(snapshot.pendingCommands.count)",
            ].joined(separator: " ")
        )
        for session in snapshot.sessions.prefix(Layout.maxThreadTimingRows) {
            lines.append(
                [
                    "mini_session=\(session.sessionID)",
                    "surface=\(session.assistantSurface)",
                    "revision=\(session.revision)",
                    "status=\(session.status)",
                    "replyable=\(session.replyable)",
                    "queue=\(session.queueCount)",
                ].joined(separator: " ")
            )
        }
    }

    private static func appendControlPlane(_ status: ControlPlaneStatusResponse, to lines: inout [String]) {
        appendSection("Control Plane", to: &lines)
        lines.append("hooks=\(status.hooks.health) owner=\(status.hooks.owner) enabled=\(status.hooks.enabled)")
        lines.append("hook_events=\(joined(status.hooks.registeredEvents))")
        lines.append("source_health=\(status.source.health)")
    }

    private static func appendMobile(
        _ mobileHealth: MobileHealthResponse?,
        mobileRoutePreference: MobileRoutePreference,
        to lines: inout [String]
    ) {
        appendSection("Mobile Route", to: &lines)
        guard let mobileHealth else {
            lines.append("mobile=missing")
            return
        }

        lines.append("mobile_ok=\(mobileHealth.ok) auth=\(mobileHealth.requiresAuthentication)")
        lines.append("route_preference=\(mobileRoutePreference.rawValue)")
        lines.append("http_route=\(mobileHealth.routeSummaryTitle(preference: mobileRoutePreference))")
        lines.append("grpc_routes=\(joined(mobileHealth.preferredRealtimeBaseURLs.map(\.absoluteString)))")
        if let tailscale = mobileHealth.tailscale {
            lines.append("tailscale=\(tailscale.statusTitle) detail=\(tailscale.routeDetailTitle)")
        }
    }

    private static func appendAcpHosts(_ response: AcpClientHostsResponse?, to lines: inout [String]) {
        appendSection("ACP Hosts", to: &lines)
        guard let response else {
            lines.append("hosts=missing")
            return
        }

        let hosts = response.hosts.sorted { $0.id.localizedStandardCompare($1.id) == .orderedAscending }
        lines.append("host_count=\(hosts.count)")
        for host in hosts {
            lines.append(
                [
                    "host=\(host.id)",
                    "label=\(host.label)",
                    "running=\(host.running)",
                    "installed=\(host.installed)",
                    "actions=\(joined(host.actions.map(\.id)))",
                    "agents=\(host.enabledAgentCount)/\(host.agents.count)",
                    runtimeText(host.runtime),
                ].joined(separator: " ")
            )
            for agent in host.agents.sorted(by: { $0.id.localizedStandardCompare($1.id) == .orderedAscending }) {
                lines.append(
                    [
                        "agent=\(agent.id)",
                        "name=\(agent.name)",
                        "control=\(agent.controlLevel)",
                        "prompt=\(agent.supportsPrompt)",
                        "sessions=\(agent.supportsSessions)",
                        "launch=\(agent.launchConfigured)",
                        "source=\(agent.source)",
                    ].joined(separator: " ")
                )
            }
            if !host.limitations.isEmpty {
                lines.append("limitations=\(joined(host.limitations))")
            }
        }
    }

    private static func appendAcpTargets(_ targets: [AcpTargetSummary], to lines: inout [String]) {
        appendSection("ACP Targets", to: &lines)
        lines.append("summary=\(LooperMenuContent.acpTargetStatusTitle(from: targets))")
        for row in LooperMenuContent.buildAcpTargetRows(from: targets) {
            lines.append("target=\(row.id) title=\(row.title) status=\(row.subtitle) detail=\(row.detail)")
        }
    }

    private static func appendRoutes(result: MenuRefreshResult, to lines: inout [String]) {
        appendSection("Routes", to: &lines)
        [
            ControlPlaneEndpoint.desktopSnapshot,
            .desktopConnections,
            .controlPlaneStatus,
            .mobileHealth,
            .acpClientHosts,
            .registerHooks,
            .unregisterHooks,
            .unregisterLiveHooks,
        ].map(routeLine)
            .forEach { lines.append($0) }

        for target in HookRepairTarget.allCases {
            lines.append(routeLine(.registerTargetHooks(target)))
            lines.append(routeLine(.unregisterTargetHooks(target)))
            lines.append(routeLine(.unregisterLiveTargetHooks(target)))
        }

        if let hosts = result.acpClientHosts?.hosts {
            for action in hosts.flatMap(\.actions).sorted(by: { $0.path.localizedStandardCompare($1.path) == .orderedAscending }) {
                lines.append("action=\(action.id) method=\(action.method) path=\(action.path)")
            }
        }
    }

    private static func appendTiming(
        _ snapshot: DesktopSnapshotResponse,
        acpHosts: AcpClientHostsResponse?,
        to lines: inout [String]
    ) {
        appendSection("Timing", to: &lines)
        if let revision = snapshot.revision {
            lines.append("revision_preview=\(bounded(revision, maxCharacters: Layout.maxRevisionPreviewCharacters)) revision_length=\(revision.count)")
        } else {
            lines.append("revision=none")
        }
        lines.append("threads=\(snapshot.threadCount) active=\(snapshot.activeThreadCount) archived=\(snapshot.archivedThreadCount)")
        for thread in snapshot.threads.prefix(Layout.maxThreadTimingRows) {
            lines.append(
                [
                    "thread=\(thread.threadId)",
                    "created_at_ms=\(optional(thread.createdAtMs))",
                    "updated_at_ms=\(optional(thread.updatedAtMs))",
                    "latest_message_at_ms=\(optional(thread.latestMessageAtMs))",
                ].joined(separator: " ")
            )
        }
        for session in acpHosts?.hosts.flatMap(\.sessions).prefix(Layout.maxAcpSessionTimingRows) ?? [] {
            lines.append("acp_session=\(session.threadID) status=\(session.status) updated_at_ms=\(optional(session.updatedAtMs))")
        }
    }

    private static func appendClassification(_ snapshot: DesktopSnapshotResponse, to lines: inout [String]) {
        appendSection("Classification", to: &lines)
        for thread in snapshot.threads.prefix(Layout.maxClassificationRows) {
            lines.append(
                [
                    "thread=\(thread.threadId)",
                    "assistant=\(thread.capabilities.assistantKind ?? "unknown")",
                    "source=\(thread.source ?? "unknown")",
                    "originator=\(thread.originator ?? "unknown")",
                    "launch=\(thread.capabilities.spawn.launchKind)",
                    "parent=\(thread.capabilities.spawn.parentThreadId ?? "none")",
                    "root=\(thread.capabilities.spawn.rootThreadId)",
                    "children=\(thread.capabilities.spawn.children.count)",
                    "agent=\(thread.capabilities.agentNickname ?? "none")",
                    "role=\(thread.capabilities.agentRole ?? "none")",
                ].joined(separator: " ")
            )
        }
    }

    private static func runtimeText(_ runtime: AcpClientHostRuntime?) -> String {
        guard let runtime else {
            return "runtime=missing"
        }
        return "runtime_connected=\(runtime.connected) runtime_connections=\(runtime.connectionCount) runtime_sessions=\(runtime.sessionCount)"
    }

    private static func appendSection(_ title: String, to lines: inout [String]) {
        lines.append("")
        lines.append("[\(title)]")
    }

    private static func routeLine(_ endpoint: ControlPlaneEndpoint) -> String {
        "route=\(endpoint.method) \(endpointDisplayPath(endpoint))"
    }

    private static func endpointDisplayPath(_ endpoint: ControlPlaneEndpoint) -> String {
        guard !endpoint.queryItems.isEmpty else {
            return endpoint.path
        }

        let query = endpoint.queryItems.map { item in
            guard let value = item.value else {
                return item.name
            }
            return "\(item.name)=\(value)"
        }.joined(separator: "&")
        return "\(endpoint.path)?\(query)"
    }

    private static func joined(_ values: [String]) -> String {
        values.isEmpty ? "none" : values.joined(separator: ",")
    }

    private static func optional(_ value: Int64?) -> String {
        value.map(String.init) ?? "none"
    }

    private static func bounded(_ value: String, maxCharacters: Int) -> String {
        guard value.count > maxCharacters else {
            return value
        }
        return "\(value.prefix(maxCharacters))..."
    }
}
