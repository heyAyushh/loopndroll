import XCTest

private enum ControlTapMetrics {
    static let trailingSwitchWidthFraction: CGFloat = 0.25
    static let maximumTrailingSwitchInset: CGFloat = 44
    static let scopeMenuTimeout: TimeInterval = 4
    static let springboardPageSwipeLimit = 4
    static let systemAppPollInterval: TimeInterval = 0.2
    static let textViewLeadingFocusInset: CGFloat = 24
    static let textViewTopFocusInset: CGFloat = 24
    static let keyboardDismissXFraction: CGFloat = 0.5
    static let keyboardDismissYFraction: CGFloat = 0.12
    static let keyboardFocusTimeout: TimeInterval = 0.5
    static let keyboardDismissSwipeLimit = 3
    static let notificationDeliveryTimeout: TimeInterval = 8
    static let promptQueueTimeout: TimeInterval = 12
    static let mutationControlTimeout: TimeInterval = 15
}

private enum LiveLatencyMetrics {
    static let baseURLsEnvironmentKey = "LOOPER_LIVE_LATENCY_BASE_URLS"
    static let bearerTokenEnvironmentKey = "LOOPER_LIVE_LATENCY_BEARER_TOKEN"
    static let mobileSessionEnvironmentKey = "LOOPER_LIVE_LATENCY_MOBILE_SESSION"
    static let outputEnvironmentKey = "LOOPER_LIVE_LATENCY_OUTPUT"
    static let sessionTitleEnvironmentKey = "LOOPER_LIVE_LATENCY_SESSION_TITLE"
    static let appBaseURLsEnvironmentKey = "LOOPER_UI_TEST_API_BASE_URLS"
    static let appBearerTokenEnvironmentKey = "LOOPER_UI_TEST_API_BEARER_TOKEN"
    static let appMobileSessionEnvironmentKey = "LOOPER_UI_TEST_MOBILE_SESSION"
    static let uiTestEnvironmentKey = "LOOPER_UI_TEST"
    static let defaultSessionTitle = "Looper latency fixture"
    static let promptText = "Live latency prompt"
    static let awaitReplyModeIdentifier = "session-detail.mode.await-reply"
    static let infiniteModeIdentifier = "session-detail.mode.infinite"
    static let millisecondsPerSecond = 1_000.0
    static let connectionTimeout: TimeInterval = 8
    static let detailTimeout: TimeInterval = 6
    static let mutationTimeout: TimeInterval = 6
    static let promptAckTimeout: TimeInterval = 6
    static let pollInterval: TimeInterval = 0.05
}

private struct LiveLatencyConfiguration {
    let baseURLs: String
    let bearerToken: String
    let mobileSession: String
    let outputPath: String?
    let sessionTitle: String
}

private struct LiveLatencyResult: Encodable {
    let connectMilliseconds: Int
    let modeSwitchMilliseconds: Int
    let promptAckMilliseconds: Int
    let totalMilliseconds: Int
    let sessionTitle: String
}

private enum ControlFlowLaunchArgument {
    static let openOrbScannerOnLaunch = "--open-orb-scanner-on-launch"
    static let sendTestAlertOnLaunch = "--send-test-alert-on-launch"
}

private enum SystemAppBundleID {
    static let siriCandidates = ["com.apple.SiriApp", "com.apple.siri"]
}

private enum SystemAppSurfaceText {
    static let siriUpdateInProgress = "Siri Update in Progress"
}

@MainActor
final class LooperCompanionControlFlowUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    func testOnboardingControlsReachMainApp() throws {
        launchApp(showOnboarding: true)

        XCTAssertTrue(waitForText("Set Up looper"))
        recordSurfaceEvidence("ios-onboarding")
        XCTAssertTrue(app.textFields["Enter device code"].exists)
        XCTAssertFalse(app.buttons["Login with Device Code"].isEnabled)

        app.textFields["Enter device code"].tap()
        app.textFields["Enter device code"].typeText("http://127.0.0.1:8765")
        XCTAssertTrue(app.buttons["Login with Device Code"].isEnabled)
        app.buttons["Login with Device Code"].tap()
        XCTAssertTrue(waitForEnabledButton("Scan Mac Orb", timeout: 6))
        dismissKeyboard(identifier: "onboarding.keyboard-done")

        scrollToControl(named: "Enable Local Network")
        tapVisibleControl(named: "Enable Local Network")
        scrollToControl(named: "Enable Notifications")
        tapVisibleControl(named: "Enable Notifications")
        handleNotificationPromptIfNeeded()

        let onboardingFaceIDSwitch = app.switches["onboarding.face-id-unlock"].firstMatch
        scrollToElementFrame(onboardingFaceIDSwitch)
        XCTAssertTrue(onboardingFaceIDSwitch.exists)
        XCTAssertTrue(waitForText("Face ID", allowsPartial: true))

