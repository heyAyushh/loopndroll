import Foundation
import LooperClientCore

private enum StreamingReplyBufferTuning {
    /// A buffer is considered "streaming" while its last chunk landed within
    /// this window; older buffers are treated as settled so the UI drops the
    /// typewriter affordance and falls back to the projected snapshot text.
    static let livenessWindow: TimeInterval = 2.0
}

/// Per-session, append-only text buffer fed directly by realtime `TextChunk`
/// frames — independent of the full mobile-snapshot projection pipeline, so
/// the visible reply can update the instant a chunk lands instead of waiting
/// on `CompanionPreparedSnapshotProjection.build(...)`.
///
/// One instance per session (keyed by thread/session id) is owned by
/// `CompanionAppModel`; `SessionDetailScreen` and `SessionRow` read it
/// directly so re-renders stay scoped to the view observing this buffer.
@MainActor
@Observable
final class StreamingReplyBuffer {
    private(set) var text = ""
    private(set) var messageID: String?
    /// Range of `text` appended by the most recently applied chunk, so a view
    /// can animate only the newly revealed tail instead of the whole string.
    private(set) var lastAppendedRange: Range<String.Index>?

    @ObservationIgnored private var isFinalMessage = false
    @ObservationIgnored private var highestAppliedSeqByMessageID: [String: Int64] = [:]
    @ObservationIgnored private var lastChunkAt: Date?

    /// True while chunks are actively arriving for the current message. Once
    /// a chunk is marked final, or chunks stop arriving for a while, this
    /// flips false and callers should fall back to the settled snapshot text.
    var isStreaming: Bool {
        guard !isFinalMessage, let lastChunkAt else {
            return false
        }
        return Date().timeIntervalSince(lastChunkAt) < StreamingReplyBufferTuning.livenessWindow
    }

    /// Appends a chunk if it advances the buffer: chunks are ordered and
    /// deduped by `seq` within a `messageId`, so replays/duplicates from
    /// reconnects are dropped and a new `messageId` starts a fresh reply.
    func append(_ chunk: ClientTextChunk, now: Date = Date()) {
        if messageID != chunk.messageId {
            reset()
            messageID = chunk.messageId
        }

        let highestAppliedSeq = highestAppliedSeqByMessageID[chunk.messageId] ?? -1
        guard chunk.seq > highestAppliedSeq else {
            return
        }
        highestAppliedSeqByMessageID[chunk.messageId] = chunk.seq

        guard !chunk.content.isEmpty else {
            lastAppendedRange = nil
            lastChunkAt = now
            isFinalMessage = isFinalMessage || chunk.isFinal
            return
        }

        let insertionPoint = text.endIndex
        text.append(chunk.content)
        lastAppendedRange = insertionPoint..<text.endIndex
        lastChunkAt = now
        isFinalMessage = isFinalMessage || chunk.isFinal
    }

    func reset() {
        text = ""
        messageID = nil
        lastAppendedRange = nil
        isFinalMessage = false
        highestAppliedSeqByMessageID.removeAll()
        lastChunkAt = nil
    }
}
