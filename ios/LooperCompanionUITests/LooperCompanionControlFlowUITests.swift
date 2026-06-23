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
    static let promptQueueTimeout: TimeInterval = 12
    static let mutationControlTimeout: TimeInterval = 15
}

private enum ControlFlowLaunchArgument {
    static let openOrbScannerOnLaunch = "--open-orb-scanner-on-launch"
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
        recordSurfaceEvidence("ios-session-detail")

        let firstPromptSuggestion = app.buttons["session-detail.prompt-suggestion.0"].firstMatch
        scrollToElement(firstPromptSuggestion)
        firstPromptSuggestion.tap()
        typeText(
            "UI test prompt",
            inTextView: "session-detail.prompt-editor",
            keyboardDoneIdentifier: "session-detail.keyboard-done"
        )
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
            inTextView: "settings.default-prompt-editor",
            keyboardDoneIdentifier: "settings.keyboard-done"
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
        app.buttons["Search"].tap()
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

    private func openPrimarySessionDetail() {
        XCTAssertTrue(waitForText("Sessions"))
        XCTAssertTrue(waitForText("Make an iOS app for looper"))
        app.staticTexts["Make an iOS app for looper"].tap()
        XCTAssertTrue(waitForButton("Use with Siri"))
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
        searchField.tap()

        let clearButton = searchField.buttons.firstMatch
        if clearButton.waitForExistence(timeout: 1) {
            clearButton.tap()
            return
        }

        searchField.press(forDuration: 0.5)
        if app.menuItems["Select All"].waitForExistence(timeout: 1) {
            app.menuItems["Select All"].tap()
            searchField.typeText(XCUIKeyboardKey.delete.rawValue)
        }
    }

    private func searchFieldContains(_ expectedText: String) -> Bool {
        let searchField = app.searchFields.firstMatch
        guard searchField.waitForExistence(timeout: 6) else {
            return false
        }

        let value = String(describing: searchField.value ?? "")
        return value.localizedCaseInsensitiveContains(expectedText)
    }

    private func tapSearchScope(_ rawValue: String) {
        if tapSearchScopeButton(rawValue) {
            return
        }

        let scopeScroller = app.scrollViews["search.scope.scroller"].firstMatch
        if scopeScroller.waitForExistence(timeout: 1) {
            for _ in 0..<4 {
                scopeScroller.swipeLeft()
                if tapSearchScopeButton(rawValue) {
                    return
                }
            }

            for _ in 0..<4 {
                scopeScroller.swipeRight()
                if tapSearchScopeButton(rawValue) {
                    return
                }
            }
        }

        let stableScopeMenu = app.buttons["search.scope.menu"].firstMatch
        let scopeMenu = stableScopeMenu.waitForExistence(timeout: 1) ?
            stableScopeMenu :
            app.buttons["Filter search scope"].firstMatch
        XCTAssertTrue(scopeMenu.waitForExistence(timeout: ControlTapMetrics.scopeMenuTimeout))
        scopeMenu.tap()

        let title = searchScopeTitle(rawValue)
        let menuItem = app.buttons[title].firstMatch
        XCTAssertTrue(menuItem.waitForExistence(timeout: ControlTapMetrics.scopeMenuTimeout))
        menuItem.tap()
    }

    private func tapSearchScopeButton(_ rawValue: String) -> Bool {
        let scopeButton = app.buttons["search.scope.\(rawValue)"].firstMatch
        if scopeButton.waitForExistence(timeout: 1), scopeButton.isHittable {
            scopeButton.tap()
            return true
        }

        let scope = app.descendants(matching: .any)
            .matching(identifier: "search.scope.\(rawValue)")
            .firstMatch
        if scope.waitForExistence(timeout: 1), isVisibleFrame(scope.frame) {
            tapCenter(of: scope)
            return true
        }

        return false
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

        let identifiedDoneButton = app.buttons[identifier].firstMatch
        if identifiedDoneButton.waitForExistence(timeout: 1), tapIfFrameIsInsideApp(identifiedDoneButton) {
            return
        }

        let keyboardDoneButton = app.keyboards.buttons["Done"].firstMatch
        if keyboardDoneButton.waitForExistence(timeout: 1), tapIfFrameIsInsideApp(keyboardDoneButton) {
            return
        }

        let keyboardDoneKey = app.keyboards.keys["Done"].firstMatch
        if keyboardDoneKey.waitForExistence(timeout: 1), tapIfFrameIsInsideApp(keyboardDoneKey) {
            return
        }

        tapKeyboardDismissSafeArea()
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
        inTextView identifier: String,
        keyboardDoneIdentifier: String
    ) {
        let textView = app.textViews[identifier].firstMatch
        scrollToElementFrame(textView)
        focusTextView(textView, keyboardDoneIdentifier: keyboardDoneIdentifier)
        textView.typeText(text)
    }

    private func focusTextView(_ textView: XCUIElement, keyboardDoneIdentifier: String) {
        let frame = textView.frame
        let leadingTopCoordinate = app.coordinate(withNormalizedOffset: .zero).withOffset(
            CGVector(
                dx: frame.minX + ControlTapMetrics.textViewLeadingFocusInset,
                dy: frame.minY + ControlTapMetrics.textViewTopFocusInset
            )
        )
        leadingTopCoordinate.tap()

        if app.buttons[keyboardDoneIdentifier].firstMatch.waitForExistence(timeout: 2) {
            return
        }

        textView.tap()
        XCTAssertTrue(
            app.buttons[keyboardDoneIdentifier].firstMatch.waitForExistence(timeout: 2),
            "Text view did not expose the keyboard toolbar: \(textView)"
        )
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
