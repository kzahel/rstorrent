import BackgroundTasks
import Foundation
import RSTorrentSession
import SwiftUI
import UIKit
import UserNotifications

private enum IOSExternalInput {
    case magnet(String)
    case torrentFile(URL)

    var key: String {
        switch self {
        case .magnet(let value): return "magnet:\(value)"
        case .torrentFile(let url): return "file:\(url.standardizedFileURL.absoluteString)"
        }
    }
}

@MainActor
final class IOSApplicationLifecycleOwner: ObservableObject {
    static let continuedTaskPrefix = "org.rstorrent.ios.dev.continued-processing"
    static let continuedTaskWildcard = "\(continuedTaskPrefix).*"
    private static let maximumMagnetBytes = 16 * 1024

    let model: AppModel
    @Published private(set) var state = IOSLifecycleState()
    @Published private(set) var notificationsEnabled = false
    @Published private(set) var backgroundStatus = "Foreground service"

    private var starting = false
    private var observedScenePhase: IOSLifecyclePhase = .foreground
    private var workGraceDeadline: Date?
    private var pendingInput: IOSExternalInput?
    private var backgroundAssertion: UIBackgroundTaskIdentifier = .invalid
    private var continuedRequestIdentifier: String?
    private var continuedTaskLease: AnyObject?
    private var monitorTask: Task<Void, Never>?
    private let notifications = IOSNotificationCoordinator()

    convenience init() {
        self.init(model: AppModel())
    }

    init(model: AppModel) {
        self.model = model
        model.userWorkDidStart = { [weak self] in self?.userStartedWork() }
        IOSBackgroundTaskBridge.shared.attach(self)
        IOSExternalInputBridge.shared.attach(self)
        Task { [weak self] in
            guard let self else { return }
            notificationsEnabled = await notifications.isAuthorized()
        }
    }

    func start() async {
        guard !starting else { return }
        if model.isReady {
            await drainPendingInput()
            return
        }
        guard state.beginGeneration() else { return }
        starting = true
        await model.start()
        starting = false
        if model.isReady {
            state.engineReady(in: observedScenePhase)
            await drainPendingInput()
        } else {
            state.engineStopped()
        }
    }

    func scenePhaseDidChange(_ scenePhase: ScenePhase) {
        switch scenePhase {
        case .active:
            observedScenePhase = .foreground
            state.scene(.foreground)
            let hadFiniteOpportunity = backgroundAssertion != .invalid
                || continuedRequestIdentifier != nil
                || continuedTaskLease != nil
            finishFiniteOpportunity(success: true, notify: false)
            if !hadFiniteOpportunity {
                backgroundStatus = "Foreground service"
            }
            if !model.isReady { Task { await start() } }
        case .inactive:
            observedScenePhase = .inactive
            state.scene(.inactive)
        case .background:
            observedScenePhase = .background
            state.scene(.background)
            guard currentWork().active else {
                backgroundStatus = "No active background work"
                return
            }
            beginUIKitBackgroundAssertion()
            backgroundStatus = continuedRequestIdentifier == nil
                ? "Finite UIKit background time"
                : "Continued processing requested"
            startMonitor()
        @unknown default:
            observedScenePhase = .inactive
            state.scene(.inactive)
        }
    }

    func receive(_ url: URL) {
        let input: IOSExternalInput
        if url.scheme?.lowercased() == "magnet" {
            let magnet = url.absoluteString
            guard !magnet.isEmpty,
                  magnet.lengthOfBytes(using: .utf8) <= Self.maximumMagnetBytes
            else {
                model.reportStatus("The incoming magnet link is empty or too large.")
                return
            }
            input = .magnet(magnet)
        } else if url.isFileURL && url.pathExtension.lowercased() == "torrent" {
            input = .torrentFile(url)
        } else {
            model.reportStatus("RSTorrent can open magnet links and .torrent files.")
            return
        }

        switch state.stageInput(key: input.key) {
        case .staged:
            pendingInput = input
            Task { await drainPendingInput() }
        case .duplicate:
            model.reportStatus("That incoming torrent was already handled.")
        case .occupied:
            model.reportStatus("Finish the pending incoming torrent before opening another.")
        }
    }

    func setNotificationsEnabled(_ enabled: Bool) async {
        if enabled {
            notificationsEnabled = await notifications.requestAuthorization()
            backgroundStatus = notificationsEnabled
                ? "Background completion notifications enabled"
                : "Notifications were not authorized"
        } else {
            notificationsEnabled = false
            backgroundStatus = "Background completion notifications disabled"
        }
    }

