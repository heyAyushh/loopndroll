import Testing
@testable import Looper

@Suite("Notification destination presentation")
struct NotificationDestinationPresentationTests {
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
}
