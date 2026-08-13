import XCTest
@testable import RSTorrent

final class IOSLifecycleStateTests: XCTestCase {
    func testGenerationAndFiniteOwnersAreIdempotent() {
        var state = IOSLifecycleState()
        XCTAssertTrue(state.beginGeneration())
        XCTAssertFalse(state.beginGeneration())
        state.engineReady()
        XCTAssertEqual(state.generation, 1)
        XCTAssertTrue(state.beginUIKitBackgroundAssertion())
        XCTAssertFalse(state.beginUIKitBackgroundAssertion())
        XCTAssertTrue(state.beginContinuedProcessing())
        XCTAssertFalse(state.beginContinuedProcessing())

        state.endUIKitBackgroundAssertion()
        state.endContinuedProcessing()
        state.beginStopping()
        state.engineStopped()
        XCTAssertFalse(state.hasUIKitBackgroundAssertion)
        XCTAssertFalse(state.hasContinuedProcessingTask)
        XCTAssertTrue(state.beginGeneration())
        XCTAssertEqual(state.generation, 2)
    }

    func testPendingInputIsSingleUseAndDeduplicated() {
        var state = IOSLifecycleState()
        XCTAssertEqual(state.stageInput(key: "first"), .staged)
        XCTAssertEqual(state.stageInput(key: "first"), .duplicate)
        XCTAssertEqual(state.stageInput(key: "second"), .occupied)
        state.finishPendingInput()
        XCTAssertNil(state.pendingInputKey)
        XCTAssertEqual(state.stageInput(key: "first"), .duplicate)
        XCTAssertEqual(state.stageInput(key: "second"), .staged)
    }

    func testHistoryAndHandledInputRetentionAreBounded() {
        var state = IOSLifecycleState()
        for index in 0..<(IOSLifecycleState.maximumHandledInputs + 20) {
            XCTAssertEqual(state.stageInput(key: "input-\(index)"), .staged)
            state.finishPendingInput()
        }
        XCTAssertEqual(state.handledInputKeys.count, IOSLifecycleState.maximumHandledInputs)
        XCTAssertEqual(state.history.count, IOSLifecycleState.maximumHistory)
        XCTAssertEqual(state.stageInput(key: "input-0"), .staged)
        XCTAssertEqual(state.stageInput(key: "input-20"), .duplicate)
    }
}
