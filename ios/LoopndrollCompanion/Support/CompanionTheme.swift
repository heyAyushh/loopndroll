import SwiftUI

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
        case .offline, .unauthorized, .unpaired:
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
