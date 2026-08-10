import XCTest

final class RootEligibilityTests: XCTestCase {
    func testSelectedRootRegistrationRemainsDisabledWithoutPhysicalControls() {
        XCTAssertFalse(ProbeRootPolicy.selectedRootRegistrationEnabled)
    }

    func testAppOwnedRequiresDirectoryAndRejectsSymbolicLink() {
        XCTAssertEqual(
            decide(.appOwned, directory: true, symbolicLink: false),
            .init(classification: .appOwned, reason: .acceptedAppOwned)
        )
        XCTAssertEqual(
            decide(.appOwned, directory: false, symbolicLink: false).reason,
            .wrongKind
        )
        XCTAssertEqual(
            decide(.appOwned, directory: true, symbolicLink: true).reason,
            .symbolicLink
        )
    }

    func testPickerAcceptsOnlyCompleteLocalNonProviderEvidence() {
        let accepted = ProbeRootEligibility.decide(
            provenance: .picker,
            observation: .init(
                isFileURL: true,
                isDirectory: true,
                isSymbolicLink: false,
                overlapsAppOwnedRoot: false,
                isUbiquitousItem: false,
                volumeIsLocal: true,
                volumeIsInternal: true,
                fileProviderLookup: .noIdentifier
            )
        )
        XCTAssertEqual(
            accepted,
            .init(
                classification: .selectedOnDevice,
                reason: .acceptedSelectedOnDevice
            )
        )

        for observation in [
            ProbeRootEligibilityObservation(
                isFileURL: true,
                isDirectory: true,
                isSymbolicLink: false,
                overlapsAppOwnedRoot: false,
                isUbiquitousItem: nil,
                volumeIsLocal: true,
                volumeIsInternal: true,
                fileProviderLookup: .noIdentifier
            ),
            ProbeRootEligibilityObservation(
                isFileURL: true,
                isDirectory: true,
                isSymbolicLink: false,
                overlapsAppOwnedRoot: false,
                isUbiquitousItem: false,
                volumeIsLocal: nil,
                volumeIsInternal: true,
                fileProviderLookup: .noIdentifier
            ),
            ProbeRootEligibilityObservation(
                isFileURL: true,
                isDirectory: true,
                isSymbolicLink: false,
                overlapsAppOwnedRoot: false,
                isUbiquitousItem: false,
                volumeIsLocal: true,
                volumeIsInternal: nil,
                fileProviderLookup: .noIdentifier
            ),
        ] {
            XCTAssertEqual(
                ProbeRootEligibility.decide(provenance: .picker, observation: observation)
                    .classification,
                .unclassifiable
            )
        }
    }

    func testRemoteAndAmbiguousProviderSignalsFailClosed() {
        XCTAssertEqual(
            decide(.picker, ubiquitous: true).classification,
            .unsupportedProvider
        )
        XCTAssertEqual(
            decide(.picker, local: false).classification,
            .unsupportedProvider
        )
        XCTAssertEqual(
            decide(.picker, internalVolume: false).classification,
            .unsupportedProvider
        )
        XCTAssertEqual(
            decide(.picker, provider: .identified).reason,
            .providerIdentified
        )
        XCTAssertEqual(
            decide(.picker, provider: .failed).reason,
            .providerLookupFailed
        )
        XCTAssertEqual(
            decide(.picker, provider: .timedOut).reason,
            .providerLookupTimedOut
        )
        XCTAssertEqual(decide(.picker, provider: .notQueried).reason, .missingEvidence)
        XCTAssertEqual(decide(.picker, fileURL: false).reason, .wrongScheme)
        XCTAssertEqual(
            decide(.picker, overlapsAppOwned: true).reason,
            .overlapsAppOwnedRoot
        )
    }

    func testEncodedObservationContainsNoProviderIdentifierField() throws {
        let observation = ProbeRootEligibilityObservation(
            isFileURL: true,
            isDirectory: true,
            isSymbolicLink: false,
            overlapsAppOwnedRoot: false,
            isUbiquitousItem: false,
            volumeIsLocal: true,
            volumeIsInternal: true,
            fileProviderLookup: .identified
        )
        let encoded = try JSONEncoder().encode(observation)
        let text = try XCTUnwrap(String(data: encoded, encoding: .utf8))
        XCTAssertFalse(text.contains("itemIdentifier"))
        XCTAssertFalse(text.contains("domainIdentifier"))
        XCTAssertFalse(text.contains("secret-provider-value"))
    }

    func testUnknownFutureProviderValueFailsDecode() {
        let json = #"{"isDirectory":true,"isSymbolicLink":false,"isUbiquitousItem":false,"volumeIsLocal":true,"volumeIsInternal":true,"fileProviderLookup":"future"}"#
        XCTAssertThrowsError(
            try JSONDecoder().decode(
                ProbeRootEligibilityObservation.self,
                from: Data(json.utf8)
            )
        )
    }

    private func decide(
        _ provenance: ProbeRootProvenance,
        fileURL: Bool? = true,
        directory: Bool? = true,
        symbolicLink: Bool? = false,
        overlapsAppOwned: Bool? = false,
        ubiquitous: Bool? = false,
        local: Bool? = true,
        internalVolume: Bool? = true,
        provider: ProbeFileProviderLookup = .noIdentifier
    ) -> ProbeRootEligibilityDecision {
        ProbeRootEligibility.decide(
            provenance: provenance,
            observation: .init(
                isFileURL: fileURL,
                isDirectory: directory,
                isSymbolicLink: symbolicLink,
                overlapsAppOwnedRoot: overlapsAppOwned,
                isUbiquitousItem: ubiquitous,
                volumeIsLocal: local,
                volumeIsInternal: internalVolume,
                fileProviderLookup: provider
            )
        )
    }
}
