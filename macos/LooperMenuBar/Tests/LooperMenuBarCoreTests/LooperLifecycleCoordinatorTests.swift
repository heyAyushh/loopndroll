import Foundation
import Testing
@testable import LooperMenuBarCore

@Suite("Looper lifecycle coordinator")
struct LooperLifecycleCoordinatorTests {
    @Test("launch starts the Rust service and registers hooks")
    func launchStartsServiceAndRegistersHooks() async throws {
        let client = RecordingControlPlaneClient()
        let service = RecordingControlPlaneService()
        let coordinator = LooperLifecycleCoordinator(client: client, service: service)

        let result = await coordinator.registerOnLaunch()

        try result.get()
        #expect(service.calls == [.start])
        #expect(client.calls == [.register])
    }

    @Test("launch does not register hooks when service start fails")
    func launchDoesNotRegisterHooksWhenServiceStartFails() async throws {
        let client = RecordingControlPlaneClient()
        let service = RecordingControlPlaneService(startResult: .failure(RecordingServiceError.failed))
        let coordinator = LooperLifecycleCoordinator(client: client, service: service)

        let result = await coordinator.registerOnLaunch()

        #expect(throws: RecordingServiceError.self) {
            try result.get()
        }
        #expect(service.calls == [.start])
        #expect(client.calls.isEmpty)
    }

    @Test("terminate clears live hooks without disabling next launch")
    func terminateClearsLiveHooks() throws {
        let client = RecordingControlPlaneClient()
        let service = RecordingControlPlaneService()
        let coordinator = LooperLifecycleCoordinator(client: client, service: service)

        let result = coordinator.unregisterBeforeQuit(timeout: 1)

        try result.get()
        #expect(client.calls == [.unregisterLive(timeout: 1), .shutdown(timeout: 1)])
        #expect(service.calls == [.stop])
    }

    @Test("detached terminate leaves service running")
    func detachedTerminateLeavesServiceRunning() throws {
        let client = RecordingControlPlaneClient()
        let service = RecordingControlPlaneService()
        let coordinator = LooperLifecycleCoordinator(client: client, service: service)

        let result = coordinator.unregisterBeforeQuit(timeout: 1, stopService: false)

        try result.get()
        #expect(client.calls == [.unregisterLive(timeout: 1)])
        #expect(service.calls.isEmpty)
    }

    @Test("manual stop shuts down service")
    func manualStopShutsDownService() {
        let client = RecordingControlPlaneClient()
        let service = RecordingControlPlaneService()
        let coordinator = LooperLifecycleCoordinator(client: client, service: service)

        coordinator.shutdownServer(timeout: 1)

        #expect(client.calls == [.shutdown(timeout: 1)])
        #expect(service.calls == [.stop])
    }
}

@Suite("HTTP control plane client")
struct HTTPControlPlaneClientTests {
    @Test("uses Rust lifecycle endpoints")
    func usesRustLifecycleEndpoints() {
        let client = HTTPControlPlaneClient(baseURL: URL(string: "http://127.0.0.1:8765")!)

        #expect(client.request(for: .registerHooks).url?.path == "/hooks/register")
        #expect(client.request(for: .unregisterLiveHooks).url?.path == "/hooks/unregister-live")
        #expect(client.request(for: .shutdown).url?.path == "/desktop/shutdown")
        #expect(client.request(for: .mobileHealth).url?.path == "/api/mobile/health")
        #expect(client.request(for: .controlPlaneStatus).url?.path == "/status/control-plane")
        #expect(client.request(for: .desktopSnapshot).url?.path == "/desktop/snapshot")
        #expect(client.request(for: .desktopSnapshot).url?.query == "profile=menu")
        #expect(client.request(for: .registerHooks).httpMethod == "POST")
        #expect(client.request(for: .shutdown).httpMethod == "POST")
        #expect(client.request(for: .mobileHealth).httpMethod == "GET")
        #expect(client.request(for: .desktopSnapshot).httpMethod == "GET")
    }

    @Test("reads requests from shared endpoint store")
    func readsRequestsFromSharedEndpointStore() {
        let endpointStore = ControlPlaneEndpointStore()
        let client = HTTPControlPlaneClient(endpointStore: endpointStore)

        endpointStore.baseURL = URL(string: "http://127.0.0.1:8766")!

        #expect(client.request(for: .desktopSnapshot).url?.absoluteString == "http://127.0.0.1:8766/desktop/snapshot?profile=menu")
    }