        let startButton = app.buttons["onboarding.start"].firstMatch
        scrollToElement(startButton)
        XCTAssertTrue(startButton.waitForExistence(timeout: 4))
        XCTAssertTrue(waitForEnabledButton(startButton))
        startButton.tap()
        XCTAssertTrue(waitForText("Sessions"))
        recordSurfaceEvidence("ios-sessions-after-onboarding")
    }

    func testOrbScannerLaunchControls() throws {
        launchApp(extraArguments: [ControlFlowLaunchArgument.openOrbScannerOnLaunch])

        let scannerCloseButton = closeScannerButton
        XCTAssertTrue(scannerCloseButton.waitForExistence(timeout: 10))
        XCTAssertTrue(waitForText("Camera Unavailable", timeout: 6))
        XCTAssertTrue(waitForText("Upload Image", timeout: 6))
        assertScannerControls(canConnectPastedOrbID: false)
        recordSurfaceEvidence("ios-scanner-fallback")
        scannerCloseButton.tap()
        XCTAssertTrue(waitForText("Sessions"))
    }

    func testSessionsDeviceHubAndSessionDetailControls() throws {
        launchApp()

        XCTAssertTrue(waitForText("Sessions"))
        XCTAssertTrue(waitForText("Make an iOS app for looper"))
        XCTAssertTrue(waitForText("Goal blocked"))
        recordSurfaceEvidence("ios-sessions-list")

        for surfaceID in ["claude-code", "zed", "devin", "grok-build", "codex"] {
            tapAssistantSurface(surfaceID)
        }
        XCTAssertTrue(waitForText("Make an iOS app for looper"))

        tapButton(identifier: "sessions.open-device-hub")
        XCTAssertTrue(waitForText("This iPhone"))
        XCTAssertTrue(waitForText("Scan Mac Orb"))
        XCTAssertTrue(waitForText("Notifications"))
        recordSurfaceEvidence("ios-device-hub")
        tapButton(identifier: "device-hub.alert-action")
        XCTAssertTrue(waitForText("Notifications"))
        XCTAssertTrue(app.buttons["device-hub.scan-orb"].exists)
        app.buttons["device-hub.done"].tap()
        XCTAssertTrue(waitForText("Sessions"))

        app.staticTexts["Make an iOS app for looper"].tap()
        XCTAssertTrue(waitForButton("Use with Siri"))
        XCTAssertTrue(waitForText("Goal blocked"))
        recordSurfaceEvidence("ios-session-detail")
        tapVisibleControl(named: "Queue")

        let promptEditor = app.textViews["session-detail.prompt-editor"].firstMatch
        scrollToElementFrame(promptEditor)
        focusTextView(promptEditor)
        let firstPromptSuggestion = app.buttons["session-detail.prompt-suggestion.0"].firstMatch
        XCTAssertTrue(firstPromptSuggestion.waitForExistence(timeout: 2))
        firstPromptSuggestion.tap()
        focusTextView(promptEditor)
        promptEditor.typeText(" UI test prompt")
        dismissKeyboard(identifier: "session-detail.keyboard-done")
        scrollToElement(app.buttons["session-detail.send-prompt"])
        let sendPromptButton = app.buttons["session-detail.send-prompt"].firstMatch
        XCTAssertTrue(waitForEnabledButton(sendPromptButton, timeout: 6))
        sendPromptButton.tap()
        XCTAssertTrue(waitForDisabledButton(sendPromptButton, timeout: 6))
        tapBackButton()
        XCTAssertTrue(
            waitForText(
                "Queued prompt:",
                timeout: ControlTapMetrics.promptQueueTimeout,
                allowsPartial: true
            )
        )
        XCTAssertTrue(waitForText("UI test prompt", timeout: 6, allowsPartial: true))
        app.staticTexts["Make an iOS app for looper"].tap()
        XCTAssertTrue(waitForButton("Use with Siri"))
        app.buttons["Use with Siri"].tap()
    }

    func testSessionModeControls() throws {
        launchApp()
        openPrimarySessionDetail()
        tapVisibleControl(named: "Queue")
        for modeLabel in [
            "Await Reply",
            "Completion Checks",
            "Max Turns 1",
            "Max Turns 2",
            "Max Turns 3",
            "Use Global Default",
            "Infinite"
        ] {
            scrollToControl(named: modeLabel)
            tapVisibleControl(named: modeLabel)
        }
    }

    func testSessionManagementControls() throws {
        launchApp()
        openPrimarySessionDetail()
        scrollToControl(named: "Archive Session")
        tapEnabledButton(identifier: "session-detail.archive-toggle")
        XCTAssertTrue(waitForButton("Unarchive Session"))
        tapEnabledVisibleButton(named: "Unarchive Session")
        XCTAssertTrue(waitForButton("Archive Session"))

        tapEnabledButton(identifier: "session-detail.delete")
        let firstDeleteConfirmButton = app.buttons["session-detail.delete-confirm"].firstMatch
        XCTAssertTrue(firstDeleteConfirmButton.waitForExistence(timeout: 4))
        let deleteCancelButton = dialogButton(identifier: "session-detail.delete-cancel", fallbackLabel: "Keep Session")
        XCTAssertTrue(deleteCancelButton.waitForExistence(timeout: 4))
        deleteCancelButton.tap()
        XCTAssertTrue(waitForButton("Archive Session"))

        tapEnabledButton(identifier: "session-detail.delete")
        let secondDeleteConfirmButton = app.buttons["session-detail.delete-confirm"].firstMatch
        XCTAssertTrue(secondDeleteConfirmButton.waitForExistence(timeout: 4))
        secondDeleteConfirmButton.tap()
        XCTAssertTrue(waitForText("Sessions"))
        XCTAssertFalse(app.staticTexts["Make an iOS app for looper"].waitForExistence(timeout: 2))
    }

    func testSettingsControls() throws {
        launchApp()

        XCTAssertTrue(waitForText("Sessions"))
        app.buttons["Settings"].tap()
        XCTAssertTrue(waitForText("Settings"))
        recordSurfaceEvidence("ios-settings")

        tapVisibleControl(named: "Remote")
        tapVisibleControl(named: "Tailscale")
        tapVisibleControl(named: "LAN")

        let localNetworkSwitch = app.switches["settings.local-network-access"].firstMatch
        scrollToElementFrame(localNetworkSwitch)
        tapSwitch(localNetworkSwitch)

        let settingsConnectionField = app.textFields["settings.connection-code"].firstMatch
        scrollToElementFrame(settingsConnectionField)
        settingsConnectionField.tap()
        settingsConnectionField.typeText("http://127.0.0.1:8765")
        dismissKeyboard(identifier: "settings.keyboard-done")
        XCTAssertTrue(waitForEnabledButton(app.buttons["settings.login-device-code"].firstMatch))
        app.buttons["settings.login-device-code"].tap()

        XCTAssertTrue(app.buttons["settings.scan-mac-orb"].exists)

        XCTAssertTrue(app.buttons["settings.open-tailscale"].exists)

        typeText(
            " UI test",
            inTextView: "settings.default-prompt-editor"
        )
        dismissKeyboard(identifier: "settings.keyboard-done")
        XCTAssertTrue(app.buttons["settings.save"].isEnabled)
        app.buttons["settings.save"].tap()

        for actionID in ["open-session", "continue", "reply", "archive", "mute-session"] {
            tapSwitch(app.switches["settings.quick-action.\(actionID)"].firstMatch)
        }

        let faceIDLabel = control(named: "Face ID Unlock")
        scrollToElementFrame(faceIDLabel)
        let faceIDControl = app.descendants(matching: .any)
            .matching(identifier: "settings.face-id-unlock")
            .firstMatch
        if faceIDControl.waitForExistence(timeout: 1), faceIDControl.isEnabled, isVisibleFrame(faceIDControl.frame) {
            tapSwitch(faceIDControl)
        }
        XCTAssertTrue(
            waitForText("Face ID", timeout: 6, allowsPartial: true),
            "Face ID settings should either expose an enabled toggle or show the expected unavailable/authentication status."
        )

        scrollToControl(named: "Appearance")
        tapVisibleControl(named: "Dark")
        tapVisibleControl(named: "Light")
        tapVisibleControl(named: "System")

        scrollToControl(named: "Notification Routes")
        tapVisibleControl(named: "Notification Routes")
        XCTAssertTrue(waitForText("Notification Routes"))
        tapBackButton()
        XCTAssertTrue(waitForText("Settings"))

        scrollToControl(named: "Completion Checks")
        tapVisibleControl(named: "Completion Checks")
        XCTAssertTrue(waitForText("Completion Checks"))
        tapBackButton()
        XCTAssertTrue(waitForText("Settings"))

        scrollToTop()
        tapButton(identifier: "settings.run-setup-again")
        XCTAssertTrue(waitForText("Set Up looper"))
    }

    func testSearchControlsAndDestinations() throws {
        launchApp(extraArguments: ["--looper-seed-recent-searches"])

        XCTAssertTrue(waitForText("Sessions"))
        XCTAssertTrue(tapBottomSearchTab())
        XCTAssertTrue(waitForText("Search"))
        recordSurfaceEvidence("ios-search")

        let recentQuery = app.buttons["search.recent-query.looper"].firstMatch
        XCTAssertTrue(recentQuery.waitForExistence(timeout: 6))
        tapCenter(of: recentQuery)
        XCTAssertTrue(searchFieldContains("looper"))
        clearSearchField()
        XCTAssertTrue(waitForText("Search"))

        let searchField = app.searchFields.firstMatch
        XCTAssertTrue(searchField.waitForExistence(timeout: 6))
        searchField.tap()
        searchField.typeText("device")
        tapSearchScope("device")
        tapButton(identifier: "search.action.openDeviceHub")
        XCTAssertTrue(waitForText("This iPhone"))
        app.buttons["device-hub.done"].tap()
        XCTAssertTrue(waitForText("Search"))

        clearSearchField()
        searchField.tap()
        searchField.typeText("looper")
        tapSearchScope("sessions")
        tapSearchScope("actions")
        tapSearchScope("settings")
        tapSearchScope("device")
        tapSearchScope("all")
        tapSearchScope("sessions")
        tapVisibleControl(named: "Make an iOS app for looper")
        XCTAssertTrue(waitForButton("Use with Siri"))
        tapBackButton()
        XCTAssertTrue(waitForText("Search"))

        clearSearchField()
        searchField.tap()
        searchField.typeText("notification")
        tapSearchScope("settings")
        tapButton(identifier: "search.settings.notificationRoutes")
        XCTAssertTrue(waitForText("Notification Routes"))
        tapBackButton()
        XCTAssertTrue(waitForText("Search"))

        clearSearchField()
        searchField.tap()
        searchField.typeText("completion")
        tapSearchScope("settings")
        tapButton(identifier: "search.settings.completionChecks")
        XCTAssertTrue(waitForText("Completion Checks"))
        tapBackButton()
        XCTAssertTrue(waitForText("Search"))

        clearSearchField()
        searchField.tap()
        searchField.typeText("alert")
        tapSearchScope("actions")
        tapButton(identifier: "search.action.sendTestAlert")
        XCTAssertTrue(waitForText("Search"))
    }

    func testLaunchVerificationAlertReachesSimulatorNotifications() throws {
        launchApp(extraArguments: [ControlFlowLaunchArgument.sendTestAlertOnLaunch])
        handleNotificationPromptIfNeeded()

        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        XCTAssertTrue(
            waitForSpringboardText(
                "Local alerts enabled",
                in: springboard,
                timeout: ControlTapMetrics.notificationDeliveryTimeout
            )
        )
        recordSurfaceEvidence("ios-local-notification-delivered")
    }

    func testSiriAppSurfaceCanOpenFromSimulator() throws {
        launchApp()
        XCTAssertTrue(waitForText("Sessions"))

        XCUIDevice.shared.press(.home)
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let looperIcon = findSpringboardIcon(named: "looper", in: springboard)
        XCTAssertTrue(looperIcon.exists)

        let siriIcon = findSpringboardIcon(named: "Siri", in: springboard)
        XCTAssertTrue(siriIcon.exists)

        siriIcon.tap()
        XCTAssertTrue(waitForSiriSurface(springboard: springboard))
    }

    func testLiveRealtimeLatencyWorkflow() throws {
        let configuration = try liveLatencyConfiguration()

        launchLiveLatencyApp(configuration)
        let connectStartedAt = Date()
        XCTAssertTrue(
            pollForText(
                configuration.sessionTitle,
                timeout: LiveLatencyMetrics.connectionTimeout,
                allowsPartial: true
            )
        )
        let connectMilliseconds = elapsedMilliseconds(since: connectStartedAt)
        XCTAssertTrue(pollForText("Sessions", timeout: 1))

        app.staticTexts[configuration.sessionTitle].firstMatch.tap()
        XCTAssertTrue(pollForButton("Use with Siri", timeout: LiveLatencyMetrics.detailTimeout))
        tapVisibleControl(named: "Queue")

        let modeControl = app.buttons[LiveLatencyMetrics.awaitReplyModeIdentifier].firstMatch
        scrollToElement(modeControl)
        let modeStartedAt = Date()
        modeControl.tap()
        let modeReadinessControl = app.buttons[LiveLatencyMetrics.infiniteModeIdentifier].firstMatch
        XCTAssertTrue(
            pollForEnabledButton(
                modeReadinessControl,
                timeout: LiveLatencyMetrics.mutationTimeout
            )
        )
        let modeSwitchMilliseconds = elapsedMilliseconds(since: modeStartedAt)

        typeText(
            LiveLatencyMetrics.promptText,
            inTextView: "session-detail.prompt-editor"
        )
        dismissKeyboard(identifier: "session-detail.keyboard-done")
        let sendPromptButton = app.buttons["session-detail.send-prompt"].firstMatch

        scrollToElement(sendPromptButton)
        XCTAssertTrue(waitForEnabledButton(sendPromptButton, timeout: LiveLatencyMetrics.mutationTimeout))

        let promptStartedAt = Date()
        sendPromptButton.tap()
        XCTAssertTrue(
            pollForPromptEditorCleared(timeout: LiveLatencyMetrics.promptAckTimeout)
        )
        let promptAckMilliseconds = elapsedMilliseconds(since: promptStartedAt)
        let totalMilliseconds = connectMilliseconds + modeSwitchMilliseconds + promptAckMilliseconds

        let result = LiveLatencyResult(
            connectMilliseconds: connectMilliseconds,
            modeSwitchMilliseconds: modeSwitchMilliseconds,
            promptAckMilliseconds: promptAckMilliseconds,
            totalMilliseconds: totalMilliseconds,
            sessionTitle: configuration.sessionTitle
        )
        recordLiveLatencyResult(result, outputPath: configuration.outputPath)
    }

    private func launchApp(showOnboarding: Bool = false, extraArguments: [String] = []) {
        app = XCUIApplication()
        app.terminate()
        app.launchArguments = [
            "--looper-ui-test-mode",
            "--looper-reset-ui-test-state"
        ]
        app.launchArguments.append(contentsOf: extraArguments)
        if showOnboarding {
            app.launchArguments.append("--looper-show-onboarding")
        }
        app.launchEnvironment["LOOPER_UI_TEST"] = "1"
        app.launch()
    }

    private func launchLiveLatencyApp(_ configuration: LiveLatencyConfiguration) {
        app = XCUIApplication()
        app.terminate()
        app.launchArguments = [
            "--looper-reset-ui-test-state"
        ]
        app.launchEnvironment[LiveLatencyMetrics.uiTestEnvironmentKey] = "1"
        app.launchEnvironment[LiveLatencyMetrics.appBaseURLsEnvironmentKey] = configuration.baseURLs
        app.launchEnvironment[LiveLatencyMetrics.appBearerTokenEnvironmentKey] = configuration.bearerToken
        app.launchEnvironment[LiveLatencyMetrics.appMobileSessionEnvironmentKey] = configuration.mobileSession
        app.launch()
    }

    private func liveLatencyConfiguration() throws -> LiveLatencyConfiguration {
        let environment = ProcessInfo.processInfo.environment
        guard let baseURLs = nonEmptyEnvironmentValue(
            LiveLatencyMetrics.baseURLsEnvironmentKey,
            environment: environment
        ) else {
            throw XCTSkip("Live mobile latency check requires isolated server base URLs.")
        }
        guard let bearerToken = nonEmptyEnvironmentValue(
            LiveLatencyMetrics.bearerTokenEnvironmentKey,
            environment: environment
        ) else {
            throw XCTSkip("Live mobile latency check requires a pairing bearer token.")
        }
        guard let mobileSession = nonEmptyEnvironmentValue(
            LiveLatencyMetrics.mobileSessionEnvironmentKey,
            environment: environment
        ) else {
            throw XCTSkip("Live mobile latency check requires a passkey mobile session.")
        }

        return LiveLatencyConfiguration(
            baseURLs: baseURLs,
            bearerToken: bearerToken,
            mobileSession: mobileSession,
            outputPath: nonEmptyEnvironmentValue(
                LiveLatencyMetrics.outputEnvironmentKey,
                environment: environment
            ),
            sessionTitle: nonEmptyEnvironmentValue(
                LiveLatencyMetrics.sessionTitleEnvironmentKey,
                environment: environment
            ) ?? LiveLatencyMetrics.defaultSessionTitle
        )
    }

    private func nonEmptyEnvironmentValue(_ key: String, environment: [String: String]) -> String? {
        let value = environment[key]?.trimmingCharacters(in: .whitespacesAndNewlines)
        return value?.isEmpty == false ? value : nil
    }

    private func openPrimarySessionDetail() {
        XCTAssertTrue(waitForText("Sessions"))
        XCTAssertTrue(waitForText("Make an iOS app for looper"))
        app.staticTexts["Make an iOS app for looper"].tap()
        XCTAssertTrue(waitForButton("Use with Siri"))
    }

    private func recordLiveLatencyResult(_ result: LiveLatencyResult, outputPath: String?) {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = (try? encoder.encode(result)) ?? Data()
        if let outputPath, !data.isEmpty {
            try? data.write(to: URL(fileURLWithPath: outputPath), options: .atomic)
        }

        let payload = String(data: data, encoding: .utf8) ?? "{}"
        let attachment = XCTAttachment(string: payload)
        attachment.name = "mobile-realtime-latency.json"
        attachment.lifetime = .keepAlways
        add(attachment)
        print(
            "MOBILE_REALTIME_LATENCY " +
                "connectMs=\(result.connectMilliseconds) " +
                "modeSwitchMs=\(result.modeSwitchMilliseconds) " +
                "promptAckMs=\(result.promptAckMilliseconds) " +
                "totalMs=\(result.totalMilliseconds)"
        )
    }

    private func elapsedMilliseconds(since startDate: Date) -> Int {
        Int((Date().timeIntervalSince(startDate) * LiveLatencyMetrics.millisecondsPerSecond).rounded())
    }

    private func recordSurfaceEvidence(_ name: String) {
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "\(name).png"
        screenshot.lifetime = .keepAlways
        add(screenshot)

        let accessibilitySnapshot = XCTAttachment(string: app.debugDescription)
        accessibilitySnapshot.name = "\(name).accessibility.txt"
        accessibilitySnapshot.lifetime = .keepAlways
        add(accessibilitySnapshot)
    }

    private func findSpringboardIcon(named name: String, in springboard: XCUIApplication) -> XCUIElement {
        let icon = springboard.icons[name].firstMatch
        if icon.waitForExistence(timeout: 2) {
            return icon
        }

        for _ in 0..<ControlTapMetrics.springboardPageSwipeLimit {
            springboard.swipeLeft()
            if icon.waitForExistence(timeout: 2) {
                return icon
            }
        }

        return icon
    }

    private func waitForSiriSurface(springboard: XCUIApplication, timeout: TimeInterval = 10) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            for bundleID in SystemAppBundleID.siriCandidates {
                let candidate = XCUIApplication(bundleIdentifier: bundleID)
                if candidate.state == .runningForeground || systemSurfaceShowsSiri(candidate) {
                    return true
                }

                if candidate.wait(for: .runningForeground, timeout: ControlTapMetrics.systemAppPollInterval) {
                    return true
                }
            }

            if systemSurfaceShowsSiri(springboard) {
                return true
            }

            RunLoop.current.run(until: Date(timeIntervalSinceNow: ControlTapMetrics.systemAppPollInterval))
        }

        for bundleID in SystemAppBundleID.siriCandidates {
            let candidate = XCUIApplication(bundleIdentifier: bundleID)
            if candidate.state == .runningForeground || systemSurfaceShowsSiri(candidate) {
                return true
            }
        }

        if systemSurfaceShowsSiri(springboard) {
            return true
        }

        return false
    }

    private func waitForSpringboardText(
        _ text: String,
        in springboard: XCUIApplication,
        timeout: TimeInterval
    ) -> Bool {
        let textPredicate = NSPredicate(format: "label CONTAINS[c] %@", text)
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if springboard.descendants(matching: .any).matching(textPredicate).firstMatch.exists ||
                app.descendants(matching: .any).matching(textPredicate).firstMatch.exists {
                return true
            }

            RunLoop.current.run(until: Date(timeIntervalSinceNow: ControlTapMetrics.systemAppPollInterval))
        }

        return springboard.descendants(matching: .any).matching(textPredicate).firstMatch.exists ||
            app.descendants(matching: .any).matching(textPredicate).firstMatch.exists
    }

    private func systemSurfaceShowsSiri(_ application: XCUIApplication) -> Bool {
        application.staticTexts[SystemAppSurfaceText.siriUpdateInProgress].exists
    }

    private func waitForText(
        _ text: String,
        timeout: TimeInterval = 10,
        allowsPartial: Bool = false,
        file: StaticString = #filePath,
        line: UInt = #line
    ) -> Bool {
        let predicate: NSPredicate
        if allowsPartial {
            predicate = NSPredicate(format: "label CONTAINS[c] %@", text)
        } else {
            predicate = NSPredicate(format: "label == %@", text)
        }
        let element = app.descendants(matching: .any).matching(predicate).firstMatch
        return element.waitForExistence(timeout: timeout)
    }

    private func waitForButton(_ label: String, timeout: TimeInterval = 10) -> Bool {
        app.buttons[label].waitForExistence(timeout: timeout)
    }

    private func waitForEnabledButton(_ label: String, timeout: TimeInterval = 10) -> Bool {
        let button = app.buttons[label]
        return waitForEnabledButton(button, timeout: timeout)
    }

    private func waitForEnabledButton(_ button: XCUIElement, timeout: TimeInterval = 10) -> Bool {
        let predicate = NSPredicate(format: "exists == true AND enabled == true")
        let expectation = XCTNSPredicateExpectation(predicate: predicate, object: button)
        return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
    }

    private func waitForDisabledButton(_ button: XCUIElement, timeout: TimeInterval = 10) -> Bool {
        let predicate = NSPredicate(format: "exists == true AND enabled == false")
        let expectation = XCTNSPredicateExpectation(predicate: predicate, object: button)
        return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
    }

    private func waitForEnabledSwitch(_ label: String, timeout: TimeInterval = 10) -> Bool {
        let toggle = app.switches[label]
        let predicate = NSPredicate(format: "exists == true AND enabled == true")
        let expectation = XCTNSPredicateExpectation(predicate: predicate, object: toggle)
        return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
    }

    private func waitForDisabledSwitch(_ identifier: String, timeout: TimeInterval = 10) -> Bool {
        let toggle = app.switches[identifier]
        let predicate = NSPredicate(format: "exists == true AND enabled == false")
        let expectation = XCTNSPredicateExpectation(predicate: predicate, object: toggle)
        return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
    }

    private func scrollToControl(named label: String) {
        let element = control(named: label)
        scrollToElement(element)
    }

    private func tapVisibleControl(named label: String) {
        let element = control(named: label)
        scrollToElement(element)
        element.tap()
    }

    private func tapVisibleButton(named label: String) {
        let button = app.buttons[label].firstMatch
        scrollToElement(button)
        button.tap()
    }

    private func tapEnabledVisibleButton(named label: String) {
        let button = app.buttons[label].firstMatch
        scrollToElement(button)
        XCTAssertTrue(waitForEnabledButton(button, timeout: ControlTapMetrics.mutationControlTimeout))
        button.tap()
    }

    private func tapButton(identifier: String) {
        let button = app.buttons[identifier].firstMatch
        scrollToElement(button)
        button.tap()
    }

    private func tapEnabledButton(identifier: String) {
        let button = app.buttons[identifier].firstMatch
        scrollToElement(button)
        XCTAssertTrue(waitForEnabledButton(button, timeout: ControlTapMetrics.mutationControlTimeout))
        button.tap()
    }

    private func dialogButton(identifier: String, fallbackLabel: String) -> XCUIElement {
        let identifiedButton = app.buttons[identifier].firstMatch
        if identifiedButton.waitForExistence(timeout: 1) {
            return identifiedButton
        }

        return app.buttons[fallbackLabel].firstMatch
    }

    private func tapBackButton(file: StaticString = #filePath, line: UInt = #line) {
        for button in [app.buttons["Back"].firstMatch, app.buttons["BackButton"].firstMatch] {
            if button.waitForExistence(timeout: 1), button.isHittable {
                button.tap()
                return
            }
        }

        XCTFail("Could not find a hittable navigation back button", file: file, line: line)
    }

    private func tapAssistantSurface(_ rawValue: String) {
        let button = app.buttons["assistant.surface.\(rawValue)"].firstMatch
        if button.waitForExistence(timeout: 1), button.isHittable {
            button.tap()
            return
        }

        let picker = app.scrollViews["assistant.surface.picker"].firstMatch
        XCTAssertTrue(picker.waitForExistence(timeout: 4))
        for _ in 0..<5 {
            picker.swipeLeft()
            if button.exists, button.isHittable {
                button.tap()
                return
            }
        }

        for _ in 0..<5 {
            picker.swipeRight()
            if button.exists, button.isHittable {
                button.tap()
                return
            }
        }

        XCTFail("Could not tap assistant surface: \(rawValue)")
    }

    private func assertScannerControls(canConnectPastedOrbID: Bool) {
        tapButton(identifier: "scanner.controls")
        XCTAssertTrue(waitForText("Scan Orb"))
        XCTAssertTrue(app.buttons["scanner.upload-image"].waitForExistence(timeout: 6))
        XCTAssertTrue(app.buttons["scanner.choose-files"].waitForExistence(timeout: 2))

        let directOrbIDField = app.textFields["scanner.direct-orb-id"].firstMatch
        scrollToElementFrame(directOrbIDField)
        directOrbIDField.tap()
        directOrbIDField.typeText("orb_ui_test")
        dismissKeyboard(identifier: "scanner.controls.keyboard-done")

        if canConnectPastedOrbID {
            let connectButton = app.buttons["scanner.connect-pasted-orb-id"].firstMatch
            scrollToElement(connectButton)
            XCTAssertTrue(waitForEnabledButton(connectButton, timeout: 4))
        }

        tapButtonInScrollView(identifier: "scanner.reset")
        tapButton(identifier: "scanner.controls.done")
    }

    private func clearSearchField() {
        let searchField = app.searchFields.firstMatch
        XCTAssertTrue(searchField.waitForExistence(timeout: 4))
        if searchFieldIsEmpty(searchField) {
            return
        }

        if resetSearchInteraction() {
            return
        }

        searchField.tap()

        let clearButton = searchField.buttons.firstMatch
        if clearButton.waitForExistence(timeout: 1) {
            clearButton.tap()
            if searchFieldIsEmpty(searchField) {
                return
            }
        }

        searchField.press(forDuration: 0.5)
        if app.menuItems["Select All"].waitForExistence(timeout: 1) {
            app.menuItems["Select All"].tap()
            searchField.typeText(XCUIKeyboardKey.delete.rawValue)
            if searchFieldIsEmpty(searchField) {
                return
            }
        }

        searchField.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 32))
        XCTAssertTrue(searchFieldIsEmpty(searchField), "Search field did not clear before next query")
    }

    private func resetSearchInteraction() -> Bool {
        let closeButton = app.buttons["Close"].firstMatch
        guard closeButton.waitForExistence(timeout: 1) else {
            return false
        }

        closeButton.tap()
        guard tapBottomSearchTab() else { return false }
        let searchField = app.searchFields.firstMatch
        return searchField.waitForExistence(timeout: 6) && searchFieldIsEmpty(searchField)
    }

    private func tapBottomSearchTab() -> Bool {
        let predicate = NSPredicate(format: "label ==[c] %@ OR identifier ==[c] %@", "Search", "Search")
        let buttons = app.buttons.matching(predicate).allElementsBoundByIndex
        guard let searchTab = buttons.first(where: { button in
            isVisibleFrame(button.frame) && button.frame.midY > app.frame.midY
        }) else {
            return false
        }

        tapCenterWithoutScrolling(searchTab)
        return true
    }

    private func searchFieldContains(_ expectedText: String) -> Bool {
        let searchField = app.searchFields.firstMatch
        guard searchField.waitForExistence(timeout: 6) else {
            return false
        }

        let value = String(describing: searchField.value ?? "")
        return value.localizedCaseInsensitiveContains(expectedText)
    }

    private func searchFieldIsEmpty(_ searchField: XCUIElement) -> Bool {
        let value = String(describing: searchField.value ?? "")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return value.isEmpty || value == "Sessions, settings, actions"
    }

    private func tapSearchScope(_ rawValue: String) {
        let title = searchScopeTitle(rawValue)
        if tapNativeSearchScopeButton(title) {
            return
        }

        XCTFail("Could not tap native search scope: \(title)")
    }

    private func tapNativeSearchScopeButton(_ title: String) -> Bool {
        let predicate = NSPredicate(format: "label == %@", title)
        let buttons = app.buttons.matching(predicate).allElementsBoundByIndex
        guard let scopeButton = buttons.first(where: { button in
            isVisibleFrame(button.frame) && button.frame.midY < app.frame.midY
        }) else {
            return false
        }

        scopeButton.tap()
        return true
    }

    private func searchScopeTitle(_ rawValue: String) -> String {
        switch rawValue {
        case "all":
            return "All"
        case "sessions":
            return "Sessions"
        case "actions":
            return "Actions"
        case "settings":
            return "Settings"
        case "device":
            return "Device"
        default:
            XCTFail("Unhandled search scope: \(rawValue)")
            return rawValue
        }
    }

    private func control(named label: String) -> XCUIElement {
        let predicate = NSPredicate(format: "label == %@", label)
        return app.descendants(matching: .any).matching(predicate).firstMatch
    }

    private func scrollToElement(_ element: XCUIElement, maxSwipes: Int = 10) {
        if element.waitForExistence(timeout: 2), element.isHittable {
            return
        }

        for _ in 0..<maxSwipes {
            if element.exists, element.isHittable {
                return
            }

            if element.exists, element.frame.midY < app.frame.midY {
                app.swipeDown()
            } else {
                app.swipeUp()
            }
        }

        if element.exists, element.isHittable {
            return
        }

        XCTFail("Could not find hittable element: \(element)")
    }

    private func scrollToTop(maxSwipes: Int = 8) {
        for _ in 0..<maxSwipes {
            app.swipeDown()
        }
    }

    private func dismissKeyboard(identifier: String) {
        tapKeyboardDismissSafeArea()
        if !app.keyboards.firstMatch.exists {
            return
        }

        dismissKeyboardWithInteractiveScroll()
        if !app.keyboards.firstMatch.exists {
            return
        }

        let identifiedDoneButton = app.buttons[identifier].firstMatch
        if identifiedDoneButton.waitForExistence(timeout: ControlTapMetrics.keyboardFocusTimeout),
           tapIfFrameIsInsideApp(identifiedDoneButton)
        {
            return
        }

        let keyboardDoneButton = app.keyboards.buttons["Done"].firstMatch
        if keyboardDoneButton.waitForExistence(timeout: ControlTapMetrics.keyboardFocusTimeout),
           tapIfFrameIsInsideApp(keyboardDoneButton)
        {
            return
        }

        let keyboardDoneKey = app.keyboards.keys["Done"].firstMatch
        if keyboardDoneKey.waitForExistence(timeout: ControlTapMetrics.keyboardFocusTimeout),
           tapIfFrameIsInsideApp(keyboardDoneKey)
        {
            return
        }

        tapKeyboardDismissSafeArea()
    }

    private func dismissKeyboardWithInteractiveScroll() {
        for _ in 0..<ControlTapMetrics.keyboardDismissSwipeLimit {
            guard app.keyboards.firstMatch.exists else {
                return
            }
            app.swipeDown()
        }
    }

    private func tapKeyboardDismissSafeArea() {
        app.coordinate(
            withNormalizedOffset: CGVector(
                dx: ControlTapMetrics.keyboardDismissXFraction,
                dy: ControlTapMetrics.keyboardDismissYFraction
            )
        )
        .tap()
    }

    private func tapIfFrameIsInsideApp(_ element: XCUIElement) -> Bool {
        let frame = element.frame
        guard frame.width > .zero, frame.height > .zero, app.frame.contains(frame) else {
            return false
        }
        element.tap()
        return true
    }

    private func tapCenter(of element: XCUIElement) {
        scrollToElementFrame(element)
        let frame = element.frame
        let coordinate = app.coordinate(withNormalizedOffset: .zero).withOffset(
            CGVector(dx: frame.midX, dy: frame.midY)
        )
        coordinate.tap()
    }

    private func typeText(
        _ text: String,
        inTextView identifier: String
    ) {
        let textView = app.textViews[identifier].firstMatch
        scrollToElementFrame(textView)
        focusTextView(textView)
        textView.typeText(text)
    }

    private func focusTextView(_ textView: XCUIElement) {
        let frame = textView.frame
        let leadingTopCoordinate = app.coordinate(withNormalizedOffset: .zero).withOffset(
            CGVector(
                dx: frame.minX + ControlTapMetrics.textViewLeadingFocusInset,
                dy: frame.minY + ControlTapMetrics.textViewTopFocusInset
            )
        )
        leadingTopCoordinate.tap()

        if app.keyboards.firstMatch.waitForExistence(timeout: ControlTapMetrics.keyboardFocusTimeout) {
            return
        }

        textView.tap()
        _ = app.keyboards.firstMatch.waitForExistence(timeout: ControlTapMetrics.keyboardFocusTimeout)
    }

    private func pollForEnabledButton(_ label: String, timeout: TimeInterval) -> Bool {
        let button = app.buttons[label].firstMatch
        return pollForEnabledButton(button, timeout: timeout)
    }

    private func pollForEnabledButton(_ button: XCUIElement, timeout: TimeInterval) -> Bool {
        return pollUntil(timeout: timeout) {
            button.exists && button.isEnabled
        }
    }

    private func pollForButton(_ label: String, timeout: TimeInterval) -> Bool {
        let button = app.buttons[label].firstMatch
        return pollUntil(timeout: timeout) {
            button.exists
        }
    }

    private func pollForText(
        _ text: String,
        timeout: TimeInterval,
        allowsPartial: Bool = false
    ) -> Bool {
        let predicate: NSPredicate
        if allowsPartial {
            predicate = NSPredicate(format: "label CONTAINS[c] %@", text)
        } else {
            predicate = NSPredicate(format: "label == %@", text)
        }
        let element = app.descendants(matching: .any).matching(predicate).firstMatch
        return pollUntil(timeout: timeout) {
            element.exists
        }
    }

    private func pollForPromptEditorCleared(timeout: TimeInterval) -> Bool {
        let textView = app.textViews["session-detail.prompt-editor"].firstMatch
        return pollUntil(timeout: timeout) {
            !String(describing: textView.value ?? "").contains(LiveLatencyMetrics.promptText)
        }
    }

    private func pollUntil(timeout: TimeInterval, predicate: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if predicate() {
                return true
            }
            RunLoop.current.run(until: Date(timeIntervalSinceNow: LiveLatencyMetrics.pollInterval))
        } while Date() < deadline

        return predicate()
    }

    private func tapButtonInScrollView(identifier: String, maxSwipes: Int = 10) {
        let button = app.buttons[identifier].firstMatch
        let scrollView = app.scrollViews.firstMatch
        XCTAssertTrue(scrollView.waitForExistence(timeout: 4))

        for _ in 0..<maxSwipes {
            if button.waitForExistence(timeout: 1), isVisibleFrame(button.frame) {
                tapCenterWithoutScrolling(button)
                return
            }
            scrollView.swipeUp()
        }

        if button.exists, isVisibleFrame(button.frame) {
            tapCenterWithoutScrolling(button)
            return
        }

        XCTFail("Could not find visible button in scroll view: \(identifier)")
    }

    private func tapCenterWithoutScrolling(_ element: XCUIElement) {
        let frame = element.frame
        let coordinate = app.coordinate(withNormalizedOffset: .zero).withOffset(
            CGVector(dx: frame.midX, dy: frame.midY)
        )
        coordinate.tap()
    }

    private func tapSwitch(_ element: XCUIElement) {
        scrollToElementFrame(element)
        let frame = element.frame
        let switchXOffset = min(
            frame.width * ControlTapMetrics.trailingSwitchWidthFraction,
            ControlTapMetrics.maximumTrailingSwitchInset
        )
        let coordinate = app.coordinate(withNormalizedOffset: .zero).withOffset(
            CGVector(dx: frame.maxX - switchXOffset, dy: frame.midY)
        )
        coordinate.tap()
    }

    private var closeScannerButton: XCUIElement {
        let identifiedButton = app.buttons["scanner.close"].firstMatch
        if identifiedButton.exists {
            return identifiedButton
        }
        return app.buttons["Close scanner"].firstMatch
    }

    private func scrollToElementFrame(_ element: XCUIElement, maxSwipes: Int = 10) {
        if element.waitForExistence(timeout: 2), isVisibleFrame(element.frame) {
            return
        }

        for _ in 0..<maxSwipes {
            if element.exists, isVisibleFrame(element.frame) {
                return
            }

            if element.exists, element.frame.midY < app.frame.midY {
                app.swipeDown()
            } else {
                app.swipeUp()
            }
        }

        if element.exists, isVisibleFrame(element.frame) {
            return
        }

        XCTFail("Could not find visible element frame: \(element)")
    }

    private func isVisibleFrame(_ frame: CGRect) -> Bool {
        let appFrame = app.frame.insetBy(dx: 0, dy: 24)
        return isFiniteFrame(frame) && appFrame.intersects(frame) && frame.width > 0 && frame.height > 0
    }

    private func isFiniteFrame(_ frame: CGRect) -> Bool {
        frame.minX.isFinite &&
            frame.minY.isFinite &&
            frame.maxX.isFinite &&
            frame.maxY.isFinite
    }

    private func handleNotificationPromptIfNeeded() {
        handleSystemPromptIfNeeded(buttonLabels: ["Allow"])
    }

    private func handleSystemPromptIfNeeded(buttonLabels: [String]) {
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        for label in buttonLabels {
            let button = springboard.buttons[label]
            if button.waitForExistence(timeout: 2) {
                button.tap()
                return
            }
        }
    }
}
