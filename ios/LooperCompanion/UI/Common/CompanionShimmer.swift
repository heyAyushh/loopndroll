import SwiftUI

/// Tuning for the shimmer sweep and its Reduce Motion fallback pulse. Kept
/// calm on purpose: a skeleton is meant to read as "working", not to draw
/// the eye the way a flashy loading effect would.
private enum CompanionShimmerTuning {
    static let sweepPeriod: TimeInterval = 1.4
    /// Width of the moving highlight band, as a fraction of the content it sweeps across.
    static let bandWidthRatio: CGFloat = 0.35
    static let highlightOpacity: Double = 0.55

    static let pulseDuration: TimeInterval = 1.4
    static let pulseMinOpacity: Double = 0.45
    static let pulseMaxOpacity: Double = 1.0
}

/// Moves a soft highlight band left-to-right across `content`, masked to
/// `content`'s own shape so the sweep only lights up pixels that are
/// actually there (a rounded-rect skeleton line, a glyph run, etc.).
private struct CompanionShimmerSweep: ViewModifier {
    @State private var phase: CGFloat = 0

    func body(content: Content) -> some View {
        content
            .overlay {
                GeometryReader { proxy in
                    shimmerBand(in: proxy.size)
                }
                .mask(content)
            }
            .onAppear {
                withAnimation(
                    .linear(duration: CompanionShimmerTuning.sweepPeriod).repeatForever(autoreverses: false)
                ) {
                    phase = 1
                }
            }
    }

    private func shimmerBand(in size: CGSize) -> some View {
        let bandWidth = size.width * CompanionShimmerTuning.bandWidthRatio
        // Travels from fully off the left edge to fully off the right edge so
        // the highlight enters and exits cleanly instead of popping in place.
        let travel = size.width + bandWidth
        return LinearGradient(
            colors: [.clear, .white.opacity(CompanionShimmerTuning.highlightOpacity), .clear],
            startPoint: .leading,
            endPoint: .trailing
        )
        .frame(width: bandWidth)
        .offset(x: -bandWidth + phase * travel)
    }
}

/// Reduce Motion fallback: a slow, gentle opacity breathe instead of a
/// sweeping gradient, so the "still loading" signal survives without motion.
private struct CompanionShimmerPulse: ViewModifier {
    @State private var isDim = false

    func body(content: Content) -> some View {
        content
            .opacity(isDim ? CompanionShimmerTuning.pulseMinOpacity : CompanionShimmerTuning.pulseMaxOpacity)
            .onAppear {
                withAnimation(
                    .easeInOut(duration: CompanionShimmerTuning.pulseDuration).repeatForever(autoreverses: true)
                ) {
                    isDim = true
                }
            }
    }
}

private struct CompanionShimmerModifier: ViewModifier {
    let isActive: Bool

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        if !isActive {
            content
        } else if reduceMotion {
            content.modifier(CompanionShimmerPulse())
        } else {
            content.modifier(CompanionShimmerSweep())
        }
    }
}

extension View {
    /// Applies the standard Looper skeleton shimmer to `self` while `active`:
    /// a calm gradient sweep, or a gentle opacity pulse under Reduce Motion.
    /// Used anywhere the app is waiting on data, so the UI reads as "working"
    /// rather than "stuck" the moment a spinner would otherwise sit still.
    func companionShimmer(active: Bool) -> some View {
        modifier(CompanionShimmerModifier(isActive: active))
    }
}

/// A single shimmering skeleton bar. The reusable building block behind
/// `CompanionSkeletonSessionRow` and any other "this text isn't in yet"
/// placeholder (e.g. the session-detail thinking indicator).
struct CompanionShimmerLine: View {
    let width: CGFloat
    var height: CGFloat = CompanionShimmerLineMetrics.defaultHeight

    var body: some View {
        RoundedRectangle(cornerRadius: height / 2, style: .continuous)
            .fill(Color.secondary.opacity(CompanionShimmerLineMetrics.placeholderOpacity))
            .frame(width: width, height: height)
            .companionShimmer(active: true)
    }
}

private enum CompanionShimmerLineMetrics {
    static let defaultHeight: CGFloat = 12
    static let placeholderOpacity: Double = 0.14
}