    @Test("builds fallback listen candidates")
    func buildsFallbackListenCandidates() {
        let candidates = BundledControlPlaneService.listenCandidates(environment: [:])

        #expect(candidates.count == 16)
        #expect(candidates.prefix(4).map(\.listenAddress) == [
            "0.0.0.0:8765",
            "0.0.0.0:8766",
            "0.0.0.0:8767",
            "0.0.0.0:8768",
        ])
        #expect(candidates.suffix(1).map(\.listenAddress) == ["0.0.0.0:8780"])
        #expect(candidates.prefix(4).map(\.baseURL.absoluteString) == [
            "http://127.0.0.1:8765",
            "http://127.0.0.1:8766",
            "http://127.0.0.1:8767",
            "http://127.0.0.1:8768",
        ])
    }

    @Test("uses configured listen candidate")
    func usesConfiguredListenCandidate() {
        let candidates = BundledControlPlaneService.listenCandidates(
            environment: ["AGENT_CONTROL_PLANE_LISTEN": "0.0.0.0:9000"]
        )

        #expect(candidates.map(\.listenAddress) == ["0.0.0.0:9000"])
        #expect(candidates.map(\.baseURL.absoluteString) == ["http://127.0.0.1:9000"])
    }

    @Test("removes Codex sandbox variables from service launch")
    func removesCodexSandboxVariablesFromServiceLaunch() {
        let environment = BundledControlPlaneService.sanitizedLaunchEnvironment([
            "CODEX_SANDBOX_NETWORK_DISABLED": "1",
            "HOME": "/Users/test",
            "PATH": "/usr/bin",
        ])

        #expect(environment["CODEX_SANDBOX_NETWORK_DISABLED"] == nil)
        #expect(environment["HOME"] == "/Users/test")
        #expect(environment["PATH"] == "/usr/bin")
    }

    @Test("accepts only Looper health responses")
    func acceptsOnlyLooperHealthResponses() throws {
        let response = try #require(HTTPURLResponse(
            url: URL(string: "http://127.0.0.1:8765/health")!,
            statusCode: 200,
            httpVersion: nil,
            headerFields: nil
        ))
        let looperHealth = Data(
            """
            {"service":"looper"}
            """.utf8
        )
        let foreignHealth = Data(
            """
            {"status":"ok","platform":"hermes-agent"}
            """.utf8
        )

        #expect(BundledControlPlaneService.isLooperHealthResponse(data: looperHealth, response: response))
        #expect(!BundledControlPlaneService.isLooperHealthResponse(data: foreignHealth, response: response))
    }

    @Test("decodes mobile health for human status")
    func decodesMobileHealth() throws {
        let data = Data(
            """
            {
              "ok": true,
              "baseURL": "http://192.168.1.4:8765",
              "baseURLs": [
                "http://192.168.1.4:8765",
                "http://127.0.0.1:8765"
              ],
              "requiresAuthentication": true
            }
            """.utf8
        )

        let health = try JSONDecoder().decode(MobileHealthResponse.self, from: data)

        #expect(health.ok)
        #expect(health.preferredHandoffBaseURL?.absoluteString == "http://192.168.1.4:8765")
        #expect(health.requiresAuthentication)
    }

    @Test("builds human status for lifecycle awareness")
    func buildsHumanStatus() async throws {
        let client = RecordingControlPlaneClient()
        let snapshot = try await client.fetchDesktopSnapshot()

        let status = LooperHumanStatus.from(
            snapshot: snapshot,
            mobileHealth: MobileHealthResponse(
                ok: true,
                baseURL: "http://192.168.1.4:8765",
                baseURLs: ["http://192.168.1.4:8765", "http://127.0.0.1:8765"],
                requiresAuthentication: true
            ),
            detachOnQuit: false
        )

        #expect(status.kind == .ready)
        #expect(status.title == "Ready")
        #expect(status.lifecycle == "Quit stops server")
        #expect(status.detail.contains("iPhone=ready"))
    }

    @Test("decodes control plane status with Codex servers")
    func decodesControlPlaneStatus() throws {
        let data = Data(
            """
            {
              "hooks": {
                "enabled": true,
                "registered_events": ["SessionStart", "Stop"],
                "active_command": "agent-control-plane --hook --managed-by looper",
                "owner": "looper-rust",
                "health": "healthy",
                "issues": [],
                "recent_failures_count": 0
              },
              "app_server": null,
              "codex_servers": [
                {
                  "pid": 100,
                  "parent_pid": 1,
                  "tty": "??",
                  "executable": "/Applications/Codex.app/Contents/Resources/codex",
                  "command": "/Applications/Codex.app/Contents/Resources/codex app-server",
                  "owner": "codex-app",
                  "parent_processes": []
                }
              ],
              "source": {
                "codex_home": "/Users/test/.codex",
                "state_db": "/Users/test/.codex/state.sqlite",
                "logs_db": null,
                "sessions_root": "/Users/test/.codex/sessions",
                "health": "healthy",
                "degraded_reason": null
              }
            }
            """.utf8
        )

        let status = try JSONDecoder().decode(ControlPlaneStatusResponse.self, from: data)

        #expect(status.hooks.owner == "looper-rust")
        #expect(status.codexServers[0].owner == "codex-app")
        #expect(status.source.health == "healthy")
    }

    @Test("decodes desktop snapshot for menu content")
    func decodesDesktopSnapshot() throws {
        let data = Data(
            """
            {
              "control_plane": {
                "hooks": {
                  "enabled": true,
                  "registered_events": ["SessionStart", "Stop"],
                  "active_command": "agent-control-plane --hook --managed-by looper",
                  "owner": "looper-rust",
                  "health": "healthy",
                  "issues": [],
                  "recent_failures_count": 0
                },
                "app_server": null,
                "codex_servers": [],
                "source": {
                  "codex_home": "/Users/test/.codex",
                  "state_db": "/Users/test/.codex/state.sqlite",
                  "logs_db": null,
                  "sessions_root": "/Users/test/.codex/sessions",
                  "health": "healthy",
                  "degraded_reason": null
                }
              },
              "devin_desktop": {
                "acp_bridge": {
                  "available": true,
                  "control_level": "client-capable",
                  "summary": "Devin ACP bridge can attach to configured agent transports after explicit approval",
                  "actions": [],
                  "agents": []
                }
              },
              "thread_count": 2,
              "active_thread_count": 1,
              "archived_thread_count": 1,
              "threads": [
                {
                  "thread_id": "thread-main",
                  "title": "Build Looper",
                  "cwd": "/Users/test/project",
                  "transcript_path": "/Users/test/.codex/sessions/thread-main.jsonl",
                  "source": "desktop",
                  "model": "gpt-5.5",
                  "reasoning_effort": "high",
                  "git_sha": "abc",
                  "git_branch": "main",
                  "cli_version": "0.124.0",
                  "agent_nickname": null,
                  "agent_role": null,
                  "agent_path": null,
                  "created_at_ms": 1000,
                  "updated_at_ms": 2000,
                  "archived": false,
                  "capabilities": {
                    "thread_id": "thread-main",
                    "assistant_kind": "codex",
                    "tools": [],
                    "mcp_tools": ["filesystem"],
                    "app_tools": [],
                    "automation_tools": ["automation_update"],
                    "spawn": {
                      "parent_thread_id": null,
                      "root_thread_id": "thread-main",
                      "children": ["thread-child"],
                      "launch_kind": "main"
                    },
                    "diff": {
                      "git_branch": "main",
                      "git_sha": "abc",
                      "produced_file_changes": false,
                      "paths": []
                    },
                    "agent_nickname": null,
                    "agent_role": null,
                    "agent_path": null
                  }
                }
              ],
              "automations": [
                {
                  "id": "daily",
                  "kind": "heartbeat",
                  "name": "Daily Review",
                  "status": "ACTIVE",
                  "rrule": "FREQ=MINUTELY;INTERVAL=5",
                  "schedule_summary": "MINUTELY every 5",
                  "target_thread_id": "thread-main",
                  "target_known": true,
                  "control_plane_covered": true,
                  "source_path": "/Users/test/.codex/automations/daily/automation.toml"
                }
              ],
              "automation_runs": [],
              "goals": [
                {
                  "id": "ship-looper",
                  "title": "Ship Looper",
                  "status": "pursuing",
                  "lifecycle": "pursuing",
                  "priority": "high",
                  "target_thread_id": "thread-main",
                  "target_known": true,
                  "source_kind": "toml",
                  "source_path": "/Users/test/.codex/goals/ship-looper/goal.toml",
                  "updated_at_ms": 1000,
                  "content_hash": "abc",
                  "sync_safe": true
                }
              ],
              "sync_manifest": {
                "schema_version": 1,
                "manifest_id": "manifest-local",
                "generated_at_ms": 1000,
                "privacy": {
                  "profile": "metadata-only",
                  "raw_goal_bodies": false,
                  "raw_automation_prompts": false,
                  "raw_thread_logs": false,
                  "credentials": false,
                  "source_paths": false
                },
                "goals": [],
                "automations": [],
                "threads": [],
                "hooks": {
                  "enabled": true,
                  "owner": "looper-rust",
                  "health": "healthy",
                  "registered_events": ["SessionStart"]
                }
              },
              "assistant_adapters": [
                {
                  "assistant_kind": "grok-build",
                  "live_sessions": false,
                  "tool_inventory": false,
                  "spawn_graph": false,
                  "diff_summary": false,
                  "auth_capabilities": false,
                  "runtimes": [
                    {
                      "kind": "cli",
                      "running": true,
                      "installed": true,
                      "label": "Grok Build CLI",
                      "bundle_id": null,
                      "executable": "/Users/test/.grok/bin/grok",
                      "command": null
                    }
                  ],
                  "detail": "CLI runtime detection only"
                }
              ],
              "compactions": [
                {
                  "event_id": "event-1",
                  "event_type": "codex.context_compacted",
                  "thread_id": "thread-main",
                  "occurred_at": "2026-05-03T12:00:00Z",
                  "rollout_path": "/Users/test/.codex/sessions/thread-main.jsonl",
                  "line_number": 12
                }
              ]
            }
            """.utf8
        )

        let snapshot = try JSONDecoder().decode(DesktopSnapshotResponse.self, from: data)

        #expect(snapshot.threadCount == 2)
        #expect(snapshot.activeThreadCount == 1)
        #expect(snapshot.controlPlane.hooks.health == "healthy")
        #expect(snapshot.threads[0].title == "Build Looper")
        #expect(snapshot.threads[0].transcriptPath == "/Users/test/.codex/sessions/thread-main.jsonl")
        #expect(snapshot.threads[0].capabilities.spawn.children == ["thread-child"])
        #expect(snapshot.automations[0].controlPlaneCovered)
        #expect(snapshot.goals[0].title == "Ship Looper")
        #expect(snapshot.compactions[0].eventType == "codex.context_compacted")
        #expect(snapshot.grokBuildStatusTitle == "CLI running")
        #expect(snapshot.grokBuildAdapter?.assistantKind == "grok-build")
    }

    @Test("decodes desktop snapshot without assistant adapters")
    func decodesDesktopSnapshotWithoutAssistantAdapters() throws {
        let data = Data(
            """
            {
              "control_plane": {
                "hooks": {
                  "enabled": true,
                  "registered_events": [],
                  "active_command": null,
                  "owner": "looper-rust",
                  "health": "healthy",
                  "issues": [],
                  "recent_failures_count": 0
                },
                "codex_servers": [],
                "source": {
                  "codex_home": "/Users/test/.codex",
                  "state_db": null,
                  "logs_db": null,
                  "sessions_root": "/Users/test/.codex/sessions",
                  "health": "healthy",
                  "degraded_reason": null
                }
              },
              "devin_desktop": {
                "acp_bridge": {
                  "available": false,
                  "control_level": "visibility-only",
                  "summary": "Unavailable",
                  "actions": [],
                  "agents": []
                }
              },
              "thread_count": 0,
              "active_thread_count": 0,
              "archived_thread_count": 0,
              "threads": [],
              "automations": [],
              "goals": [],
              "compactions": []
            }
            """.utf8
        )

        let snapshot = try JSONDecoder().decode(DesktopSnapshotResponse.self, from: data)

        #expect(snapshot.assistantAdapters.isEmpty)
        #expect(snapshot.grokBuildStatusTitle == "Unavailable")
    }
}

