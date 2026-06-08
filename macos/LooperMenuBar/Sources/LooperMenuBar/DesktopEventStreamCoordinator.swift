import Foundation
import LooperMenuBarCore
import LooperRealtime
import OSLog

@MainActor
final class DesktopEventStreamCoordinator {
    private enum Layout {
        static let loggingSubsystem = "dev.looper.app.ios"
        static let loggingCategory = "desktop-events"
        static let reconnectDelay: Duration = .seconds(2)
        static let refreshDebounce: Duration = .milliseconds(250)
        static let eventLinePrefix = "event:"
        static let connectedEventName = "connected"
    }

    private let client: HTTPControlPlaneClient
    private let session: URLSession
    private let logger = Logger(subsystem: Layout.loggingSubsystem, category: Layout.loggingCategory)
    private let refreshMenu: @MainActor () async -> Void
    private var streamTask: Task<Void, Never>?
    private var pendingRefreshTask: Task<Void, Never>?

    init(
        client: HTTPControlPlaneClient,
        session: URLSession = .shared,
        refreshMenu: @escaping @MainActor () async -> Void
    ) {
        self.client = client
        self.session = session
        self.refreshMenu = refreshMenu
    }

    func start() {
        guard streamTask == nil else {
            return
        }

        streamTask = Task { [weak self] in
            await self?.runStreamLoop()
        }
    }

    func stop() {
        streamTask?.cancel()
        pendingRefreshTask?.cancel()
        streamTask = nil
        pendingRefreshTask = nil
    }

    private func runStreamLoop() async {
        while !Task.isCancelled {
            do {
                try await consumeStream()
            } catch {
                guard !Task.isCancelled else {
                    return
                }
                logger.debug("desktop event stream reconnecting error=\(error.localizedDescription, privacy: .public)")
            }

            try? await Task.sleep(for: Layout.reconnectDelay)
        }
    }

    private func consumeStream() async throws {
        if let realtimeClient = try? await realtimeClient() {
            do {
                try await realtimeClient.streamDesktopEvents { [weak self] event in
                    await MainActor.run {
                        self?.scheduleRefresh(reason: event.eventName)
                    }
                }
                return
            } catch {
                logger.debug("desktop gRPC event stream failed error=\(error.localizedDescription, privacy: .public)")
            }
        }

        try await consumeSSEStream()
    }

    private func consumeSSEStream() async throws {
        let request = client.request(for: .desktopEvents)
        let (bytes, response) = try await session.bytes(for: request)
        try validateEventStreamResponse(response)
        logger.info("desktop event stream connected")
        scheduleRefresh(reason: Layout.connectedEventName)

        for try await line in bytes.lines {
            guard !Task.isCancelled else {
                return
            }
            guard let eventName = eventName(from: line) else {
                continue
            }
            scheduleRefresh(reason: eventName)
        }
    }

    private func realtimeClient() async throws -> LooperRealtimeClient? {
        let health = try await client.fetchMobileHealth()
        let endpoints = health.preferredRealtimeBaseURLs.map(LooperRealtimeEndpoint.init(baseURL:))
        guard !endpoints.isEmpty else {
            return nil
        }
        return LooperRealtimeClient(
            endpoints: endpoints,
            credentials: LooperRealtimeCredentials(bearerToken: nil, mobileSessionHeader: nil)
        )
    }

    private func scheduleRefresh(reason: String) {
        pendingRefreshTask?.cancel()
        pendingRefreshTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: Layout.refreshDebounce)
            guard !Task.isCancelled else {
                return
            }
            self?.logger.debug("desktop event stream refreshing menu event=\(reason, privacy: .public)")
            await self?.refreshMenu()
        }
    }

    private func eventName(from line: String) -> String? {
        guard line.hasPrefix(Layout.eventLinePrefix) else {
            return nil
        }

        let eventName = line
            .dropFirst(Layout.eventLinePrefix.count)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return eventName.isEmpty ? nil : eventName
    }

    private func validateEventStreamResponse(_ response: URLResponse) throws {
        guard let response = response as? HTTPURLResponse else {
            throw ControlPlaneClientError.invalidResponse
        }
        guard (200..<300).contains(response.statusCode) else {
            throw ControlPlaneClientError.requestFailed(statusCode: response.statusCode)
        }
    }
}
