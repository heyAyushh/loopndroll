import LooperClientCore
import SwiftUI

/// Marks which "word" (a run of non-whitespace plus its trailing whitespace)
/// a glyph run in `Text.Layout` belongs to. Attaching this via
/// `Text.customAttribute(_:)` forces SwiftUI to lay the text out as one
/// `Text.Layout.Run` per word, which is what lets `StreamingWordFadeRenderer`
/// fade/blur each word in independently as it's revealed instead of
/// animating the whole block.
private struct StreamingWordIndexAttribute: TextAttribute {
    let index: Int
}

/// Renders a `StreamingReplyBuffer` with the standard LLM-app "typewriter"
/// look: settled text is static, and each newly revealed word fades in with
/// a slight blur ramp. Falls back to a plain, unanimated `Text` once the
/// buffer stops streaming or when Reduce Motion is on.
struct StreamingText: View {
    let buffer: StreamingReplyBuffer

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var revealedAt: [Int: Date] = [:]
    @State private var revealedWordCount = 0
    @State private var revealedMessageID: String?

    private enum Tuning {
        static let fadeDuration: TimeInterval = 0.35
        static let blurRadius: CGFloat = 6
        static let frameInterval: TimeInterval = 1.0 / 60.0
    }

    var body: some View {
        Group {
            if reduceMotion || !buffer.isStreaming {
                Text(buffer.text)
            } else {
                TimelineView(.periodic(from: .now, by: Tuning.frameInterval)) { context in
                    renderedText(now: context.date)
                }
            }
        }
        .onChange(of: buffer.text, initial: true) { _, newText in
            markNewlyRevealedWords(in: newText)
        }
    }

    private func renderedText(now: Date) -> some View {
        Self.text(for: buffer.text)
            .textRenderer(
                StreamingWordFadeRenderer(
                    revealedAt: revealedAt,
                    now: now,
                    fadeDuration: Tuning.fadeDuration,
                    blurRadius: Tuning.blurRadius
                )
            )
    }

    private func markNewlyRevealedWords(in text: String) {
        if buffer.messageID != revealedMessageID {
            revealedAt.removeAll()
            revealedWordCount = 0
            revealedMessageID = buffer.messageID
        }

        let tokens = Self.wordTokens(in: text)
        guard tokens.count > revealedWordCount else {
            return
        }

        let now = Date()
        for index in revealedWordCount..<tokens.count {
            revealedAt[index] = now
        }
        revealedWordCount = tokens.count
    }

    /// Splits `text` into word tokens, each a run of non-whitespace with its
    /// trailing whitespace attached (so a word and the space after it fade in
    /// together as one unit).
    private static func wordTokens(in text: String) -> [Substring] {
        var tokens: [Substring] = []
        var index = text.startIndex
        while index < text.endIndex {
            var end = index
            if text[index].isWhitespace {
                while end < text.endIndex, text[end].isWhitespace {
                    end = text.index(after: end)
                }
            } else {
                while end < text.endIndex, !text[end].isWhitespace {
                    end = text.index(after: end)
                }
                while end < text.endIndex, text[end].isWhitespace {
                    end = text.index(after: end)
                }
            }
            tokens.append(text[index..<end])
            index = end
        }
        return tokens
    }

    /// Builds `Text` as a concatenation of per-word segments, each tagged
    /// with its word index via `customAttribute`. Text concatenation
    /// preserves `TextAttribute` values per segment, which is what forces
    /// `Text.Layout` to lay each word out as its own `Run`.
    private static func text(for text: String) -> Text {
        wordTokens(in: text).enumerated().reduce(Text(verbatim: "")) { partial, item in
            let (index, token) = item
            return partial + Text(String(token))
                .customAttribute(StreamingWordIndexAttribute(index: index))
        }
    }
}

/// Fades and sharpens each word in from wall-clock reveal timestamps rather
/// than interpolated `Animatable` data — the buffer only ever grows, so each
/// word's progress is a pure function of "now minus when it first appeared."
private struct StreamingWordFadeRenderer: TextRenderer {
    let revealedAt: [Int: Date]
    let now: Date
    let fadeDuration: TimeInterval
    let blurRadius: CGFloat

    func draw(layout: Text.Layout, in context: inout GraphicsContext) {
        for line in layout {
            for run in line {
                var runContext = context
                let progress = revealProgress(for: run)
                runContext.opacity = progress
                if progress < 1 {
                    runContext.addFilter(.blur(radius: (1 - progress) * blurRadius))
                }
                runContext.draw(run)
            }
        }
    }

    private func revealProgress(for run: Text.Layout.Run) -> Double {
        guard let wordAttribute = run[StreamingWordIndexAttribute.self],
              let revealedAt = revealedAt[wordAttribute.index]
        else {
            return 1
        }

        let elapsed = now.timeIntervalSince(revealedAt)
        guard elapsed < fadeDuration else {
            return 1
        }
        return max(0, elapsed / fadeDuration)
    }
}

#Preview("Streaming reply") {
    StreamingTextPreviewHarness()
        .padding()
}

private struct StreamingTextPreviewHarness: View {
    @State private var buffer = StreamingReplyBuffer()
    @State private var feedTask: Task<Void, Never>?

    private static let sampleWords =
        "Sure, here is a quick summary of what changed in this pull request, streamed in as if it were arriving live from the assistant."
            .split(separator: " ")
            .map(String.init)

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            StreamingText(buffer: buffer)
            Button("Restart stream") {
                restart()
            }
        }
        .task {
            restart()
        }
    }

    private func restart() {
        feedTask?.cancel()
        buffer.reset()
        let messageID = UUID().uuidString
        let words = Self.sampleWords
        feedTask = Task {
            for (index, word) in words.enumerated() {
                guard !Task.isCancelled else {
                    return
                }
                let chunk = ClientTextChunk(
                    seq: Int64(index),
                    threadId: "preview-session",
                    messageId: messageID,
                    content: index == 0 ? word : " \(word)",
                    isFinal: index == words.count - 1,
                    serverTime: ""
                )
                buffer.append(chunk)
                try? await Task.sleep(for: .milliseconds(140))
            }
        }
    }
}
