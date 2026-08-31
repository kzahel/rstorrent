import XCTest
@testable import RSTorrent

final class TorrentPresentationTests: XCTestCase {
    func testPeerCountUsesEnglishPluralBranches() {
        XCTAssertEqual(localizedPeerCount(1), "1 peer")
        XCTAssertEqual(localizedPeerCount(2), "2 peers")
    }

    func testDownloadingWithAllReportedBytesNeverDisplaysComplete() {
        let progress = torrentDisplayProgress(
            state: .downloading,
            storageState: .available,
            requiredPayloadBytes: "276445467",
            remainingPayloadBytes: "0",
            pieceCount: 1_055,
            verifiedPieceCount: 1_055
        )

        XCTAssertEqual(progress, 0.99, accuracy: 0.000_001)
        XCTAssertEqual(formattedProgress(progress), "99%")
    }

    func testCompleteStateDisplaysOneHundredPercent() {
        let progress = torrentDisplayProgress(
            state: .complete,
            storageState: .available,
            requiredPayloadBytes: "100",
            remainingPayloadBytes: "0",
            pieceCount: 10,
            verifiedPieceCount: 4
        )

        XCTAssertEqual(progress, 1)
        XCTAssertEqual(formattedProgress(progress), "100%")
    }

    func testRootUnavailableStateRemainsIncomplete() {
        let progress = torrentDisplayProgress(
            state: .awaitingStorage,
            storageState: .unavailable,
            requiredPayloadBytes: "100",
            remainingPayloadBytes: "0",
            pieceCount: 1,
            verifiedPieceCount: 1
        )

        XCTAssertEqual(progress, 0.99, accuracy: 0.000_001)
        XCTAssertEqual(formattedProgress(progress), "99%")
    }

    func testRoundingCannotTurnNonterminalFractionIntoOneHundredPercent() {
        XCTAssertEqual(formattedProgress(0.999_999), "99%")
        XCTAssertEqual(formattedProgress(0.994), "99%")
        XCTAssertEqual(formattedProgress(0.419), "42%")
    }

    func testMalformedByteCountsFallBackToVerifiedPieces() {
        let progress = torrentDisplayProgress(
            state: .downloading,
            storageState: .available,
            requiredPayloadBytes: "not-a-number",
            remainingPayloadBytes: "0",
            pieceCount: 8,
            verifiedPieceCount: 3
        )

        XCTAssertEqual(progress, 0.375, accuracy: 0.000_001)
    }

    func testInvalidAndOutOfRangeFractionsStayBounded() {
        XCTAssertEqual(
            torrentDisplayProgress(
                state: .downloading,
                storageState: .available,
                requiredPayloadBytes: "100",
                remainingPayloadBytes: "101",
                pieceCount: 0,
                verifiedPieceCount: 0
            ),
            0
        )
        XCTAssertEqual(
            torrentDisplayProgress(
                state: .downloading,
                storageState: .available,
                requiredPayloadBytes: "nan",
                remainingPayloadBytes: "0",
                pieceCount: 0,
                verifiedPieceCount: 0
            ),
            0
        )
        XCTAssertEqual(formattedProgress(.nan), "0%")
    }
}