private enum RecordingCall: Equatable {
    case register
    case unregisterLive(timeout: TimeInterval)
    case shutdown(timeout: TimeInterval)
}

private enum RecordingServiceCall: Equatable {
    case start
    case stop
}

private enum RecordingServiceError: Error {
    case failed
}

private func decodeFixture<Response: Decodable>(_ json: String) throws -> Response {
    try JSONDecoder().decode(Response.self, from: Data(json.utf8))
}

private func emptyDevinBridge() -> DevinAcpBridgeStatus {
    DevinAcpBridgeStatus(
        available: false,
        controlLevel: "visibility-only",
        summary: "Unavailable",
        actions: [],
        agents: []
    )
}

private final class RecordingControlPlaneClient: ControlPlaneClient, @unchecked Sendable {
    private let lock = NSLock()
    private var recordedCalls: [RecordingCall] = []

    var calls: [RecordingCall] {
        lock.withLock {
            recordedCalls
        }
    }

    func registerHooks() async throws {
        lock.withLock {
            recordedCalls.append(.register)
        }
    }

    func unregisterLiveHooks(timeout: TimeInterval) throws {
        lock.withLock {
            recordedCalls.append(.unregisterLive(timeout: timeout))
        }
    }

    func shutdownServer(timeout: TimeInterval) throws {
        lock.withLock {
            recordedCalls.append(.shutdown(timeout: timeout))
        }
    }