private enum CompanionSkeletonSessionRowMetrics {
    static let avatarSize: CGFloat = 28
    static let avatarSpacing: CGFloat = 4
    static let leadingSpacing: CGFloat = 12
    static let sectionSpacing: CGFloat = 8
    static let lineSpacing: CGFloat = 6
    static let metaRowSpacing: CGFloat = 12
    static let verticalPadding: CGFloat = 4
    static let trailingSpacerMinLength: CGFloat = 12
    static let placeholderOpacity: Double = 0.14

    static let refLineWidth: CGFloat = 24
    static let titleLineWidth: CGFloat = 150
    static let subtitleLineWidth: CGFloat = 110
    static let statusPillWidth: CGFloat = 56
    static let statusPillHeight: CGFloat = 20
    static let previewLineWidth: CGFloat = 220
    static let previewLineHeight: CGFloat = 13
    static let metaChipWidth: CGFloat = 64
    static let metaLineHeight: CGFloat = 11
}

/// Placeholder card matching `SessionRow`'s rough geometry — a leading
/// avatar, title/subtitle lines, a status pill, a preview line, and a meta
/// row — so the first-launch loading state looks like the list that's about
/// to arrive instead of a blank screen with a spinner.
struct CompanionSkeletonSessionRow: View {
    var body: some View {
        VStack(alignment: .leading, spacing: CompanionSkeletonSessionRowMetrics.sectionSpacing) {
            HStack(alignment: .top, spacing: CompanionSkeletonSessionRowMetrics.leadingSpacing) {
                VStack(spacing: CompanionSkeletonSessionRowMetrics.avatarSpacing) {
                    Circle()
                        .fill(placeholderFill)
                        .frame(
                            width: CompanionSkeletonSessionRowMetrics.avatarSize,
                            height: CompanionSkeletonSessionRowMetrics.avatarSize
                        )
                        .companionShimmer(active: true)

                    CompanionShimmerLine(
                        width: CompanionSkeletonSessionRowMetrics.refLineWidth,
                        height: CompanionSkeletonSessionRowMetrics.metaLineHeight
                    )
                }

                VStack(alignment: .leading, spacing: CompanionSkeletonSessionRowMetrics.lineSpacing) {
                    CompanionShimmerLine(width: CompanionSkeletonSessionRowMetrics.titleLineWidth)
                    CompanionShimmerLine(
                        width: CompanionSkeletonSessionRowMetrics.subtitleLineWidth,
                        height: CompanionSkeletonSessionRowMetrics.metaLineHeight
                    )
                }

                Spacer(minLength: CompanionSkeletonSessionRowMetrics.trailingSpacerMinLength)

                CompanionShimmerLine(
                    width: CompanionSkeletonSessionRowMetrics.statusPillWidth,
                    height: CompanionSkeletonSessionRowMetrics.statusPillHeight
                )
            }

            CompanionShimmerLine(
                width: CompanionSkeletonSessionRowMetrics.previewLineWidth,
                height: CompanionSkeletonSessionRowMetrics.previewLineHeight
            )

            HStack(spacing: CompanionSkeletonSessionRowMetrics.metaRowSpacing) {
                CompanionShimmerLine(
                    width: CompanionSkeletonSessionRowMetrics.metaChipWidth,
                    height: CompanionSkeletonSessionRowMetrics.metaLineHeight
                )
                CompanionShimmerLine(
                    width: CompanionSkeletonSessionRowMetrics.metaChipWidth,
                    height: CompanionSkeletonSessionRowMetrics.metaLineHeight
                )
            }
        }
        .padding(.vertical, CompanionSkeletonSessionRowMetrics.verticalPadding)
        .accessibilityHidden(true)
    }

    private var placeholderFill: Color {
        Color.secondary.opacity(CompanionSkeletonSessionRowMetrics.placeholderOpacity)
    }
}

#Preview("Skeleton session row") {
    VStack(spacing: CompanionMetrics.rowSpacing) {
        ForEach(0..<4, id: \.self) { _ in
            CompanionSkeletonSessionRow()
                .companionCardRowSurface()
        }
    }
    .padding()
}
