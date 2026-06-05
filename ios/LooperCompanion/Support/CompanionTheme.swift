import OSLog
import SwiftUI

enum CompanionAppearanceMode: String, CaseIterable, Identifiable {
    case system
    case light
    case dark

    var id: String {
        rawValue
    }

    var label: String {
        switch self {
        case .system:
            return "System"
        case .light:
            return "Light"
        case .dark:
            return "Dark"
        }
    }

    var colorScheme: ColorScheme? {
        switch self {
        case .system:
            return nil
        case .light:
            return .light
        case .dark:
            return .dark
        }
    }
}

enum CompanionMetrics {
    static let compactCornerRadius: CGFloat = 8
    static let mediumCornerRadius: CGFloat = 12
    static let cardCornerRadius: CGFloat = 10
    static let editorMinHeight: CGFloat = 144
    static let rowSpacing: CGFloat = 12
    static let cardPadding: CGFloat = 18
    static let screenPadding: CGFloat = 20
    static let sectionSpacing: CGFloat = 28
    static let autoRefreshInterval: Duration = .seconds(20)
}

enum CompanionDiagnostics {
    private static let fallbackSubsystem = "dev.looper.app.ios"
    private static let diagnosticsFilename = "looper-diagnostics.log"
    private static let subsystem = Bundle.main.bundleIdentifier ?? fallbackSubsystem

    static let cache = Logger(subsystem: subsystem, category: "SnapshotCache")
    static let configuration = Logger(subsystem: subsystem, category: "Configuration")
    static let lifecycle = Logger(subsystem: subsystem, category: "Lifecycle")
    static let networking = Logger(subsystem: subsystem, category: "Networking")

    static func record(_ message: String) {
        #if DEBUG
        let timestamp = ISO8601DateFormatter().string(from: Date())
        let line = "\(timestamp) \(message)\n"
        guard let data = line.data(using: .utf8) else {
            return
        }

        let url = diagnosticsURL()
        if FileManager.default.fileExists(atPath: url.path),
           let fileHandle = try? FileHandle(forWritingTo: url) {
            defer {
                try? fileHandle.close()
            }
            _ = try? fileHandle.seekToEnd()
            try? fileHandle.write(contentsOf: data)
            return
        }

        try? data.write(to: url, options: .atomic)
        #endif
    }

    private static func diagnosticsURL() -> URL {
        let cacheDirectory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
        return cacheDirectory.appendingPathComponent(diagnosticsFilename)
    }
}

extension View {
    func companionListSurface() -> some View {
        self
    }

    func companionCardRowSurface() -> some View {
        pinballSurface(
            cornerRadius: CompanionMetrics.cardCornerRadius,
            material: .glass
        )
    }
}

enum CompanionTint {
    static func tint(for state: ConnectivityState) -> Color {
        switch state {
        case .connected:
            return .green
        case .connecting:
            return .orange
        case .offline, .unauthorized, .locked, .unpaired:
            return .red
        }
    }

    static func tint(for status: SessionStatus) -> Color {
        switch status {
        case .active:
            return .green
        case .waiting:
            return .orange
        case .stopped:
            return .blue
        case .archived:
            return .indigo
        }
    }
}
