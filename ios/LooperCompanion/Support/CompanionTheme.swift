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
    static let eventStreamReconnectDelay: Duration = .seconds(3)
    /// Bound on pairing/bootstrap waiting for the realtime stream to prove
    /// liveness before falling back to the connection-failure copy.
    static let bootstrapConnectTimeout: Duration = .seconds(6)
}

enum CompanionDiagnostics {
    private static let fallbackSubsystem = "dev.looper.app.ios"
    private static let diagnosticsFilename = "looper-diagnostics.log"
    private static let subsystem = Bundle.main.bundleIdentifier ?? fallbackSubsystem
    private static let millisecondsPerSecond: TimeInterval = 1_000

    static let cache = Logger(subsystem: subsystem, category: "SnapshotCache")
    static let configuration = Logger(subsystem: subsystem, category: "Configuration")
    static let lifecycle = Logger(subsystem: subsystem, category: "Lifecycle")
    static let networking = Logger(subsystem: subsystem, category: "Networking")
    static let assistantSurface = Logger(subsystem: subsystem, category: "AssistantSurface")

    static func record(_ message: String) {
        #if DEBUG
        Task.detached(priority: .utility) {
            await DiagnosticsLogWriter.shared.record(message)
        }
        #endif
    }

    /// Wall-clock span in whole milliseconds since `startedAt`. Used to
    /// instrument suspected main-thread blockers (e.g. the route-switch
    /// chain) with cheap, DEBUG-only spans consumed via `record(_:)`.
    static func elapsedMilliseconds(since startedAt: Date) -> Int {
        Int((Date().timeIntervalSince(startedAt) * millisecondsPerSecond).rounded())
    }

    private static func diagnosticsURL() -> URL {
        let cacheDirectory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
        return cacheDirectory.appendingPathComponent(diagnosticsFilename)
    }

    #if DEBUG
    private actor DiagnosticsLogWriter {
        static let shared = DiagnosticsLogWriter()

        private let timestampFormatter = ISO8601DateFormatter()
        private var fileHandle: FileHandle?

        deinit {
            try? fileHandle?.close()
        }

        func record(_ message: String) {
            let timestamp = timestampFormatter.string(from: Date())
            let line = "\(timestamp) \(message)\n"
            guard let data = line.data(using: .utf8) else {
                return
            }

            do {
                let fileHandle = try writableFileHandle()
                try fileHandle.write(contentsOf: data)
            } catch {
                CompanionDiagnostics.networking.error(
                    "Failed to record diagnostics error=\(error.localizedDescription, privacy: .public)"
                )
            }
        }

        private func writableFileHandle() throws -> FileHandle {
            if let fileHandle {
                return fileHandle
            }

            let url = CompanionDiagnostics.diagnosticsURL()
            if !FileManager.default.fileExists(atPath: url.path) {
                _ = FileManager.default.createFile(atPath: url.path, contents: nil)
            }

            let fileHandle = try FileHandle(forWritingTo: url)
            try fileHandle.seekToEnd()
            self.fileHandle = fileHandle
            return fileHandle
        }
    }
    #endif
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

    static func tint(for status: CompanionConnectionPresentationStatus) -> Color {
        switch status {
        case .live:
            return .green
        case .reconnecting, .local:
            return .secondary
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
