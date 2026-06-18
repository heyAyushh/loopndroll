import SwiftUI

enum MessageRenderBlock: Identifiable {
    case markdown(id: Int, text: AttributedString)
    case code(id: Int, language: String?, text: String)

    var id: Int {
        switch self {
        case let .markdown(id, _), let .code(id, _, _):
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

        blocks.append(.code(id: id, language: language, text: text))
        id += 1
    }

    private static func renderedText(from markdown: String) -> AttributedString {
        (try? AttributedString(markdown: markdown)) ?? AttributedString(markdown)
    }
}

struct MarkdownMessageView: View {
    let markdown: String
    private let blocks: [MessageRenderBlock]

    init(markdown: String) {
        self.markdown = markdown
        blocks = MessageRenderBlockParser.parse(markdown)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(blocks) { block in
                switch block {
                case let .markdown(_, text):
                    MarkdownTextBlock(text: text)
                case let .code(_, language, text):
                    CodeBlockView(language: language, code: text)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
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

    private var isDiff: Bool {
        DiffBlockView.isDiff(language: language, code: code)
    }

    var body: some View {
        if isDiff {
            DiffBlockView(language: language, diff: code)
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
