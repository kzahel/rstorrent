import BackgroundTasks
import SwiftUI
import UIKit

private let continuedTaskIdentifier = "com.kgraehl.rstorrent.storage-probe.continued"

final class ProbeAppDelegate: NSObject, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        if #available(iOS 26.0, *) {
            let registered = BGTaskScheduler.shared.register(
                forTaskWithIdentifier: continuedTaskIdentifier,
                using: nil
            ) { task in
                guard let continued = task as? BGContinuedProcessingTask else {
                    task.setTaskCompleted(success: false)
                    return
                }
                ContinuedProbe.run(task: continued)
            }
            ProbeDefaults.set("continued_registered", registered ? "true" : "false")
        }
        return true
    }
}

@main
struct RSTorrentIOSStorageProbeApp: App {
    @UIApplicationDelegateAdaptor(ProbeAppDelegate.self) private var appDelegate
    @Environment(\.scenePhase) private var scenePhase
    @StateObject private var model = ProbeModel()

    var body: some Scene {
        WindowGroup {
            ProbeView(model: model)
        }
        .onChange(of: scenePhase) { _, phase in
            model.record(phase: phase)
        }
    }
}

@available(iOS 26.0, *)
private enum ContinuedProbe {
    static func run(task: BGContinuedProcessingTask) {
        let state = ExpirationState()
        task.progress.totalUnitCount = 20
        task.expirationHandler = {
            state.expire()
            ProbeDefaults.set("continued_result", "expired")
        }
        DispatchQueue.global(qos: .userInitiated).async {
            let completedStorage: Bool
            do {
                let (_, storage) = try ProbeRootAccess.runAppOwned(at: ProbePaths.documents)
                completedStorage = storage.ok
            } catch {
                completedStorage = false
            }
            ProbeDefaults.set("resources", ProbeResourceLedger.shared.evidence())
            var completed = completedStorage
            for step in 1 ... 20 {
                if state.isExpired {
                    completed = false
                    break
                }
                task.progress.completedUnitCount = Int64(step)
                task.updateTitle(
                    "RSTorrent finite check",
                    subtitle: "Verified step \(step) of 20"
                )
                Thread.sleep(forTimeInterval: 0.5)
            }
            ProbeDefaults.set(
                "continued_result",
                completed ? "completed" : state.isExpired ? "expired" : "failed"
            )
            task.setTaskCompleted(success: completed)
        }
    }
}

private final class ExpirationState: @unchecked Sendable {
    private let lock = NSLock()
    private var expired = false

    var isExpired: Bool {
        lock.withLock { expired }
    }

    func expire() {
        lock.withLock { expired = true }
    }
}