    func fetchControlPlaneStatus() async throws -> ControlPlaneStatusResponse {
        ControlPlaneStatusResponse(
            hooks: HookStatusSummary(
                enabled: true,
                registeredEvents: ["SessionStart"],
                activeCommand: nil,
                owner: "looper-rust",
                health: "healthy",
                issues: [],
                recentFailuresCount: 0
            ),
            codexServers: [],
            source: SourceStatusSummary(
                codexHome: "/tmp/.codex",
                stateDb: nil,
                logsDb: nil,
                sessionsRoot: "/tmp/.codex/sessions",
                health: "healthy",
                degradedReason: nil
            )
        )
    }

    func probeDevinAcpBridge(agentId: String?) async throws -> DevinAcpBridgeProbeResponse {
        try decodeFixture(
            """
            {
              "probe": {
                "ok": false,
                "status": "blocked",
                "attach_ready": false,
                "launch_configured": false,
                "blockers": [],
                "detail": "test"
              },
              "bridge": {
                "available": false,
                "control_level": "visibility-only",
                "summary": "Unavailable",
                "actions": [],
                "agents": []
              }
            }
            """
        )
    }

    func attachDevinAcpBridge(agentId: String?) async throws -> DevinAcpBridgeAttachResponse {
        try decodeFixture(
            """
            {
              "attach": {
                "ok": false,
                "status": "blocked",
                "attach_ready": false,
                "detail": "test",
                "blockers": [],
                "limitations": []
              },
              "bridge": {
                "available": false,
                "control_level": "visibility-only",
                "summary": "Unavailable",
                "actions": [],
                "agents": []
              }
            }
            """
        )
    }

