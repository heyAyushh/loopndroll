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

/// Tuning for the Apple-Intelligence-style text shimmer, modeled on
/// assistant-ui's `tw-shimmer`: a diagonal highlight band that sweeps across
/// live glyphs, rests for a beat, then sweeps again — distinct from
/// `CompanionShimmerTuning`'s continuous skeleton sweep.
private enum CompanionTextShimmerTuning {
    /// Highlight band width, independent of text length so short and long
    /// previews read as the same "size" of sweep.
    static let bandWidth: CGFloat = 120
    /// Sweep speed in points per second; a sweep's duration is derived from
    /// content width (`(width + bandWidth) / speed`) so short and long text
    /// feel like the same speed rather than the same duration.
    static let speed: CGFloat = 200
    /// Rest between sweep cycles — this is a breathing highlight, not a
    /// continuous loop.
    static let pauseDuration: TimeInterval = 1.0
    /// Tilt off horizontal, approximating assistant-ui's
    /// `linear-gradient(105deg, ...)` (105° is 15° past pure horizontal).
    static let angleDegrees: Double = 15
    static let highlightOpacity: Double = 0.9
    /// Extra band height so the rotated band still fully covers multi-line
    /// text instead of clipping at its corners.
    static let bandHeightMultiplier: CGFloat = 3
}

/// Sweeps a brighter `.primary` highlight band across `content`'s own
/// glyphs over a dimmed `.secondary` base — text shimmering in place, not a
/// skeleton bar. A `TimelineView(.animation)` derives the band's offset
/// from elapsed wall-clock time: the offset holds at "fully off the right
/// edge" for `pauseDuration` after each sweep completes, which gives the
/// sweep/pause/sweep cadence from plain arithmetic instead of chained
/// `repeatForever` animation state.
private struct CompanionTextShimmerSweep: ViewModifier {
    func body(content: Content) -> some View {
        content
            .foregroundStyle(.secondary)
            .overlay {
                TimelineView(.animation) { timeline in
                    GeometryReader { proxy in
                        highlightBand(
                            in: proxy.size,
                            elapsed: timeline.date.timeIntervalSinceReferenceDate
                        )
                    }
                }
                .mask(content)
            }
    }

    private func highlightBand(in size: CGSize, elapsed: TimeInterval) -> some View {
        let bandWidth = CompanionTextShimmerTuning.bandWidth
        // Travels from fully off the left edge to fully off the right edge,
        // same reasoning as CompanionShimmerSweep's skeleton band.
        let travel = size.width + bandWidth
        let sweepDuration = TimeInterval(travel / CompanionTextShimmerTuning.speed)
        let cycleDuration = sweepDuration + CompanionTextShimmerTuning.pauseDuration
        let cyclePhase = cycleDuration > 0 ? elapsed.truncatingRemainder(dividingBy: cycleDuration) : 0
        // Clamped to 1 once a sweep finishes, so the band parks off-screen
        // (at `-bandWidth + travel`, i.e. fully past the right edge) for the
        // remainder of the cycle instead of sweeping continuously.
        let sweepProgress = sweepDuration > 0 ? min(cyclePhase / sweepDuration, 1) : 1
        let xOffset = -bandWidth + CGFloat(sweepProgress) * travel

        return LinearGradient(
            colors: [.clear, Color.primary.opacity(CompanionTextShimmerTuning.highlightOpacity), .clear],
            startPoint: .leading,
            endPoint: .trailing
        )
        .frame(width: bandWidth, height: size.height * CompanionTextShimmerTuning.bandHeightMultiplier)
        .rotationEffect(.degrees(CompanionTextShimmerTuning.angleDegrees))
        .offset(x: xOffset)
    }
}

private struct CompanionTextShimmerModifier: ViewModifier {
    let isActive: Bool

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        if !isActive {
            content
        } else if reduceMotion {
            content.modifier(CompanionShimmerPulse())
        } else {
            content.modifier(CompanionTextShimmerSweep())
        }
    }
}

extension View {
    /// Apple-Intelligence-style text shimmer for glyphs that are actively
    /// "alive" right now (e.g. a live session's streaming preview): a
    /// diagonal highlight sweeps across the dimmed text, rests briefly, then
    /// sweeps again. Falls back to the existing opacity pulse under Reduce
    /// Motion. Renders as plain `content` while `active` is false — the
    /// `TimelineView` driving the sweep is never constructed for inactive
    /// content, so rows that aren't alive don't pay for it.
    func companionTextShimmer(active: Bool) -> some View {
        modifier(CompanionTextShimmerModifier(isActive: active))
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