    @available(iOS 26.0, *)
    func accept(_ task: BGContinuedProcessingTask) {
        guard continuedTaskLease == nil else {
            task.setTaskCompleted(success: false)
            return
        }
        continuedTaskLease = task
        continuedRequestIdentifier = task.identifier
        _ = state.beginContinuedProcessing()
        endUIKitBackgroundAssertion()
        task.progress.totalUnitCount = 1_000
        task.progress.completedUnitCount = 0
        task.expirationHandler = { [weak self, weak task] in
            Task { @MainActor in
                guard let self, self.continuedTaskLease === task else { return }
                await self.expireFiniteOpportunity()
            }
        }
        backgroundStatus = "Continued processing active"
        startMonitor()
    }

    private func userStartedWork() {
        workGraceDeadline = Date().addingTimeInterval(5)
        guard #available(iOS 26.0, *), continuedRequestIdentifier == nil else { return }
        let identifier = "\(Self.continuedTaskPrefix).\(UUID().uuidString.lowercased())"
        let request = BGContinuedProcessingTaskRequest(
            identifier: identifier,
            title: "RSTorrent download",
            subtitle: "Downloading and verifying selected content"
        )
        request.strategy = .fail
        do {
            try BGTaskScheduler.shared.submit(request)
            continuedRequestIdentifier = identifier
            backgroundStatus = "Continued processing requested"
            startMonitor()
        } catch {
            backgroundStatus = "Continued processing unavailable; UIKit fallback will be used"
        }
    }

    private func drainPendingInput() async {
        guard model.isReady, let pendingInput else { return }
        state.finishPendingInput()
        self.pendingInput = nil
        do {
            switch pendingInput {
            case .magnet(let magnet):
                _ = try await model.addMagnet(magnet)
            case .torrentFile(let url):
                _ = try await model.addTorrentFile(url)
            }
            model.reportStatus("Incoming torrent accepted.")
        } catch {
            model.reportStatus(error.localizedDescription)
        }
    }

    private func beginUIKitBackgroundAssertion() {
        guard backgroundAssertion == .invalid,
              state.beginUIKitBackgroundAssertion()
        else { return }
        backgroundAssertion = UIApplication.shared.beginBackgroundTask(
            withName: "RSTorrent finite checkpoint"
        ) { [weak self] in
            Task { @MainActor in await self?.expireFiniteOpportunity() }
        }
        if backgroundAssertion == .invalid {
            state.endUIKitBackgroundAssertion()
        }
    }

    private func endUIKitBackgroundAssertion() {
        guard backgroundAssertion != .invalid else { return }
        UIApplication.shared.endBackgroundTask(backgroundAssertion)
        backgroundAssertion = .invalid
        state.endUIKitBackgroundAssertion()
    }

    private func startMonitor() {
        guard monitorTask == nil else { return }
        monitorTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                let work = currentWork()
                updateProgress(work)
                if !work.active {
                    let shouldNotify = state.phase == .background
                    finishFiniteOpportunity(success: true, notify: shouldNotify)
                    return
                }
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }

    private func updateProgress(_ work: WorkProgress) {
        guard #available(iOS 26.0, *),
              let continuedTask = continuedTaskLease as? BGContinuedProcessingTask
        else { return }
        continuedTask.progress.totalUnitCount = work.total
        continuedTask.progress.completedUnitCount = work.completed
        continuedTask.updateTitle(
            "RSTorrent download",
            subtitle: "\(work.completed) of \(work.total) pieces verified"
        )
    }

