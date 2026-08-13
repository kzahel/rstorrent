import XCTest

final class RootEligibilityTests: XCTestCase {
    func testAcceptsQualifiedOnDeviceFolderWhenProviderLookupFails() {
        XCTAssertEqual(
            decide(provider: .failed),
            RootEligibilityDecision(
                classification: .selectedOnDevice,
                reason: .acceptedSelectedOnDevice
            )
        )
    }

    func testRejectsUbiquitousAndIdentifiedProviderFolders() {
        XCTAssertEqual(decide(ubiquitous: true).reason, .ubiquitous)
        XCTAssertEqual(decide(provider: .identified).reason, .providerIdentified)
    }

    func testRequiresPositiveLocalInternalAndOverlapEvidence() {
        XCTAssertEqual(decide(local: nil).reason, .missingEvidence)
        XCTAssertEqual(decide(internalVolume: nil).reason, .missingEvidence)
        XCTAssertEqual(decide(overlaps: nil).reason, .overlapsRegisteredRoot)
        XCTAssertEqual(decide(overlaps: true).reason, .overlapsRegisteredRoot)
    }

    private func decide(
        overlaps: Bool? = false,
        ubiquitous: Bool? = false,
        local: Bool? = true,
        internalVolume: Bool? = true,
        provider: RootProviderLookup = .noIdentifier
    ) -> RootEligibilityDecision {
        RootEligibility.decide(
            RootEligibilityObservation(
                isFileURL: true,
                isDirectory: true,
                isSymbolicLink: false,
                overlapsRegisteredRoot: overlaps,
                isUbiquitousItem: ubiquitous,
                volumeIsLocal: local,
                volumeIsInternal: internalVolume,
                fileProviderLookup: provider
            )
        )
    }
}
