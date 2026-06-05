import UIKit

private enum HapticFeedbackTiming {
    static let notificationMinimumInterval: TimeInterval = 0.7
    static let selectionMinimumInterval: TimeInterval = 0.12
    static let impactMinimumInterval: TimeInterval = 0.25
    static let unsetFeedbackTime = -Double.greatestFiniteMagnitude
}

@MainActor
enum Haptics {
    private static let notificationGenerator = UINotificationFeedbackGenerator()
    private static let selectionGenerator = UISelectionFeedbackGenerator()
    private static let impactGenerator = UIImpactFeedbackGenerator(style: .light)

    private static var lastNotificationFeedbackTime = HapticFeedbackTiming.unsetFeedbackTime
    private static var lastSelectionFeedbackTime = HapticFeedbackTiming.unsetFeedbackTime
    private static var lastImpactFeedbackTime = HapticFeedbackTiming.unsetFeedbackTime

    static func success() {
        playNotification(.success)
    }

    static func warning() {
        playNotification(.warning)
    }

    static func error() {
        playNotification(.error)
    }

    static func selectionChanged() {
        guard canPlay(
            lastFeedbackTime: &lastSelectionFeedbackTime,
            minimumInterval: HapticFeedbackTiming.selectionMinimumInterval
        ) else {
            return
        }

        selectionGenerator.selectionChanged()
        selectionGenerator.prepare()
    }

    static func impact() {
        guard canPlay(
            lastFeedbackTime: &lastImpactFeedbackTime,
            minimumInterval: HapticFeedbackTiming.impactMinimumInterval
        ) else {
            return
        }

        impactGenerator.impactOccurred()
        impactGenerator.prepare()
    }

    private static func playNotification(_ type: UINotificationFeedbackGenerator.FeedbackType) {
        guard canPlay(
            lastFeedbackTime: &lastNotificationFeedbackTime,
            minimumInterval: HapticFeedbackTiming.notificationMinimumInterval
        ) else {
            return
        }

        notificationGenerator.notificationOccurred(type)
        notificationGenerator.prepare()
    }

    private static func canPlay(
        lastFeedbackTime: inout TimeInterval,
        minimumInterval: TimeInterval
    ) -> Bool {
        let currentFeedbackTime = Date().timeIntervalSinceReferenceDate
        guard currentFeedbackTime - lastFeedbackTime >= minimumInterval else {
            return false
        }

        lastFeedbackTime = currentFeedbackTime
        return true
    }
}
