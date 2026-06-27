import Foundation
import Testing
@testable import LooperMenuBarCore

struct LooperDiagnosticsContentTests {
    @Test
    func rendersAcpTimingRoutesAndClassification() {
        let report = LooperDiagnosticsContent.report(
            from: MenuRefreshResult(
                didFetchHTTP: true,
                sessionMiniSnapshot: nil,
                snapshot: snapshot(),
                connections: nil,
                acpClientHosts: acpClientHosts(),
                mobileState: nil,
                pushDevices: nil,
                mobileHealth: MobileHealthResponse(
                    ok: true,
                    baseURL: "http://127.0.0.1:8765",
                    baseURLs: ["http://100.119.200.69:8765"],
                    grpcBaseURL: "http://100.119.200.69:8766",
                    grpcBaseURLs: ["http://100.119.200.69:8766"],
                    requiresAuthentication: true
                ),
                error: nil
            ),
            mobileRoutePreference: .lan,
            generatedAt: Date(timeIntervalSince1970: 0)
        )

        #expect(report.contains("Looper Diagnostics"))
        #expect(report.contains("diagnostics_generated_at=1970-01-01T00:00:00Z"))
        #expect(report.contains("route_preference=lan"))
        #expect(report.contains("[ACP Hosts]"))
        #expect(report.contains("host=devin label=Devin Desktop running=true installed=true actions=install,probe"))
        #expect(report.contains("agent=looper name=Looper control=agent-configured prompt=true sessions=true"))
        #expect(report.contains("host=zed label=Zed running=true installed=true actions=probe"))
        #expect(report.contains("[ACP Targets]"))
        #expect(report.contains("target=zed:looper title=Zed: Looper status=Read-only - command"))
        #expect(report.contains("[Routes]"))
        #expect(report.contains("route=GET /desktop/snapshot?profile=menu"))
        #expect(report.contains("route=POST /hooks/unregister"))
        #expect(report.contains("route=POST /hooks/claude/unregister-live"))
        #expect(report.contains("action=install method=POST path=/desktop/acp-client-hosts/devin/install"))
        #expect(report.contains("[Timing]"))
        #expect(report.contains("revision_preview=rev-7 revision_length=5"))
        #expect(report.contains("thread=child-thread created_at_ms=100 updated_at_ms=300 latest_message_at_ms=400"))
        #expect(report.contains("acp_session=devin-thread status=active updated_at_ms=500"))
        #expect(report.contains("[Classification]"))
        #expect(report.contains("thread=child-thread assistant=codex source=desktop originator=acp launch=subagent parent=parent-thread root=parent-thread children=0 agent=Noether role=explorer"))
    }

    @Test
    func rendersUnavailableRefreshError() {
        let report = LooperDiagnosticsContent.report(from: MenuRefreshResult(
            didFetchHTTP: true,
            sessionMiniSnapshot: nil,
            snapshot: nil,
            connections: nil,
            acpClientHosts: nil,
            mobileState: nil,
            pushDevices: nil,
            mobileHealth: nil,
            error: MenuRefreshError(ControlPlaneClientError.timeout)
        ))

        #expect(report.contains("State: unavailable"))
        #expect(report.contains("[Error]"))
        #expect(report.contains("refresh_error=timeout"))
    }