    private func finishFiniteOpportunity(success: Bool, notify: Bool) {
        monitorTask?.cancel()
        monitorTask = nil
        endUIKitBackgroundAssertion()
        if #available(iOS 26.0, *),
           let continuedTask = continuedTaskLease as? BGContinuedProcessingTask
        {
            continuedTask.expirationHandler = nil
            continuedTask.setTaskCompleted(success: success)
            continuedTaskLease = nil
            state.endContinuedProcessing()
        }
        if #available(iOS 26.0, *), let identifier = continuedRequestIdentifier {
            BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: identifier)
        }
        continuedRequestIdentifier = nil
        if notify, notificationsEnabled {
            Task { await notifications.postCompletion() }
        }
        backgroundStatus = success ? "Background work complete" : "Background time expired"
    }

    private func expireFiniteOpportunity() async {
        guard model.isReady else {
            finishFiniteOpportunity(success: false, notify: false)
            return
        }
        state.beginStopping()
        backgroundStatus = "Background time expired; saving state"
        do {
            try await model.shutdown()
        } catch {
            model.reportStatus(error.localizedDescription)
        }
        finishFiniteOpportunity(success: false, notify: false)
        state.engineStopped()
    }

    private func currentWork() -> WorkProgress {
        let active = model.presentation.torrents.filter {
            switch $0.state {
            case .awaitingMetadata, .checking, .downloading:
                return true
            case .awaitingStorage, .paused, .complete, .needsRepair, .error:
                return false
            }
        }
        guard !active.isEmpty else {
            if workGraceDeadline.map({ $0 > Date() }) == true {
                return WorkProgress(active: true, completed: 0, total: 1)
            }
            workGraceDeadline = nil
            return WorkProgress(active: false, completed: 1, total: 1)
        }
        workGraceDeadline = nil
        let total = active.reduce(Int64(0)) { partial, torrent in
            partial + Int64(max(torrent.pieceCount, 1))
        }
        let completed = active.reduce(Int64(0)) { partial, torrent in
            partial + Int64(min(torrent.verifiedPieceCount, max(torrent.pieceCount, 1)))
        }
        return WorkProgress(active: true, completed: completed, total: max(total, 1))
    }
}

private struct WorkProgress {
    var active: Bool
    var completed: Int64
    var total: Int64
}

@MainActor
final class IOSExternalInputBridge {
    static let shared = IOSExternalInputBridge()

    private weak var owner: IOSApplicationLifecycleOwner?
    private var pendingURL: URL?

    func attach(_ owner: IOSApplicationLifecycleOwner) {
        self.owner = owner
        if let pendingURL {
            self.pendingURL = nil
            owner.receive(pendingURL)
        }
    }

    func deliver(_ url: URL) {
        if let owner {
            owner.receive(url)
        } else if pendingURL == nil {
            pendingURL = url
        }
    }
}

@MainActor
final class IOSBackgroundTaskBridge {
    static let shared = IOSBackgroundTaskBridge()

    private weak var owner: IOSApplicationLifecycleOwner?
    private var pendingTask: AnyObject?
    private var registered = false

    func register() {
        guard #available(iOS 26.0, *), !registered else { return }
        registered = BGTaskScheduler.shared.register(
            forTaskWithIdentifier: IOSApplicationLifecycleOwner.continuedTaskWildcard,
            using: nil
        ) { task in
            Task { @MainActor in
                guard let continued = task as? BGContinuedProcessingTask else {
                    task.setTaskCompleted(success: false)
                    return
                }
                IOSBackgroundTaskBridge.shared.deliver(continued)
            }
        }
    }

    func attach(_ owner: IOSApplicationLifecycleOwner) {
        self.owner = owner
        if #available(iOS 26.0, *),
           let pendingTask = pendingTask as? BGContinuedProcessingTask
        {
            self.pendingTask = nil
            owner.accept(pendingTask)
        }
    }

    @available(iOS 26.0, *)
    private func deliver(_ task: BGContinuedProcessingTask) {
        if let owner {
            owner.accept(task)
        } else if pendingTask == nil {
            pendingTask = task
        } else {
            task.setTaskCompleted(success: false)
        }
    }
}

private actor IOSNotificationCoordinator {
    private static let category = "RSTORRENT_BACKGROUND_COMPLETE"
    private static let request = "org.rstorrent.ios.background-complete"

    init() {
        let category = UNNotificationCategory(
            identifier: Self.category,
            actions: [],
            intentIdentifiers: []
        )
        UNUserNotificationCenter.current().setNotificationCategories([category])
    }

    func isAuthorized() async -> Bool {
        let settings = await UNUserNotificationCenter.current().notificationSettings()
        return settings.authorizationStatus == .authorized
            || settings.authorizationStatus == .provisional
    }

    func requestAuthorization() async -> Bool {
        do {
            return try await UNUserNotificationCenter.current().requestAuthorization(
                options: [.alert, .sound]
            )
        } catch {
            return false
        }
    }

    func postCompletion() async {
        let content = UNMutableNotificationContent()
        content.title = "RSTorrent"
        content.body = "Background download work completed."
        content.categoryIdentifier = Self.category
        UNUserNotificationCenter.current().removePendingNotificationRequests(
            withIdentifiers: [Self.request]
        )
        try? await UNUserNotificationCenter.current().add(
            UNNotificationRequest(identifier: Self.request, content: content, trigger: nil)
        )
    }
}
