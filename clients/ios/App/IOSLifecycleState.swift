import Foundation

enum IOSLifecyclePhase: String, Equatable {
    case cold
    case starting
    case foreground
    case inactive
    case background
    case stopping
    case stopped
}

enum IOSPendingInputDecision: Equatable {
    case staged
    case duplicate
    case occupied
}

struct IOSLifecycleState: Equatable {
    static let maximumHistory = 64
    static let maximumHandledInputs = 64

    private(set) var generation: UInt64 = 0
    private(set) var phase: IOSLifecyclePhase = .cold
    private(set) var pendingInputKey: String?
    private(set) var handledInputKeys: [String] = []
    private(set) var hasUIKitBackgroundAssertion = false
    private(set) var hasContinuedProcessingTask = false
    private(set) var history: [String] = []

    mutating func beginGeneration() -> Bool {
        guard phase != .starting && phase != .stopping else { return false }
        guard generation < UInt64.max else { return false }
        generation += 1
        phase = .starting
        record("generation-started")
        return true
    }

    mutating func engineReady(in scenePhase: IOSLifecyclePhase = .foreground) {
        phase = scenePhase
        record("engine-ready")
    }

    mutating func scene(_ phase: IOSLifecyclePhase) {
        self.phase = phase
        record("scene-\(phase.rawValue)")
    }

    mutating func beginUIKitBackgroundAssertion() -> Bool {
        guard !hasUIKitBackgroundAssertion else { return false }
        hasUIKitBackgroundAssertion = true
        record("uikit-background-began")
        return true
    }

    mutating func endUIKitBackgroundAssertion() {
        guard hasUIKitBackgroundAssertion else { return }
        hasUIKitBackgroundAssertion = false
        record("uikit-background-ended")
    }

    mutating func beginContinuedProcessing() -> Bool {
        guard !hasContinuedProcessingTask else { return false }
        hasContinuedProcessingTask = true
        record("continued-processing-began")
        return true
    }

    mutating func endContinuedProcessing() {
        guard hasContinuedProcessingTask else { return }
        hasContinuedProcessingTask = false
        record("continued-processing-ended")
    }

    mutating func stageInput(key: String) -> IOSPendingInputDecision {
        if pendingInputKey == key || handledInputKeys.contains(key) {
            return .duplicate
        }
        guard pendingInputKey == nil else { return .occupied }
        pendingInputKey = key
        record("input-staged")
        return .staged
    }

    mutating func finishPendingInput() {
        guard let key = pendingInputKey else { return }
        pendingInputKey = nil
        handledInputKeys.append(key)
        if handledInputKeys.count > Self.maximumHandledInputs {
            handledInputKeys.removeFirst(handledInputKeys.count - Self.maximumHandledInputs)
        }
        record("input-finished")
    }

    mutating func beginStopping() {
        phase = .stopping
        record("engine-stopping")
    }

    mutating func engineStopped() {
        phase = .stopped
        hasUIKitBackgroundAssertion = false
        hasContinuedProcessingTask = false
        record("engine-stopped")
    }

    private mutating func record(_ event: String) {
        history.append(event)
        if history.count > Self.maximumHistory {
            history.removeFirst(history.count - Self.maximumHistory)
        }
    }
}
