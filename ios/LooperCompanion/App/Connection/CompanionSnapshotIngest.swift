import Foundation
import LooperClientCore
import LooperCompanionCore

private enum CompanionSnapshotIngestMetrics {
    static let millisecondsPerSecond = 1_000.0
}

/// Single funnel for every snapshot apply in the app: the live realtime
/// stream, cache replay at launch/unlock/session-open, background recovery,
/// and the local-truth replay after a client-core command is accepted.
///
/// Before this type existed, "is this candidate fresh enough to show" was
/// answered in three different places with three different guards — a
/// build-reservation generation/seq pair, a `shouldApplyStateMiniSnapshot`
/// seq comparison, and a five-condition guard inside the stream-apply
/// completion — that could disagree with each other. That's the documented
/// cause of a past freeze bug where the visible session list stopped
/// updating because two seq trackers advanced out of step. Every apply path
/// now goes through `ingestLiveStreamUpdate` or `ingestReplaySnapshot`, and
/// both bottom out in the single `shouldAcceptContent` comparison below.
@MainActor
final class CompanionSnapshotIngest {
    /// How a candidate's content was folded into the visible snapshot.
    enum ApplyStrategy: Equatable {
        /// The candidate became the new source snapshot outright.
        case direct
        /// The candidate was rejected as the new source snapshot, but its
        /// sessions on OTHER surfaces were merged into whatever is currently
        /// visible (cache/recovery replay only — never the live stream).
        case broaderMerge
    }

    struct Outcome {
        let reason: String
        let latestSeq: Int64
        let endpointURL: URL?
        let strategy: ApplyStrategy
        let applyResult: CompanionSnapshotApplyResult
        /// True only for the live stream: cache/recovery/local-command
        /// replays are local-only and never prove the stream is live.
        let provesLiveness: Bool
        let sourceSnapshot: MobileSnapshot
    }

    struct Rejection {
        let reason: String
        let latestSeq: Int64
        let endpointURL: URL?
        let sourceSnapshot: MobileSnapshot
        let provesLiveness: Bool
    }

    enum IngestResult {
        case applied(Outcome)
        case rejected(Rejection)
        /// The candidate was superseded before it could even be evaluated
        /// (stream generation moved on, or a newer live-stream frame already
        /// reserved the ordering slot). No side effect — including
        /// liveness — should run for a dropped candidate.
        case dropped
    }

    private let snapshotState: CompanionSnapshotStateStore
    private var currentStreamGeneration: () -> UInt64 = { 0 }
    private var currentRealtimeLatestSeq: () -> Int64 = { 0 }
    /// Stored (not passed per-call) so the async build's completion never has
    /// to carry a closure value across the `Task.detached` boundary — only
    /// `[weak self]` does, which is the pattern the rest of the connection
    /// layer already relies on under Swift 6 strict concurrency checking.
    private var onLiveStreamResult: ((IngestResult) -> Void)?

    /// Bumped on every accepted build reservation. A completing build whose
    /// reservation is no longer current (a newer frame reserved after it, or
    /// the ordering was invalidated by a surface switch / stream restart) is
    /// dropped instead of applied.
    private(set) var generation: UInt64 = 0
    /// Dedups build reservations for the live-stream path only: avoids
    /// spending CPU on a projection build for a frame already known to be
    /// older than one already in flight or applied.
    private var reservedSeq: Int64 = 0
    private var buildTask: Task<Void, Never>?

    init(snapshotState: CompanionSnapshotStateStore) {
        self.snapshotState = snapshotState
    }

    func configure(
        currentStreamGeneration: @escaping () -> UInt64,
        currentRealtimeLatestSeq: @escaping () -> Int64,
        onLiveStreamResult: @escaping (IngestResult) -> Void
    ) {
        self.currentStreamGeneration = currentStreamGeneration
        self.currentRealtimeLatestSeq = currentRealtimeLatestSeq
        self.onLiveStreamResult = onLiveStreamResult
    }

    /// Cancels any in-flight build and bumps the generation so its
    /// completion (if already running) drops instead of applying. Called
    /// whenever something invalidates in-flight ordering: the visible
    /// surface changes, the stream stops/restarts, or the snapshot resets.
    func invalidatePendingBuilds() {
        generation &+= 1
        buildTask?.cancel()
        buildTask = nil
    }

