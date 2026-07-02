import Foundation

@MainActor
public final class LooperContinuationActivityStorePublisher: @unchecked Sendable {
    public static let defaultDebounceDuration: Duration = .seconds(2)

    private let debounceDuration: Duration
    private let publish: (LooperContinuationActivityDescriptor) -> Void
    private var pendingPublishTask: Task<Void, Never>?

    public init(
        debounceDuration: Duration = LooperContinuationActivityStorePublisher.defaultDebounceDuration,
        publish: @escaping (LooperContinuationActivityDescriptor) -> Void
    ) {
        self.debounceDuration = debounceDuration
        self.publish = publish
    }

    public func schedulePublish(
        from snapshot: MenuBarSessionMiniLocalSnapshot,
        handoffBaseURL: URL?
    ) {
        pendingPublishTask?.cancel()
        let debounceDuration = self.debounceDuration
        pendingPublishTask = Task { [weak self, snapshot, handoffBaseURL, debounceDuration] in
            do {
                try await Task.sleep(for: debounceDuration)
            } catch {
                return
            }
            guard !Task.isCancelled else {
                return
            }
            self?.publishNow(from: snapshot, handoffBaseURL: handoffBaseURL)
        }
    }

    public func cancel() {
        pendingPublishTask?.cancel()
        pendingPublishTask = nil
    }

    private func publishNow(
        from snapshot: MenuBarSessionMiniLocalSnapshot,
        handoffBaseURL: URL?
    ) {
        publish(
            LooperContinuationActivityStoreTruth.descriptor(
                from: snapshot,
                handoffBaseURL: handoffBaseURL
            )
        )
        pendingPublishTask = nil
    }
}

public enum LooperContinuationActivityStoreTruth {
    public static func descriptor(
        from snapshot: MenuBarSessionMiniLocalSnapshot,
        handoffBaseURL: URL?
    ) -> LooperContinuationActivityDescriptor {
        LooperContinuationActivityBuilder.descriptor(
            from: snapshot,
            handoffBaseURL: handoffBaseURL
        )
    }

    public static func descriptorForRefreshFailure(
        latestSessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot?,
        fallbackDescriptor: LooperContinuationActivityDescriptor,
        handoffBaseURL: URL?
    ) -> LooperContinuationActivityDescriptor {
        guard let latestSessionMiniSnapshot else {
            return fallbackDescriptor
        }
        return descriptor(
            from: latestSessionMiniSnapshot,
            handoffBaseURL: handoffBaseURL
        )
    }
}
