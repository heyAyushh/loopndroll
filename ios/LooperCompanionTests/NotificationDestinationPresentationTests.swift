import Foundation
import Testing
@testable import Looper

@Suite("Notification destination presentation")
struct NotificationDestinationPresentationTests {
    private enum Constants {
        static let testDefaultsSuite = "dev.looper.tests.quick-actions"
    }

    @Test("Telegram and Slack keep channel-specific branding")
    func telegramAndSlackKeepChannelSpecificBranding() {
        let telegram = NotificationDestinationPresentation(
            label: "Telegram DM",
            channelIdentifier: "telegram"
        )
        let slack = NotificationDestinationPresentation(
            label: "Slack Build",
            channelIdentifier: "slack"
        )

        #expect(telegram.title == "Telegram DM")
        #expect(telegram.channelTitle == "Telegram")
        #expect(telegram.systemImageName == "paperplane.fill")
        #expect(telegram.channelKind == .telegram)
        #expect(slack.title == "Slack Build")
        #expect(slack.channelTitle == "Slack")
        #expect(slack.systemImageName == "number")
        #expect(slack.channelKind == .slack)
    }

    @Test("Built-in and unknown targets get readable labels")
    func builtInAndUnknownTargetsGetReadableLabels() {
        let macOS = NotificationDestinationPresentation(
            label: "",
            channelIdentifier: "macos"
        )
        let webhook = NotificationDestinationPresentation(
            label: "",
            channelIdentifier: "custom_webhook"
        )

        #expect(macOS.title == "macOS")
        #expect(macOS.channelTitle == "macOS")
        #expect(macOS.systemImageName == "macbook")
        #expect(webhook.title == "Custom Webhook")
        #expect(webhook.channelTitle == "Custom Webhook")
        #expect(webhook.systemImageName == "bell.badge")
    }

    @Test("Stop quick actions default to reply first")
    func stopQuickActionsDefaultToReplyFirst() {
        #expect(QuickActionSettings.defaultActions.contains(.reply))
        #expect(QuickActionSettings.notificationPresentationOrder.first == .reply)
        #expect(
            QuickActionSettings.storageValue(for: QuickActionSettings.defaultActions)
                .hasPrefix(QuickActionOption.reply.rawValue)
        )
    }

    @Test("Legacy stop quick action defaults gain reply")
    func legacyStopQuickActionDefaultsGainReply() throws {
        let defaults = try #require(UserDefaults(suiteName: Constants.testDefaultsSuite))
        defaults.removeObject(forKey: QuickActionSettings.storageKey)
        defer {
            defaults.removeObject(forKey: QuickActionSettings.storageKey)
        }

        defaults.set("open-session,continue", forKey: QuickActionSettings.storageKey)

        #expect(QuickActionSettings.loadSelectedActions(userDefaults: defaults).contains(.reply))
    }

    @MainActor
    @Test("Session quick action submission waits for handler")
    func sessionQuickActionSubmissionWaitsForHandler() async {
        let center = SessionQuickActionCenter()
        var handledPrompts: [String] = []

        center.registerHandler { request in
            try? await Task.sleep(for: .milliseconds(10))
            handledPrompts.append(request.prompt ?? "")
        }

        await center.submit(SessionQuickActionRequest(
            action: .reply,
            sessionID: "thread-1",
            prompt: "keep going"
        ))

        #expect(handledPrompts == ["keep going"])
    }
}