    /// Full reset for a fresh connection: invalidates any in-flight build
    /// AND clears the build-dedup floor so the next frame — whatever its
    /// seq — is accepted for building again.
    func resetOrdering() {
        invalidatePendingBuilds()
        reservedSeq = 0
    }

    // MARK: - Live stream

    /// Builds `CompanionPreparedSnapshotProjection` off-main (cancelling any
    /// previous build) and applies it through the single acceptance gate.
    /// Replaces the old `applySessionMiniSyncUpdate` +
    /// `reserveSessionMiniProjectionBuild` + `applyPreparedSessionMiniSyncSnapshot`
    /// pipeline, now as one funnel entry point.
    func ingestLiveStreamUpdate(
        _ update: CompanionSessionMiniSyncUpdate,
        streamGeneration: UInt64
    ) {
        guard streamGeneration == currentStreamGeneration() else {
            CompanionDiagnostics.record("session-mini:sync-stale-skip")
            return
        }
        guard update.latestSeq >= reservedSeq else {
            CompanionDiagnostics.record("session-mini:projection-stale-drop seq=\(update.latestSeq)")
            return
        }

        generation &+= 1
        reservedSeq = update.latestSeq
        let reservedGeneration = generation
        let builtSurface = snapshotState.selectedAssistantSurface

        buildTask?.cancel()
        buildTask = Task.detached(priority: .userInitiated) { [weak self] in
            let buildStartedAt = Date()
            let prepared = CompanionPreparedSnapshotProjection.build(
                snapshot: update.snapshot,
                surface: builtSurface
            )
            let durationMilliseconds = Int(
                (Date().timeIntervalSince(buildStartedAt) * CompanionSnapshotIngestMetrics.millisecondsPerSecond)
                    .rounded()
            )
            CompanionDiagnostics.record(
                "session-mini:projection-built seq=\(update.latestSeq) ms=\(durationMilliseconds)"
            )
            await MainActor.run { [weak self] in
                guard let self else { return }
                if reservedGeneration == self.generation {
                    self.buildTask = nil
                }
                let result = self.completeLiveStreamIngest(
                    prepared,
                    reason: update.reason,
                    latestSeq: update.latestSeq,
                    endpointURL: update.endpointURL,
                    streamGeneration: streamGeneration,
                    reservedGeneration: reservedGeneration,
                    builtSurface: builtSurface
                )
                self.onLiveStreamResult?(result)
            }
        }
    }

    private func completeLiveStreamIngest(
        _ prepared: CompanionPreparedSnapshotProjection,
        reason: String,
        latestSeq: Int64,
        endpointURL: URL?,
        streamGeneration: UInt64,
        reservedGeneration: UInt64,
        builtSurface: CompanionAssistantSurface
    ) -> IngestResult {
        guard streamGeneration == currentStreamGeneration(),
              reservedGeneration == generation,
              latestSeq >= reservedSeq,
              builtSurface == snapshotState.selectedAssistantSurface,
              prepared.surface == snapshotState.selectedAssistantSurface
        else {
            CompanionDiagnostics.record("session-mini:projection-stale-drop seq=\(latestSeq)")
            return .dropped
        }

        guard Self.shouldAcceptContent(
            candidateSeq: latestSeq,
            realtimeLatestSeq: currentRealtimeLatestSeq(),
            hasSnapshot: snapshotState.hasSnapshot,
            allSessionsEmpty: snapshotState.allSessions.isEmpty
        ) else {
            CompanionDiagnostics.record("session-mini:sync-snapshot-skip reason=\(reason) seq=\(latestSeq)")
            return .rejected(Rejection(
                reason: reason,
                latestSeq: latestSeq,
                endpointURL: endpointURL,
                sourceSnapshot: prepared.sourceSnapshot,
                provesLiveness: true
            ))
        }

        let applyResult = snapshotState.applyPreparedSnapshotResult(prepared)
        CompanionDiagnostics.record("session-mini:sync-applied reason=\(reason) seq=\(latestSeq)")
        return .applied(Outcome(
            reason: reason,
            latestSeq: latestSeq,
            endpointURL: endpointURL,
            strategy: .direct,
            applyResult: applyResult,
            provesLiveness: true,
            sourceSnapshot: prepared.sourceSnapshot
        ))
    }

    // MARK: - Cache / recovery / post-command replay

