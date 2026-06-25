import Foundation
import LooperMenuBarCore
import UserNotifications

@MainActor
final class LooperDesktopNotificationCenter: NSObject, UNUserNotificationCenterDelegate {
  typealias OpenSessionHandler = @MainActor @Sendable (String) async -> Void
  typealias ReplyHandler = @MainActor @Sendable (String, String, String) async -> Void

  private enum Layout {
    static let categorySessionStop = "looper-session-stop"
    static let actionOpenSession = "open-session"
    static let actionReply = "reply"
    static let notificationKindKey = "notificationKind"
    static let notificationKindSessionStop = "session-stop"
    static let sessionIDKey = "sessionId"
    static let sessionRefKey = "sessionRef"
    static let replyTitle = "Reply"
    static let openTitle = "Open"
    static let replyButtonTitle = "Send"
    static let replyPlaceholder = "Message"
    static let fallbackTitle = "Session stopped"
    static let fallbackBody = "Ready for your next prompt."
    static let subtitle = "Ready for reply"
    static let maxNotificationBodyCharacterCount = 220
    static let maxDeliveredEventKeyCount = 128
  }

  private let notificationCenter: UNUserNotificationCenter
  private let openSession: OpenSessionHandler
  private let replyToSession: ReplyHandler
  private var deliveredEventKeys: [String] = []
  private var deliveredEventKeySet: Set<String> = []

  init(
    notificationCenter: UNUserNotificationCenter = .current(),
    openSession: @escaping OpenSessionHandler,
    replyToSession: @escaping ReplyHandler
  ) {
    self.notificationCenter = notificationCenter
    self.openSession = openSession
    self.replyToSession = replyToSession
  }

  func start() {
    notificationCenter.delegate = self
    configureCategories()
    Task {
      _ = await requestAuthorizationIfNeeded()
    }
  }

  func deliverSessionStop(
    notificationID: String,
    threadID: String,
    title: String?,
    body: String?
  ) async -> Bool {
    guard rememberDeliveryKey(notificationID) else {
      return false
    }
    guard await requestAuthorizationIfNeeded() else {
      return false
    }

    let content = UNMutableNotificationContent()
    content.title = notificationTitle(title)
    content.subtitle = Layout.subtitle
    content.body = notificationBody(body)
    content.sound = .default
    content.categoryIdentifier = Layout.categorySessionStop
    content.threadIdentifier = threadID
    content.targetContentIdentifier = threadID
    content.interruptionLevel = .active
    content.userInfo = [
      Layout.notificationKindKey: Layout.notificationKindSessionStop,
      Layout.sessionIDKey: threadID,
      Layout.sessionRefKey: threadID,
    ]

    let request = UNNotificationRequest(
      identifier: sanitizedNotificationIdentifier(notificationID),
      content: content,
      trigger: nil
    )

    do {
      try await notificationCenter.add(request)
      return true
    } catch {
      return false
    }
  }

  nonisolated func userNotificationCenter(
    _: UNUserNotificationCenter,
    willPresent _: UNNotification,
    withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
  ) {
    completionHandler([.banner, .list, .sound])
  }

  nonisolated func userNotificationCenter(
    _: UNUserNotificationCenter,
    didReceive response: UNNotificationResponse,
    withCompletionHandler completionHandler: @escaping () -> Void
  ) {
    let action = DesktopNotificationResponseAction(
      notificationID: response.notification.request.identifier,
      threadID: response.notification.request.content.userInfo[Layout.sessionIDKey] as? String,
      actionIdentifier: response.actionIdentifier,
      replyText: (response as? UNTextInputNotificationResponse)?.userText
    )
    // UNUserNotificationCenter still provides a legacy completion closure; Swift 6
    // cannot infer its sendability, but this path calls it exactly once on MainActor.
    nonisolated(unsafe) let complete = completionHandler
    Task { @MainActor [weak self] in
      await self?.handle(action: action)
      complete()
    }
  }

  private func configureCategories() {
    let reply = UNTextInputNotificationAction(
      identifier: Layout.actionReply,
      title: Layout.replyTitle,
      options: [],
      textInputButtonTitle: Layout.replyButtonTitle,
      textInputPlaceholder: Layout.replyPlaceholder
    )
    let open = UNNotificationAction(
      identifier: Layout.actionOpenSession,
      title: Layout.openTitle,
      options: [.foreground]
    )
    notificationCenter.setNotificationCategories([
      UNNotificationCategory(
        identifier: Layout.categorySessionStop,
        actions: [reply, open],
        intentIdentifiers: [],
        options: []
      )
    ])
  }

  private func requestAuthorizationIfNeeded() async -> Bool {
    let current = await notificationCenter.notificationSettings().authorizationStatus
    if isAuthorized(current) {
      return true
    }
    guard current == .notDetermined else {
      return false
    }

    let granted = (try? await notificationCenter.requestAuthorization(options: [.alert, .sound]))
      ?? false
    guard granted else {
      return false
    }
    return isAuthorized(await notificationCenter.notificationSettings().authorizationStatus)
  }

  private func isAuthorized(_ status: UNAuthorizationStatus) -> Bool {
    switch status {
    case .authorized, .provisional:
      return true
    case .denied, .notDetermined:
      return false
    @unknown default:
      return false
    }
  }

  private func handle(action: DesktopNotificationResponseAction) async {
    guard
      let threadID = action.threadID,
      !threadID.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    else {
      return
    }

    switch action.actionIdentifier {
    case UNNotificationDefaultActionIdentifier, Layout.actionOpenSession:
      await openSession(threadID)
    case Layout.actionReply:
      guard
        let prompt = action.replyText?.nonBlankTrimmed
      else {
        return
      }
      await replyToSession(action.notificationID, threadID, prompt)
    default:
      return
    }
  }

  private func notificationTitle(_ title: String?) -> String {
    title?.nonBlankTrimmed ?? Layout.fallbackTitle
  }

  private func notificationBody(_ body: String?) -> String {
    let body = body?.nonBlankTrimmed ?? Layout.fallbackBody
    return body.truncated(maxCharacters: Layout.maxNotificationBodyCharacterCount)
  }

  private func sanitizedNotificationIdentifier(_ notificationID: String) -> String {
    notificationID.split(separator: "-")
      .map(String.init)
      .map(sanitizedIdentifierComponent)
      .joined(separator: "-")
  }

  private func rememberDeliveryKey(_ key: String) -> Bool {
    guard !deliveredEventKeySet.contains(key) else {
      return false
    }

    deliveredEventKeySet.insert(key)
    deliveredEventKeys.append(key)
    while deliveredEventKeys.count > Layout.maxDeliveredEventKeyCount {
      let removed = deliveredEventKeys.removeFirst()
      deliveredEventKeySet.remove(removed)
    }
    return true
  }

  private func sanitizedIdentifierComponent(_ value: String) -> String {
    let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_"))
    return value.unicodeScalars.map { scalar in
      allowed.contains(scalar) ? String(scalar) : "-"
    }
    .joined()
  }
}

private struct DesktopNotificationResponseAction: Sendable {
  let notificationID: String
  let threadID: String?
  let actionIdentifier: String
  let replyText: String?
}

private extension String {
  var nonBlankTrimmed: String? {
    let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
  }

  func truncated(maxCharacters: Int) -> String {
    guard count > maxCharacters else {
      return self
    }
    let suffix = "..."
    let limit = max(maxCharacters - suffix.count, 0)
    return String(prefix(limit)).trimmingCharacters(in: .whitespacesAndNewlines) + suffix
  }
}
