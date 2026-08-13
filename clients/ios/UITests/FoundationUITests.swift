import XCTest

final class ProductSurfaceUITests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    func testLibraryAddAndSettingsNavigation() {
        let app = XCUIApplication()
        app.launch()
        let ready = NSPredicate(format: "label == %@", "Ready")
        expectation(for: ready, evaluatedWith: app.staticTexts["Ready"])
        waitForExpectations(timeout: 30)

        XCTAssertTrue(app.staticTexts["RSTorrent"].exists)
        XCTAssertTrue(app.staticTexts["No torrents"].exists)

        app.buttons["Add torrent"].tap()
        XCTAssertTrue(app.navigationBars["Add Torrent"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.textFields["magnet:?xt=urn:btih:…"].exists)
        app.buttons["Cancel"].tap()

        app.buttons["Settings"].tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Choose folder"].exists)
        XCTAssertTrue(app.staticTexts["RSTorrent Documents"].exists)
    }
}
