import XCTest

final class RootRegistryTests: XCTestCase {
    func testInstallPersistsBoundedOpaqueRootWithoutURL() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
            "rstorrent-ios-registry-\(UUID().uuidString)",
            isDirectory: true
        )
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = RootRegistryStore(
            fileURL: directory.appendingPathComponent("roots.json")
        )
        let installed = try await store.install(
            bookmarkData: Data([1, 2, 3]),
            displayLabel: "Downloads"
        )
        let loaded = try await store.load()
        XCTAssertEqual(loaded.selectedRoots, [installed])
        XCTAssertTrue(installed.id.hasPrefix("ios-selected-"))
        XCTAssertFalse(String(data: try JSONEncoder().encode(loaded), encoding: .utf8)!.contains("file://"))
    }

    func testValidationRejectsDuplicateIDsAndOversizedBookmarks() {
        let record = SelectedRootRecord(
            schemaVersion: SelectedRootRecord.schemaVersion,
            id: "ios-selected-test",
            generation: 1,
            displayLabel: "Downloads",
            bookmarkData: Data([1]),
            lastEligibilityClass: .selectedOnDevice
        )
        XCTAssertThrowsError(
            try RootRegistryStore.validate(
                RootRegistryDocument(
                    schemaVersion: RootRegistryDocument.schemaVersion,
                    selectedRoots: [record, record]
                )
            )
        )
        var oversized = record
        oversized.id = "ios-selected-large"
        oversized.bookmarkData = Data(repeating: 0, count: 64 * 1024 + 1)
        XCTAssertThrowsError(
            try RootRegistryStore.validate(
                RootRegistryDocument(
                    schemaVersion: RootRegistryDocument.schemaVersion,
                    selectedRoots: [oversized]
                )
            )
        )
    }
}