    /// Local-truth replay path shared by cache restore (launch/unlock/open),
    /// background recovery, and post-command replay. No async build — the
    /// snapshot is already fully materialized — but the same seq-acceptance
    /// gate applies, with a broader-merge fallback strategy that preserves
    /// whatever the visible surface already shows while folding in sessions
    /// the cache knows about on OTHER surfaces.
    @discardableResult
    func ingestReplaySnapshot(
        _ snapshot: MobileSnapshot,
        reason: String,
        latestSeq: Int64,
        bypassesSeqGating: Bool = false
    ) -> IngestResult {
        let realtimeLatestSeq = currentRealtimeLatestSeq()
        let accepted = bypassesSeqGating || Self.shouldAcceptContent(
            candidateSeq: latestSeq,
            realtimeLatestSeq: realtimeLatestSeq,
            hasSnapshot: snapshotState.hasSnapshot,
            allSessionsEmpty: snapshotState.allSessions.isEmpty
        )

        guard accepted else {
            guard let applyResult = snapshotState.applyBroaderSnapshotPreservingCurrentSessions(
                snapshot,
                preferredSurface: snapshotState.selectedAssistantSurface
            ) else {
                CompanionDiagnostics.record(
                    "session-mini:cache-skip reason=\(reason) latestSeq=\(latestSeq) realtimeSeq=\(realtimeLatestSeq)"
                )
                return .rejected(Rejection(
                    reason: reason,
                    latestSeq: latestSeq,
                    endpointURL: nil,
                    sourceSnapshot: snapshot,
                    provesLiveness: false
                ))
            }
            return .applied(Outcome(
                reason: reason,
                latestSeq: latestSeq,
                endpointURL: nil,
                strategy: .broaderMerge,
                applyResult: applyResult,
                provesLiveness: false,
                sourceSnapshot: snapshot
            ))
        }

        let applyResult = snapshotState.applySnapshotResult(snapshot)
        return .applied(Outcome(
            reason: reason,
            latestSeq: latestSeq,
            endpointURL: nil,
            strategy: .direct,
            applyResult: applyResult,
            provesLiveness: false,
            sourceSnapshot: snapshot
        ))
    }

    /// THE single seq-acceptance comparison site. Every apply path funnels
    /// its candidate seq through this to decide whether the candidate's
    /// content may overwrite what's currently visible. Preserves, in one
    /// place, the exact bypasses the three predecessor guards granted
    /// (nothing visible yet, no realtime evidence yet, the empty-session
    /// recovery case) plus the `>=` — not `>` — comparison a same-frame
    /// liveness pass requires.
    private static func shouldAcceptContent(
        candidateSeq: Int64,
        realtimeLatestSeq: Int64,
        hasSnapshot: Bool,
        allSessionsEmpty: Bool
    ) -> Bool {
        if realtimeLatestSeq > 0, candidateSeq < realtimeLatestSeq {
            return false
        }
        if !hasSnapshot {
            return true
        }
        if realtimeLatestSeq <= 0 {
            return true
        }
        if allSessionsEmpty {
            return true
        }
        // The liveness pass of the same stream frame can already have
        // advanced realtimeLatestSeq to this frame's seq before its
        // snapshot arrives, so equality must count as fresh — requiring
        // strictly-greater here rejects every streamed update once the
        // tracker catches up and freezes the visible session list.
        return candidateSeq >= realtimeLatestSeq
    }

    // MARK: - Testing seams

    /// Mirrors the old reserve-then-complete two-step so out-of-order /
    /// surface-race coverage can still drive the pipeline deterministically
    /// without a real stream.
    func reserveForTesting(latestSeq: Int64) -> UInt64? {
        guard latestSeq >= reservedSeq else {
            return nil
        }
        generation &+= 1
        reservedSeq = latestSeq
        return generation
    }

    func completeForTesting(
        _ prepared: CompanionPreparedSnapshotProjection,
        reason: String,
        latestSeq: Int64,
        endpointURL: URL?,
        streamGeneration: UInt64,
        reservedGeneration: UInt64,
        builtSurface: CompanionAssistantSurface
    ) -> IngestResult {
        completeLiveStreamIngest(
            prepared,
            reason: reason,
            latestSeq: latestSeq,
            endpointURL: endpointURL,
            streamGeneration: streamGeneration,
            reservedGeneration: reservedGeneration,
            builtSurface: builtSurface
        )
    }

    func waitForPendingBuildForTesting() async {
        await buildTask?.value
    }
}
