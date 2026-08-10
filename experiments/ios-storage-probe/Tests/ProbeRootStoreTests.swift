import XCTest

final class ProbeRootStoreTests: XCTestCase {
    private var suiteName = ""
    private var defaults: UserDefaults!
    private var store: ProbeRootStore!

    override func setUpWithError() throws {
        suiteName = "rstorrent-ios-root-tests-\(UUID().uuidString)"
        defaults = try XCTUnwrap(UserDefaults(suiteName: suiteName))
        defaults.removePersistentDomain(forName: suiteName)
        store = ProbeRootStore(defaults: defaults)
    }

    override func tearDownWithError() throws {
        defaults.removePersistentDomain(forName: suiteName)
        store = nil
        defaults = nil
    }

    func testAppOwnedIdentityIsStableAcrossReopen() throws {
        let first = try store.ensureAppOwned(displayLabel: "On My iPhone / RSTorrent")
        let reopened = ProbeRootStore(defaults: defaults)
        let second = try reopened.ensureAppOwned(displayLabel: "ignored replacement")
        XCTAssertEqual(first, second)
        XCTAssertEqual(first.generation, 1)
        XCTAssertNil(first.bookmarkData)
    }

    func testSelectedRepairKeepsIdentityAndAdvancesGeneration() throws {
        _ = try store.ensureAppOwned(displayLabel: "Local")
        let first = try store.installSelected(
            bookmarkData: Data([1, 2, 3]),
            displayLabel: "Selected"
        )
        let repaired = try store.installSelected(
            bookmarkData: Data([4, 5, 6]),
            displayLabel: "Repaired"
        )
        XCTAssertEqual(first.stableRootID, repaired.stableRootID)
        XCTAssertEqual(repaired.generation, first.generation + 1)
        XCTAssertEqual(repaired.bookmarkData, Data([4, 5, 6]))
        XCTAssertEqual(try store.load().roots.count, 2)
    }

    func testBookmarkAndLabelBoundsFailBeforePersistence() throws {
        _ = try store.ensureAppOwned(displayLabel: "Local")
        XCTAssertThrowsError(
            try store.installSelected(bookmarkData: Data(), displayLabel: "Selected")
        ) { error in
            XCTAssertEqual(error as? ProbeRootStoreError, .invalidBookmark)
        }
        XCTAssertThrowsError(
            try store.installSelected(
                bookmarkData: Data(repeating: 1, count: ProbeRootStore.maximumBookmarkBytes + 1),
                displayLabel: "Selected"
            )
        ) { error in
            XCTAssertEqual(error as? ProbeRootStoreError, .invalidBookmark)
        }
        XCTAssertThrowsError(
            try store.installSelected(
                bookmarkData: Data([1]),
                displayLabel: String(repeating: "x", count: ProbeRootStore.maximumDisplayLabelBytes + 1)
            )
        ) { error in
            XCTAssertEqual(error as? ProbeRootStoreError, .invalidDisplayLabel)
        }
        XCTAssertEqual(try store.load().roots.count, 1)
    }

    func testPendingCompletionIsGenerationFenced() throws {
        let root = try store.ensureAppOwned(displayLabel: "Local")
        try store.beginPendingOperation(for: root)
        XCTAssertFalse(
            try store.completePendingOperation(
                rootID: root.stableRootID,
                generation: root.generation + 1
            )
        )
        XCTAssertNotNil(try store.load().pendingOperation)
        XCTAssertTrue(
            try store.completePendingOperation(
                rootID: root.stableRootID,
                generation: root.generation
            )
        )
        XCTAssertNil(try store.load().pendingOperation)
    }

    func testCorruptAndOverfullRegistriesFailClosed() throws {
        let invalid = ProbeRootRegistry(
            schemaVersion: 99,
            roots: [],
            pendingOperation: nil
        )
        defaults.set(try JSONEncoder().encode(invalid), forKey: "probe.root-registry.v1")
        XCTAssertThrowsError(try store.load()) { error in
            XCTAssertEqual(error as? ProbeRootStoreError, .unsupportedSchema(99))
        }

        store.reset()
        let roots = (0 ... ProbeRootStore.maximumRoots).map { index in
            ProbeRootRecord(
                schemaVersion: ProbeRootStore.schemaVersion,
                stableRootID: UUID().uuidString,
                kind: index == 0 ? .appOwned : .selectedOnDevice,
                generation: 1,
                displayLabel: "root-\(index)",
                bookmarkData: index == 0 ? nil : Data([1]),
                lastEligibilityClass: index == 0 ? .appOwned : .selectedOnDevice
            )
        }
        XCTAssertThrowsError(
            try ProbeRootStore.validate(
                .init(schemaVersion: 1, roots: roots, pendingOperation: nil)
            )
        ) { error in
            XCTAssertEqual(
                error as? ProbeRootStoreError,
                .tooManyRoots(ProbeRootStore.maximumRoots + 1)
            )
        }
    }
}