    func fetchDesktopSnapshot() async throws -> DesktopSnapshotResponse {
        DesktopSnapshotResponse(
            controlPlane: try await fetchControlPlaneStatus(),
            devinDesktop: DevinDesktopStatus(acpBridge: emptyDevinBridge()),
            threadCount: 0,
            activeThreadCount: 0,
            archivedThreadCount: 0,
            threads: [],
            automations: [],
            goals: [],
            compactions: [],
            assistantAdapters: []
        )
    }

    func fetchMobileHealth() async throws -> MobileHealthResponse {
        MobileHealthResponse(
            ok: true,
            baseURL: "http://192.168.1.4:8765",
            baseURLs: ["http://192.168.1.4:8765", "http://127.0.0.1:8765"],
            requiresAuthentication: true
        )
    }
}

private final class RecordingControlPlaneService: ControlPlaneService, @unchecked Sendable {
    private let lock = NSLock()
    private let startResult: Result<Void, Error>
    private var recordedCalls: [RecordingServiceCall] = []

    init(startResult: Result<Void, Error> = .success(())) {
        self.startResult = startResult
    }

    var calls: [RecordingServiceCall] {
        lock.withLock {
            recordedCalls
        }
    }

    func startIfNeeded() async throws {
        lock.withLock {
            recordedCalls.append(.start)
        }
        try startResult.get()
    }

    func stop() {
        lock.withLock {
            recordedCalls.append(.stop)
        }
    }
}
