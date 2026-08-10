import XCTest

final class RSTorrentIOSStorageProbeUITests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    func testAppOwnedNetworkBackgroundAndForceCloseRecovery() throws {
        let app = XCUIApplication()
        copyControlledEndpointEnvironment(to: app)
        app.launch()
        acceptLocalNetworkPromptIfPresent(app: app)

        XCTAssertTrue(waitForPrefix("pass", in: app.staticTexts["app-owned-status"], timeout: 30))
        XCTAssertTrue(waitForPrefix("pass", in: app.staticTexts["network-status"], timeout: 30))

        app.buttons["start-continued"].tap()
        XCTAssertTrue(waitForSubstring("continued=submitted", in: app.staticTexts["lifecycle-status"], timeout: 10))
        XCUIDevice.shared.press(.home)
        sleep(12)
        app.activate()
        XCTAssertTrue(waitForSubstring("continued=completed", in: app.staticTexts["lifecycle-status"], timeout: 15))

        app.buttons["prepare-app-interruption"].tap()
        XCTAssertTrue(
            waitForSubstring(
                "forceArmed=true",
                in: app.staticTexts["lifecycle-status"],
                timeout: 15
            )
        )
        app.terminate()
        app.launch()
        XCTAssertTrue(
            waitForSubstring(
                "forceRecovered=true",
                in: app.staticTexts["lifecycle-status"],
                timeout: 30
            )
        )
    }

    func testClassifiesOverlappingPickerRootWithoutPersistence() throws {
        let app = XCUIApplication()
        app.launch()
        app.buttons["choose-folder"].tap()

        let pickerRoot = app.staticTexts["PickerRoot"]
        if !pickerRoot.waitForExistence(timeout: 5) {
            let browse = app.buttons["Browse"]
            if browse.exists { browse.tap() }
            let local = app.staticTexts["On My iPhone"]
            if local.waitForExistence(timeout: 5) { local.tap() }
            let appFolder = app.staticTexts["RSTorrent Probe"]
            if appFolder.waitForExistence(timeout: 5) { appFolder.tap() }
        }
        XCTAssertTrue(pickerRoot.waitForExistence(timeout: 10))
        pickerRoot.tap()
        let open = app.buttons["Open"]
        if open.waitForExistence(timeout: 5) { open.tap() }

        XCTAssertTrue(
            waitForPrefix(
                "classification-only",
                in: app.staticTexts["selected-status"],
                timeout: 30
            )
        )
        app.terminate()
        app.launch()
        XCTAssertTrue(
            waitForPrefix(
                "disabled app-owned-only",
                in: app.staticTexts["selected-status"],
                timeout: 30
            )
        )
    }

    private func copyControlledEndpointEnvironment(to app: XCUIApplication) {
        for key in ["RSTORRENT_PROBE_HOST", "RSTORRENT_PROBE_TCP_PORT", "RSTORRENT_PROBE_UDP_PORT"] {
            if let value = ProcessInfo.processInfo.environment[key] {
                app.launchEnvironment[key] = value
            }
        }
    }

    private func acceptLocalNetworkPromptIfPresent(app: XCUIApplication) {
        addUIInterruptionMonitor(withDescription: "Local network") { alert in
            let allow = alert.buttons["Allow"]
            if allow.exists {
                allow.tap()
                return true
            }
            return false
        }
        app.tap()
    }

    private func waitForPrefix(_ prefix: String, in element: XCUIElement, timeout: TimeInterval) -> Bool {
        let predicate = NSPredicate(format: "label BEGINSWITH %@", prefix)
        let expectation = XCTNSPredicateExpectation(predicate: predicate, object: element)
        return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
    }

    private func waitForSubstring(_ substring: String, in element: XCUIElement, timeout: TimeInterval) -> Bool {
        let predicate = NSPredicate(format: "label CONTAINS %@", substring)
        let expectation = XCTNSPredicateExpectation(predicate: predicate, object: element)
        return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
    }
}
