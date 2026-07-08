import Foundation
import LooperClientCore
import Network

/// Caches the realtime endpoint plan so recovery paths triggered by user
/// gestures never await an HTTP health round-trip.
///
/// The plan is derived from the configured base URLs plus the most recent
/// health payload. Configured URLs alone always produce dialable h2
/// candidates, and the Rust core reorders candidates by its persisted
/// last-good endpoint — so serving a slightly stale plan is safe. Health only
/// enriches the plan with h3 candidates and certificate pins.
///
/// Await rules:
/// - Warm cache: returns synchronously and refreshes health in the background
///   when the cached payload is older than `healthRefreshInterval`.
/// - Cold cache (no health seen for this connection yet): awaits one
///   single-flight health fetch, once per process/connection.
///
/// Invalidation: network-path changes (Wi-Fi <-> LTE, VPN/Tailscale flips)
/// mark the cached health stale so the next request refreshes eagerly while
/// still returning the previous plan; authorization failures clear it.
actor CompanionEndpointPlanCache {
    static let shared = CompanionEndpointPlanCache()

    typealias HealthProvider = @Sendable () async -> CompanionServerHealth?

    private static let healthRefreshInterval: TimeInterval = 60

    private var cachedHealth: CompanionServerHealth?
    private var cachedHealthDate: Date?
    private var cachedConfigurationSignature: String?
    private var inflightHealthFetch: Task<CompanionServerHealth?, Never>?
    private var pathMonitor: NWPathMonitor?
    private var hasObservedInitialPath = false

    func endpoints(
        configuredBaseURLs: [URL],
        healthProvider: @escaping HealthProvider
    ) async -> [ClientEndpoint] {
        startPathMonitorIfNeeded()

        let signature = Self.configurationSignature(for: configuredBaseURLs)
        if cachedConfigurationSignature != signature {
            cachedHealth = nil
            cachedHealthDate = nil
            cachedConfigurationSignature = signature
        }

        if let cachedHealth {
            if isCachedHealthStale {
                refreshHealthInBackground(healthProvider)
            }
            return CompanionRealtimeEndpointResolver.endpoints(
                configuredBaseURLs: configuredBaseURLs,
                health: cachedHealth
            )
        }

        // Cold path: one single-flight fetch per connection; concurrent
        // callers share the same task instead of racing duplicate requests.
        let health = await fetchHealthSingleFlight(healthProvider)
        return CompanionRealtimeEndpointResolver.endpoints(
            configuredBaseURLs: configuredBaseURLs,
            health: health
        )
    }

    func noteAuthorizationFailure() {
        cachedHealth = nil
        cachedHealthDate = nil
    }

    private var isCachedHealthStale: Bool {
        guard let cachedHealthDate else {
            return true
        }
        return Date().timeIntervalSince(cachedHealthDate) > Self.healthRefreshInterval
    }

    private func fetchHealthSingleFlight(
        _ healthProvider: @escaping HealthProvider
    ) async -> CompanionServerHealth? {
        if let inflightHealthFetch {
            return await inflightHealthFetch.value
        }

        let fetch = Task<CompanionServerHealth?, Never> {
            await healthProvider()
        }
        inflightHealthFetch = fetch
        let health = await fetch.value
        inflightHealthFetch = nil
        if let health {
            cachedHealth = health
            cachedHealthDate = Date()
        }
        return health
    }

    private func refreshHealthInBackground(_ healthProvider: @escaping HealthProvider) {
        guard inflightHealthFetch == nil else {
            return
        }
        let fetch = Task<CompanionServerHealth?, Never> {
            await healthProvider()
        }
        inflightHealthFetch = fetch
        Task {
            await self.adoptBackgroundHealthResult(from: fetch)
        }
    }

    private func adoptBackgroundHealthResult(
        from fetch: Task<CompanionServerHealth?, Never>
    ) async {
        let health = await fetch.value
        if inflightHealthFetch == fetch {
            inflightHealthFetch = nil
        }
        if let health {
            cachedHealth = health
            cachedHealthDate = Date()
        }
    }

    private func startPathMonitorIfNeeded() {
        guard pathMonitor == nil else {
            return
        }
        let monitor = NWPathMonitor()
        pathMonitor = monitor
        monitor.pathUpdateHandler = { [weak self] _ in
            guard let self else {
                return
            }
            Task {
                await self.handlePathChange()
            }
        }
        monitor.start(queue: DispatchQueue(label: "dev.looper.endpoint-plan-path"))
    }

    private func handlePathChange() {
        // The first callback fires immediately with the current path; only
        // subsequent changes should mark the plan stale.
        guard hasObservedInitialPath else {
            hasObservedInitialPath = true
            return
        }
        // Keep the cached plan as a fallback but force an eager refresh on
        // the next request — the old candidates may be undialable now.
        cachedHealthDate = .distantPast
    }

    private static func configurationSignature(for configuredBaseURLs: [URL]) -> String {
        configuredBaseURLs.map(\.absoluteString).sorted().joined(separator: "|")
    }
}
