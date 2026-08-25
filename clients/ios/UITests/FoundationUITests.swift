import XCTest

final class ProductSurfaceUITests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    func testLibraryAddAndSettingsNavigation() {
        let app = XCUIApplication()
        app.launchArguments.append("--ui-testing")
        app.launch()
        waitUntilReady(app)

        XCTAssertTrue(app.staticTexts["RSTorrent"].exists)
        XCTAssertTrue(app.staticTexts["No torrents"].exists)

        app.buttons["Add torrent"].tap()
        XCTAssertTrue(app.navigationBars["Add Torrent"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.textFields["magnet:?xt=urn:btih:…"].exists)
        app.swipeUp()
        XCTAssertTrue(app.buttons["Browse for .torrent file"].waitForExistence(timeout: 5))
        app.buttons["Cancel"].tap()

        app.buttons["Settings"].tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Choose folder"].exists)
        XCTAssertTrue(app.staticTexts["RSTorrent Documents"].exists)
        XCTAssertFalse(app.switches["Prevent sleep while downloading"].exists)
    }

    func testDarkAccessibilityTextAndLandscapeRemainNavigable() {
        let app = XCUIApplication()
        app.launchArguments += [
            "--ui-testing",
            "-AppleInterfaceStyle", "Dark",
            "-UIPreferredContentSizeCategoryName",
            "UICTContentSizeCategoryAccessibilityExtraExtraExtraLarge",
        ]
        app.launch()
        waitUntilReady(app)

        XCUIDevice.shared.orientation = .landscapeLeft
        XCTAssertTrue(app.buttons["Add torrent"].waitForExistence(timeout: 5))
        app.buttons["Add torrent"].tap()
        XCTAssertTrue(app.navigationBars["Add Torrent"].waitForExistence(timeout: 5))
        app.buttons["Cancel"].tap()

        XCUIDevice.shared.orientation = .portrait
        XCTAssertTrue(app.buttons["Settings"].waitForExistence(timeout: 5))
        app.buttons["Settings"].tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
    }

    private func waitUntilReady(_ app: XCUIApplication) {
        let ready = NSPredicate(format: "label == %@", "Ready")
        expectation(for: ready, evaluatedWith: app.staticTexts["Ready"])
        waitForExpectations(timeout: 30)
    }
}