    private func snapshot() -> DesktopSnapshotResponse {
        DesktopSnapshotResponse(
            revision: "rev-7",
            controlPlane: ControlPlaneStatusResponse(
                hooks: HookStatusSummary(
                    enabled: true,
                    registeredEvents: ["user-prompt-submit", "assistant-response-delta"],
                    activeCommand: "/usr/local/bin/looper hook",
                    owner: "looper",
                    health: "healthy",
                    issues: [],
                    recentFailuresCount: 0
                ),
                codexServers: [],
                source: SourceStatusSummary(
                    codexHome: "/Users/test/.codex",
                    stateDb: "/Users/test/.codex/state.sqlite",
                    logsDb: nil,
                    sessionsRoot: "/Users/test/.codex/sessions",
                    health: "healthy",
                    degradedReason: nil
                )
            ),
            devinDesktop: DevinDesktopStatus(acpBridge: DevinAcpBridgeStatus(
                available: true,
                controlLevel: "agent-configured",
                summary: "Looper visible",
                actions: [],
                agents: []
            )),
            threadCount: 1,
            activeThreadCount: 1,
            archivedThreadCount: 0,
            threads: [
                DesktopThreadSummary(
                    threadId: "child-thread",
                    title: "Check classifications",
                    cwd: "/tmp/looper",
                    transcriptPath: nil,
                    source: "desktop",
                    originator: "acp",
                    model: nil,
                    reasoningEffort: nil,
                    createdAtMs: 100,
                    updatedAtMs: 300,
                    latestMessageAtMs: 400,
                    assistantPreview: nil,
                    archived: false,
                    capabilities: ThreadCapabilitiesSummary(
                        threadId: "child-thread",
                        assistantKind: "codex",
                        mcpTools: [],
                        appTools: [],
                        automationTools: [],
                        spawn: SpawnGraphSummary(
                            parentThreadId: "parent-thread",
                            rootThreadId: "parent-thread",
                            children: [],
                            launchKind: "subagent"
                        ),
                        agentNickname: "Noether",
                        agentRole: "explorer",
                        agentPath: nil
                    )
                ),
            ],
            automations: [],
            goals: [],
            compactions: [],
            assistantAdapters: [],
            acpTargets: [
                AcpTargetSummary(
                    id: "zed:looper",
                    client: "zed",
                    clientName: "Zed",
                    agentId: "looper",
                    name: "Looper",
                    source: "zed-agent-servers",
                    sourcePath: "/Users/test/.config/zed/settings.json",
                    enabled: true,
                    preferred: false,
                    launchConfigured: true,
                    launch: AcpLaunchMetadataSummary(configured: true, methods: ["command"]),
                    ready: false,
                    status: "read-only",
                    detail: "Zed target is read-only."
                ),
            ]
        )
    }

    private func acpClientHosts() -> AcpClientHostsResponse {
        AcpClientHostsResponse(hosts: [
            AcpClientHost(
                id: "devin",
                label: "Devin Desktop",
                running: true,
                installed: true,
                registry: AcpClientHostRegistry(
                    path: "/Users/test/Library/Application Support/devin/acp.json",
                    exists: true,
                    version: "1",
                    agentCount: 1
                ),
                agents: [
                    AcpClientHostAgent(
                        id: "looper",
                        name: "Looper",
                        version: nil,
                        description: nil,
                        enabled: true,
                        preferred: true,
                        launchConfigured: true,
                        controlLevel: "agent-configured",
                        supportsSessions: true,
                        supportsPrompt: true,
                        supportsCancel: false,
                        source: "registry"
                    ),
                ],
                sessions: [
                    AcpClientHostSession(
                        threadID: "devin-thread",
                        sessionID: "devin-session",
                        providerID: "devin",
                        title: "Active Devin",
                        cwd: "/tmp/looper",
                        status: "active",
                        archived: false,
                        updatedAtMs: 500
                    ),
                ],
                actions: [
                    AcpClientHostAction(
                        id: "install",
                        label: "Install",
                        method: "POST",
                        path: "/desktop/acp-client-hosts/devin/install",
                        defaultAgentID: "looper"
                    ),
                    AcpClientHostAction(
                        id: "probe",
                        label: "Probe",
                        method: "POST",
                        path: "/desktop/acp-client-hosts/devin/probe",
                        defaultAgentID: "looper"
                    ),
                ],
                limitations: [],
                runtime: AcpClientHostRuntime(connected: true, connectionCount: 1, sessionCount: 1)
            ),
            AcpClientHost(
                id: "zed",
                label: "Zed",
                running: true,
                installed: true,
                registry: AcpClientHostRegistry(
                    path: "/Users/test/.config/zed/settings.json",
                    exists: true,
                    version: nil,
                    agentCount: 1
                ),
                agents: [],
                sessions: [],
                actions: [
                    AcpClientHostAction(
                        id: "probe",
                        label: "Probe",
                        method: "POST",
                        path: "/desktop/acp-client-hosts/zed/probe",
                        defaultAgentID: nil
                    ),
                ],
                limitations: ["visibility-only"],
                runtime: nil
            ),
        ])
    }
}
