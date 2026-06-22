import SwiftUI

enum DiffLineKind {
    case file
    case hunk
    case addition
    case removal
    case context

    var gutter: String {
        switch self {
        case .addition:
            return "+"
        case .removal:
            return "-"
        default:
            return " "
        }
    }

    var foregroundStyle: Color {
        switch self {
        case .addition:
            return .green
        case .removal:
            return .red
        case .file, .hunk:
            return .secondary
        case .context:
            return .primary
        }
    }

    var backgroundStyle: Color {
        switch self {
        case .addition:
            return Color.green.opacity(0.12)
        case .removal:
            return Color.red.opacity(0.12)
        case .hunk:
            return Color.blue.opacity(0.10)
        case .file:
            return Color.secondary.opacity(0.10)
        case .context:
            return Color.clear
        }
    }
}

struct DiffLine: Identifiable {
    let id: Int
    let kind: DiffLineKind
    let text: String

    init(id: Int, rawLine: String) {
        self.id = id
        text = rawLine

        if rawLine.hasPrefix("diff --git") ||
            rawLine.hasPrefix("index ") ||
            rawLine.hasPrefix("--- ") ||
            rawLine.hasPrefix("+++ ")
        {
            kind = .file
        } else if rawLine.hasPrefix("@@") {
            kind = .hunk
        } else if rawLine.hasPrefix("+") {
            kind = .addition
        } else if rawLine.hasPrefix("-") {
            kind = .removal
        } else {
            kind = .context
        }
    }
}

struct RenderedDiffBlock {
    let lines: [DiffLine]
    fileprivate let changeCount: DiffChangeCount
}

enum DiffBlockParser {
    static func parse(language: String?, diff: String) -> RenderedDiffBlock? {
        guard isDiff(language: language, code: diff) else {
            return nil
        }

        let lines = diff.components(separatedBy: .newlines)
            .enumerated()
            .map { offset, rawLine in
                DiffLine(id: offset, rawLine: rawLine)
            }
        return RenderedDiffBlock(
            lines: lines,
            changeCount: countChanges(in: lines)
        )
    }

    static func isDiff(language: String?, code: String) -> Bool {
        let normalizedLanguage = language?.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
        if normalizedLanguage == "diff" || normalizedLanguage == "patch" {
            return true
        }

        return code.contains("\n@@") ||
            code.hasPrefix("@@") ||
            code.contains("\ndiff --git ") ||
            code.hasPrefix("diff --git ")
    }

    private static func countChanges(in lines: [DiffLine]) -> DiffChangeCount {
        lines.reduce(into: DiffChangeCount(additions: 0, removals: 0)) { result, line in
            switch line.kind {
            case .addition:
                result.additions += 1
            case .removal:
                result.removals += 1
            default:
                break
            }
        }
    }
}

struct DiffBlockView: View {
    private let renderedDiff: RenderedDiffBlock

    init(renderedDiff: RenderedDiffBlock) {
        self.renderedDiff = renderedDiff
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Image(systemName: "plusminus")
                Text("Diff")
                Spacer()
                Text("\(renderedDiff.changeCount.additions) additions")
                Text("\(renderedDiff.changeCount.removals) removals")
            }
            .font(.caption.weight(.semibold))
            .foregroundStyle(.secondary)

            ScrollView(.horizontal, showsIndicators: true) {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(renderedDiff.lines) { line in
                        DiffLineRow(line: line)
                    }
                }
                .padding(.vertical, 6)
            }
            .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 8))
        }
    }
}

fileprivate struct DiffChangeCount {
    var additions: Int
    var removals: Int
}

private struct DiffLineRow: View {
    let line: DiffLine

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(line.kind.gutter)
                .font(.system(.footnote, design: .monospaced).weight(.semibold))
                .foregroundStyle(line.kind.foregroundStyle)
                .frame(width: 18, alignment: .center)

            Text(line.text)
                .font(.system(.footnote, design: .monospaced))
                .foregroundStyle(line.kind.foregroundStyle)
                .textSelection(.enabled)
                .fixedSize(horizontal: true, vertical: false)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 2)
        .background(line.kind.backgroundStyle)
    }
}
