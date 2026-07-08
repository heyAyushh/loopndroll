import Foundation
import LooperClientCore
import Testing
@testable import Looper

@MainActor
@Suite("Streaming reply buffer")
struct StreamingReplyBufferTests {
    private static func chunk(
        seq: Int64,
        messageID: String = "message-1",
        content: String,
        isFinal: Bool = false
    ) -> ClientTextChunk {
        ClientTextChunk(
            seq: seq,
            threadId: "thread-1",
            messageId: messageID,
            content: content,
            isFinal: isFinal,
            serverTime: ""
        )
    }

    @Test("Chunks concatenate in arrival order")
    func chunksConcatenateInOrder() {
        let buffer = StreamingReplyBuffer()
        buffer.append(Self.chunk(seq: 0, content: "Hello"))
        buffer.append(Self.chunk(seq: 1, content: ", "))
        buffer.append(Self.chunk(seq: 2, content: "world"))

        #expect(buffer.text == "Hello, world")
    }

    @Test("Duplicate seq is ignored")
    func duplicateSeqIgnored() {
        let buffer = StreamingReplyBuffer()
        buffer.append(Self.chunk(seq: 0, content: "Hello"))
        buffer.append(Self.chunk(seq: 0, content: "Hello"))

        #expect(buffer.text == "Hello")
    }

    @Test("Out-of-order (lower) seq is dropped")
    func outOfOrderSeqDropped() {
        let buffer = StreamingReplyBuffer()
        buffer.append(Self.chunk(seq: 5, content: "later"))
        buffer.append(Self.chunk(seq: 2, content: "earlier"))

        #expect(buffer.text == "later")
    }

    @Test("Chunks out of order arrival still applied by seq gate, not arrival order")
    func replayedChunkAfterGapIsIgnored() {
        let buffer = StreamingReplyBuffer()
        buffer.append(Self.chunk(seq: 0, content: "A"))
        buffer.append(Self.chunk(seq: 1, content: "B"))
        // A reconnect replay resending seq 0 must not double-apply.
        buffer.append(Self.chunk(seq: 0, content: "A"))

        #expect(buffer.text == "AB")
    }

    @Test("New messageId resets the buffer")
    func newMessageIDResetsBuffer() {
        let buffer = StreamingReplyBuffer()
        buffer.append(Self.chunk(seq: 0, messageID: "message-1", content: "First reply"))
        #expect(buffer.text == "First reply")

        buffer.append(Self.chunk(seq: 0, messageID: "message-2", content: "Second reply"))
        #expect(buffer.text == "Second reply")
        #expect(buffer.messageID == "message-2")
    }

    @Test("Final chunk marks the buffer no longer streaming")
    func finalChunkStopsStreaming() {
        let buffer = StreamingReplyBuffer()
        buffer.append(Self.chunk(seq: 0, content: "Done", isFinal: true))

        #expect(buffer.text == "Done")
        #expect(buffer.isStreaming == false)
    }

    @Test("Reset clears text, messageId, and dedupe state")
    func resetClearsState() {
        let buffer = StreamingReplyBuffer()
        buffer.append(Self.chunk(seq: 0, content: "Hello"))
        buffer.reset()

        #expect(buffer.text.isEmpty)
        #expect(buffer.messageID == nil)
        #expect(buffer.isStreaming == false)

        // After reset, seq 0 for the same messageId is accepted again (fresh stream).
        buffer.append(Self.chunk(seq: 0, content: "Restarted"))
        #expect(buffer.text == "Restarted")
    }

    @Test("isStreaming is true immediately after a non-final chunk")
    func isStreamingTrueAfterChunk() {
        let buffer = StreamingReplyBuffer()
        buffer.append(Self.chunk(seq: 0, content: "still going"))

        #expect(buffer.isStreaming)
    }

    @Test("lastAppendedRange reflects only the most recent append")
    func lastAppendedRangeTracksLatestChunk() throws {
        let buffer = StreamingReplyBuffer()
        buffer.append(Self.chunk(seq: 0, content: "Hello"))
        buffer.append(Self.chunk(seq: 1, content: " world"))

        let range = try #require(buffer.lastAppendedRange)
        #expect(String(buffer.text[range]) == " world")
    }
}
