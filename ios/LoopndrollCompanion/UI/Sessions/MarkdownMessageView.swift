import SwiftUI

enum MessageRenderBlock: Identifiable {
    case markdown(id: UUID, text: String)
    case code(id: UUID, language: String?, text: String)

    var id: UUID {
        switch self {
        case let .markdown(id, _), let .code(id, _, _):
            return id
        }
    }
}

enum MessageRenderBlockParser {
    static func parse(_ markdown: String) -> [MessageRenderBlock] {
        var blocks: [MessageRenderBlock] = []
        var markdownLines: [String] = []
        var codeLines: [String] = []
        var codeLanguage: String?
        var isInCodeBlock = false

        for line in markdown.components(separatedBy: .newlines) {
            if let fenceLanguage = fenceLanguage(from: line) {
                if isInCodeBlock {
                    appendCodeBlock(&blocks, language: codeLanguage, lines: codeLines)
                    codeLines = []
                    codeLanguage = nil
                    isInCodeBlock = false
                } else {
                    appendMarkdownBlock(&blocks, lines: markdownLines)
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
            appendCodeBlock(&blocks, language: codeLanguage, lines: codeLines)
        } else {
            appendMarkdownBlock(&blocks, lines: markdownLines)
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

    private static func appendMarkdownBlock(_ blocks: inout [MessageRenderBlock], lines: [String]) {
        let text = lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else {
            return
        }

        blocks.append(.markdown(id: UUID(), text: text))
    }

    private static func appendCodeBlock(
        _ blocks: inout [MessageRenderBlock],
        language: String?,
        lines: [String]
    ) {
        let text = lines.joined(separator: "\n")
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return
        }

        blocks.append(.code(id: UUID(), language: language, text: text))
    }
}

struct MarkdownMessageView: View {
    let markdown: String

    private var blocks: [MessageRenderBlock] {
        MessageRenderBlockParser.parse(markdown)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(blocks) { block in
                switch block {
                case let .markdown(_, text):
                    MarkdownTextBlock(markdown: text)
                case let .code(_, language, text):
                    CodeBlockView(language: language, code: text)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct MarkdownTextBlock: View {
    let markdown: String

    var body: some View {
        Text(renderedText)
            .font(.body)
            .foregroundStyle(.primary)
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var renderedText: AttributedString {
        (try? AttributedString(markdown: markdown)) ?? AttributedString(markdown)
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
