import Foundation
import SwiftUI

enum MessageRenderBlock: Identifiable, Sendable {
    case markdown(id: Int, text: AttributedString)
    case code(id: Int, language: String?, text: String, renderedDiff: RenderedDiffBlock?)

    var id: Int {
        switch self {
        case let .markdown(id, _), let .code(id, _, _, _):
            return id
        }
    }
}

enum MessageRenderBlockParser {
    private static let initialBlockID = 0

    static func parse(_ markdown: String) -> [MessageRenderBlock] {
        var blocks: [MessageRenderBlock] = []
        var markdownLines: [String] = []
        var codeLines: [String] = []
        var codeLanguage: String?
        var isInCodeBlock = false
        var nextBlockID = initialBlockID

        for line in markdown.components(separatedBy: .newlines) {
            if let fenceLanguage = fenceLanguage(from: line) {
                if isInCodeBlock {
                    appendCodeBlock(
                        &blocks,
                        id: &nextBlockID,
                        language: codeLanguage,
                        lines: codeLines
                    )
                    codeLines = []
                    codeLanguage = nil
                    isInCodeBlock = false
                } else {
                    appendMarkdownBlock(&blocks, id: &nextBlockID, lines: markdownLines)
                    markdownLines = []
                    codeLanguage = fenceLanguage
                    isInCodeBlock = true
                }
                continue
            }

            if isInCodeBlock {
                codeLines.append(line)
            } else {
                markdownLines.append(line)
            }
        }

        if isInCodeBlock {
            appendCodeBlock(&blocks, id: &nextBlockID, language: codeLanguage, lines: codeLines)
        } else {
            appendMarkdownBlock(&blocks, id: &nextBlockID, lines: markdownLines)
        }

        return blocks
    }

    private static func fenceLanguage(from line: String) -> String? {
        let trimmedLine = line.trimmingCharacters(in: .whitespaces)
        guard trimmedLine.hasPrefix("```") else {
            return nil
        }

        let language = trimmedLine.dropFirst(3).trimmingCharacters(in: .whitespaces)
        return language.isEmpty ? "" : language
    }

    private static func appendMarkdownBlock(
        _ blocks: inout [MessageRenderBlock],
        id: inout Int,
        lines: [String]
    ) {
        let text = lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else {
            return
        }

        blocks.append(.markdown(id: id, text: renderedText(from: text)))
        id += 1
    }

    private static func appendCodeBlock(
        _ blocks: inout [MessageRenderBlock],
        id: inout Int,
        language: String?,
        lines: [String]
    ) {
        let text = lines.joined(separator: "\n")
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return
        }

        blocks.append(
            .code(
                id: id,
                language: language,
                text: text,
                renderedDiff: DiffBlockParser.parse(language: language, diff: text)
            )
        )
        id += 1
    }

    private static func renderedText(from markdown: String) -> AttributedString {
        (try? AttributedString(markdown: markdown)) ?? AttributedString(markdown)
    }
}

private enum MessageRenderCacheLimit {
    static let maximumEntries = 48
}

private final class MessageRenderBlockCache: @unchecked Sendable {
    static let shared = MessageRenderBlockCache(maximumEntries: MessageRenderCacheLimit.maximumEntries)

    private let lock = NSLock()
    private let maximumEntries: Int
    private var blocksByMarkdown: [String: [MessageRenderBlock]] = [:]
    private var insertionOrder: [String] = []

    init(maximumEntries: Int) {
        self.maximumEntries = maximumEntries
    }

    func blocks(for markdown: String) -> [MessageRenderBlock] {
        lock.lock()
        if let cachedBlocks = blocksByMarkdown[markdown] {
            lock.unlock()
            return cachedBlocks
        }
        lock.unlock()

        let parsedBlocks = MessageRenderBlockParser.parse(markdown)

        lock.lock()
        defer {
            lock.unlock()
        }

        if let cachedBlocks = blocksByMarkdown[markdown] {
            return cachedBlocks
        }

        blocksByMarkdown[markdown] = parsedBlocks
        insertionOrder.append(markdown)
        pruneIfNeeded()
        return parsedBlocks
    }

    func cachedBlocks(for markdown: String) -> [MessageRenderBlock]? {
        lock.lock()
        defer {
            lock.unlock()
        }
        return blocksByMarkdown[markdown]
    }

    private func pruneIfNeeded() {
        while insertionOrder.count > maximumEntries {
            let evictedMarkdown = insertionOrder.removeFirst()
            blocksByMarkdown[evictedMarkdown] = nil
        }
    }
}

struct MarkdownMessageView: View {
    let markdown: String
    @State private var blocks: [MessageRenderBlock]?

    init(markdown: String) {
        self.markdown = markdown
    }

    var body: some View {
        Group {
            if let blocks {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(blocks) { block in
                        switch block {
                        case let .markdown(_, text):
                            MarkdownTextBlock(text: text)
                        case let .code(_, language, text, renderedDiff):
                            CodeBlockView(language: language, code: text, renderedDiff: renderedDiff)
                        }
                    }
                }
            } else {
                Text(Self.placeholderText(from: markdown))
                    .font(.body)
                    .foregroundStyle(.secondary)
                    .lineLimit(8)
                    .textSelection(.enabled)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .task(id: markdown) {
            await loadBlocks(for: markdown)
        }
    }

    @MainActor
    private func loadBlocks(for markdown: String) async {
        if let cachedBlocks = MessageRenderBlockCache.shared.cachedBlocks(for: markdown) {
            blocks = cachedBlocks
            return
        }

        let parsedBlocks = await Task.detached(priority: .utility) {
            MessageRenderBlockCache.shared.blocks(for: markdown)
        }.value
        guard !Task.isCancelled, self.markdown == markdown else {
            return
        }
        blocks = parsedBlocks
    }

    private static func placeholderText(from markdown: String) -> String {
        let trimmed = markdown.trimmingCharacters(in: .whitespacesAndNewlines)
        let maximumVisibleCharacterCount = 600
        guard trimmed.count > maximumVisibleCharacterCount else {
            return trimmed
        }
        return String(trimmed.prefix(maximumVisibleCharacterCount))
    }
}

private struct MarkdownTextBlock: View {
    let text: AttributedString

    var body: some View {
        Text(text)
            .font(.body)
            .foregroundStyle(.primary)
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct CodeBlockView: View {
    let language: String?
    let code: String
    let renderedDiff: RenderedDiffBlock?

    var body: some View {
        if let renderedDiff {
            DiffBlockView(renderedDiff: renderedDiff)
        } else {
            plainCodeBlock
        }
    }

    private var plainCodeBlock: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let label = languageLabel {
                Text(label)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
            }

            ScrollView(.horizontal, showsIndicators: true) {
                Text(code)
                    .font(.system(.footnote, design: .monospaced))
                    .foregroundStyle(.primary)
                    .textSelection(.enabled)
                    .padding(12)
            }
            .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 8))
        }
    }

    private var languageLabel: String? {
        guard let language,
              !language.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else {
            return nil
        }

        return language.uppercased()
    }
}
