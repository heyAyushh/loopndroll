import SwiftUI

/// Leading badge for session rows: native SF Symbol in a fixed metric (HIG-aligned list accessory).
struct AssistantClientGlyph: View {
    let client: AssistantClient

    private static let size: CGFloat = 28

    var body: some View {
        Image(systemName: client.systemImageName)
            .font(.body.weight(.semibold))
            .foregroundStyle(.secondary)
            .frame(width: Self.size, height: Self.size)
            .accessibilityLabel(client.displayTitle)
    }
}
