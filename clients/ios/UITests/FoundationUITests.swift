import XCTest

final class FoundationUITests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    func testFoundationShowsReadyEngineAndFolderPickerEntry() {
        let app = XCUIApplication()
        app.launch()
        let ready = NSPredicate(format: "label == %@", "Ready")
        expectation(for: ready, evaluatedWith: app.staticTexts["engine-status"])
        waitForExpectations(timeout: 30)
        XCTAssertTrue(app.buttons["choose-folder"].exists)
        XCTAssertTrue(app.staticTexts["selected-root-status"].exists)
    }
}
