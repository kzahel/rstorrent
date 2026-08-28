import RSTorrentSession
import XCTest
@testable import RSTorrent

@MainActor
final class IOSPresentationRepositoryTests: XCTestCase {
    func testCurrentRatesReplaceWithoutHistory() throws {
        let repository = IOSPresentationRepository()
        try repository.apply(
            update(
                sequence: "1",
                baseRevision: "0",
                revision: "1",
                payload: .snapshot(
                    snapshot: .sessionCurrentRates(
                        rates: SessionCurrentRatesView(
                            capturedMillis: "1000",
                            rates: [SpeedCurrentRate(metric: .payloadReceived, bytes: "10")]
                        )
                    )
                )
            )
        )
        try repository.apply(
            update(
                sequence: "2",
                baseRevision: "1",
                revision: "2",
                payload: .patch(
                    patch: .sessionCurrentRates(
                        rates: SessionCurrentRatesView(
                            capturedMillis: "1100",
                            rates: [SpeedCurrentRate(metric: .payloadReceived, bytes: "25")]
                        )
                    )
                )
            )
        )

        XCTAssertEqual(repository.currentRates?.capturedMillis, "1100")
        XCTAssertEqual(repository.currentRates?.rates.single?.bytes, "25")
        XCTAssertNil(repository.speed)
    }

    func testSpeedHistoryAppendPreservesCoalescedBucketsAndRejectsGaps() throws {
        let repository = IOSPresentationRepository()
        try repository.apply(
            update(
                sequence: "1",
                baseRevision: "0",
                revision: "1",
                payload: .snapshot(snapshot: .sessionSpeedHistory(history: speedHistory()))
            )
        )
        try repository.apply(
            update(
                sequence: "2",
                baseRevision: "1",
                revision: "2",
                payload: .patch(patch: .sessionSpeedHistory(append: speedAppend()))
            )
        )

        XCTAssertEqual(repository.speed?.series.single?.values, ["20", "30", "30", nil])
        XCTAssertEqual(repository.speed?.completeThroughMillis, "400")

        var gap = speedAppend()
        gap.baseCompleteThroughMillis = "100"
        XCTAssertThrowsError(
            try repository.apply(
                update(
                    sequence: "3",
                    baseRevision: "2",
                    revision: "3",
                    payload: .patch(patch: .sessionSpeedHistory(append: gap))
                )
            )
        )
    }

    private func update(
        sequence: String,
        baseRevision: String,
        revision: String,
        payload: ViewUpdatePayload
    ) -> ViewUpdate {
        ViewUpdate(
            contractVersion: 2,
            streamId: "speed",
            epoch: "epoch-1",
            sequence: sequence,
            baseRevision: baseRevision,
            revision: revision,
            payload: payload
        )
    }

    private func speedHistory() -> SpeedHistoryView {
        SpeedHistoryView(
            capturedMillis: "250",
            historyEpoch: "history-1",
            range: .seconds30,
            bucketMillis: "100",
            startMillis: "0",
            completeThroughMillis: "200",
            live: true,
            persistence: .healthy,
            series: [
                SpeedSeriesView(
                    metric: .payloadReceived,
                    values: ["10", nil, "20", "30"]
                )
            ],
            catalog: [
                SpeedMetricAvailability(metric: .payloadReceived, available: true, reason: nil)
            ]
        )
    }

    private func speedAppend() -> SpeedHistoryAppend {
        SpeedHistoryAppend(
            capturedMillis: "400",
            historyEpoch: "history-1",
            baseCompleteThroughMillis: "200",
            startMillis: "100",
            completeThroughMillis: "400",
            persistence: nil,
            series: [
                SpeedSeriesAppend(metric: .payloadReceived, values: ["30", nil])
            ]
        )
    }
}

private extension Array {
    var single: Element? { count == 1 ? first : nil }
}
