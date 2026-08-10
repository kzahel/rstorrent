import BackgroundTasks
import Foundation
import SwiftUI
import UniformTypeIdentifiers
import UIKit

enum ProbePaths {
    static var documents: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
    }
}

enum ProbeDefaults {
    private static let evidenceKeys = [
        "app_owned", "selected", "eligibility", "root_state", "resources",
        "recovery", "network", "continued_registered", "continued_result",
        "ordinary_expiration", "last_phase",
    ]
    private static let lock = NSLock()

    static func set(_ key: String, _ value: String) {
        lock.withLock {
            UserDefaults.standard.set(value, forKey: key)
            UserDefaults.standard.synchronize()
            exportEvidenceUnlocked()
        }
    }

    static func string(_ key: String, default fallback: String = "pending") -> String {
        UserDefaults.standard.string(forKey: key) ?? fallback
    }

    static func exportEvidence() {
        lock.withLock { exportEvidenceUnlocked() }
    }

    private static func exportEvidenceUnlocked() {
        var evidence: [String: Any] = [:]
        for key in evidenceKeys {
            evidence[key] = UserDefaults.standard.string(forKey: key) ?? "pending"
        }
        evidence["launch_count"] = UserDefaults.standard.integer(forKey: "launch_count")
        evidence["force_close_armed"] = UserDefaults.standard.bool(forKey: "force_close_armed")
        evidence["force_close_recovered"] = UserDefaults.standard.bool(
            forKey: "force_close_recovered"
        )
        evidence["written_unix_seconds"] = Date().timeIntervalSince1970
        guard let data = try? JSONSerialization.data(
            withJSONObject: evidence,
            options: [.prettyPrinted, .sortedKeys]
        ) else { return }
        try? data.write(
            to: ProbePaths.documents.appendingPathComponent("ProbeEvidence.json"),
            options: .atomic
        )
    }
}

struct RustResult {
    let json: String
    let ok: Bool

    static func decode(_ json: String) -> RustResult {
        let object = try? JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any]
        return RustResult(json: json, ok: object?["ok"] as? Bool == true)
    }
}

enum RustProbe {
    static func storage(at root: URL) -> RustResult {
        let json = root.path.withCString { path in
            take(rstorrent_ios_probe_run_storage(path))
        }
        return .decode(json)
    }

    static func prepareInterruptedStorage(at root: URL) -> RustResult {
        let json = root.path.withCString { path in
            take(rstorrent_ios_probe_prepare_interrupted_storage(path))
        }
        return .decode(json)
    }

    static func network(host: String, tcpPort: UInt16, udpPort: UInt16) -> RustResult {
        let json = host.withCString { address in
            take(rstorrent_ios_probe_run_network(address, tcpPort, udpPort))
        }
        return .decode(json)
    }

    private static func take(_ value: UnsafeMutablePointer<CChar>?) -> String {
        guard let value else { return #"{"ok":false,"error":"null Rust result"}"# }
        defer { rstorrent_ios_probe_free_json(value) }
        return String(cString: value)
    }
}

@MainActor
final class ProbeModel: ObservableObject {
    @Published private(set) var appOwned = ProbeDefaults.string("app_owned")
    @Published private(set) var selected = ProbeDefaults.string("selected", default: "none")
    @Published private(set) var eligibility = ProbeDefaults.string(
        "eligibility",
        default: "not observed"
    )
    @Published private(set) var rootState = ProbeDefaults.string("root_state")
    @Published private(set) var resources = ProbeDefaults.string("resources")
    @Published private(set) var network = ProbeDefaults.string("network")
    @Published private(set) var lifecycle = "pending"
    @Published var presentsPicker = false

    private let rootStore = ProbeRootStore()
    private let rootQueue: OperationQueue = {
        let queue = OperationQueue()
        queue.name = "com.kgraehl.rstorrent.storage-probe.root"
        queue.maxConcurrentOperationCount = 1
        queue.qualityOfService = .userInitiated
        return queue
    }()
    private var appOwnedRoot: ProbeRootRecord?
    private var appOwnedToken: UInt64 = 0
    private var selectedToken: UInt64 = 0
    private var ordinaryBackgroundTask: UIBackgroundTaskIdentifier = .invalid

    init() {
        let environment = ProcessInfo.processInfo.environment
        let launches = UserDefaults.standard.integer(forKey: "launch_count") + 1
        UserDefaults.standard.set(launches, forKey: "launch_count")
        UserDefaults.standard.synchronize()
        ProbeDefaults.exportEvidence()

        configureRootState()
        runNetworkFromEnvironment()
        if environment["RSTORRENT_PROBE_ARM_EXPIRATION"] == "1" {
            DispatchQueue.main.async { [weak self] in self?.armOrdinaryExpirationProbe() }
        }
        if environment["RSTORRENT_PROBE_SUBMIT_CONTINUED"] == "1" {
            DispatchQueue.main.async { [weak self] in self?.submitContinuedProbe() }
        }
        if environment["RSTORRENT_PROBE_PREPARE_APP_INTERRUPTION"] == "1" {
            DispatchQueue.main.async { [weak self] in
                self?.prepareAppOwnedInterruption()
            }
        }
    }

    deinit {
        rootQueue.cancelAllOperations()
        rootQueue.waitUntilAllOperationsAreFinished()
    }

    func runAppOwned() {
        guard let root = appOwnedRoot else {
            publishAppOwned("fail app-owned root record unavailable")
            return
        }
        appOwnedToken &+= 1
        let token = appOwnedToken
        rootQueue.addOperation { [weak self] in
            let status: String
            do {
                let (_, rust) = try ProbeRootAccess.runAppOwned(at: ProbePaths.documents)
                status = "pass generation=\(root.generation) \(rust.json)"
            } catch {
                status = "fail \(Self.boundedError(error))"
            }
            Self.persistResourceEvidence()
            DispatchQueue.main.async { [weak self] in
                guard let self, self.appOwnedToken == token else { return }
                self.publishAppOwned(status)
            }
        }
    }

    func chooseFolder() {
        presentsPicker = true
    }

    func selectedFolder(_ url: URL) {
        presentsPicker = false
        selectedToken &+= 1
        let token = selectedToken
        let rootStore = rootStore
        rootQueue.addOperation { [weak self] in
            var status = "fail operation did not complete"
            var eligibilityStatus = "failed-before-decision"
            do {
                let result = try ProbeRootAccess.inspectSelected(url)
                eligibilityStatus = Self.formatEligibility(result.observed)
                if result.observed.decision.classification == .selectedOnDevice,
                   let bookmark = result.bookmarkData,
                   let rust = result.rustResult
                {
                    let record = try rootStore.installSelected(
                        bookmarkData: bookmark,
                        displayLabel: result.observed.displayLabel
                    )
                    status = "pass restored=false stale=false generation=\(record.generation) \(rust.json)"
                } else {
                    status = "rejected \(eligibilityStatus) bookmark=false rust=false"
                }
            } catch {
                status = "fail \(Self.boundedError(error))"
            }
            Self.persistResourceEvidence()
            ProbeDefaults.set("eligibility", eligibilityStatus)
            ProbeDefaults.set("selected", status)
            DispatchQueue.main.async { [weak self] in
                guard let self, self.selectedToken == token else { return }
                self.eligibility = eligibilityStatus
                self.selected = status
                self.refreshRootState()
            }
        }
    }

    func cancelPicker() {
        presentsPicker = false
        eligibility = "picker cancelled"
        ProbeDefaults.set("eligibility", eligibility)
    }

    func retrySelected() {
        restoreSelectedBookmark()
    }

    func prepareAppOwnedInterruption() {
        guard let root = appOwnedRoot else {
            publishRecovery("fail app-owned root record unavailable")
            return
        }
        appOwnedToken &+= 1
        let token = appOwnedToken
        let rootStore = rootStore
        rootQueue.addOperation { [weak self] in
            let status: String
            do {
                try rootStore.beginPendingOperation(for: root)
                do {
                    let rust = try ProbeRootAccess.prepareAppOwnedInterruption(
                        at: ProbePaths.documents
                    )
                    UserDefaults.standard.set(true, forKey: "force_close_armed")
                    UserDefaults.standard.set(false, forKey: "force_close_recovered")
                    UserDefaults.standard.synchronize()
                    status = "prepared app_owned generation=\(root.generation) \(rust.json)"
                } catch {
                    _ = try? rootStore.completePendingOperation(
                        rootID: root.stableRootID,
                        generation: root.generation
                    )
                    throw error
                }
            } catch {
                status = "fail \(Self.boundedError(error))"
            }
            Self.persistResourceEvidence()
            ProbeDefaults.set("recovery", status)
            DispatchQueue.main.async { [weak self] in
                guard let self, self.appOwnedToken == token else { return }
                self.publishRecovery(status)
                self.refreshRootState()
            }
        }
    }

    func prepareSelectedInterruption() {
        let record: ProbeRootRecord
        do {
            guard let selected = try rootStore.load().roots.first(where: {
                $0.kind == .selectedOnDevice
            }) else {
                publishRecovery("fail selected root unavailable")
                return
            }
            record = selected
        } catch {
            publishRecovery("fail \(Self.boundedError(error))")
            return
        }
        guard let bookmark = record.bookmarkData else {
            publishRecovery("fail selected bookmark unavailable")
            return
        }
        selectedToken &+= 1
        let token = selectedToken
        let rootStore = rootStore
        rootQueue.addOperation { [weak self] in
            let status: String
            do {
                try rootStore.beginPendingOperation(for: record)
                do {
                    let result = try ProbeRootAccess.prepareSelectedInterruption(
                        bookmarkData: bookmark
                    )
                    guard
                        result.observed.decision.classification == .selectedOnDevice,
                        let rust = result.rustResult
                    else {
                        throw ProbeRootAccessError.resourceObservationFailed(
                            result.observed.decision.reason.rawValue
                        )
                    }
                    UserDefaults.standard.set(true, forKey: "force_close_armed")
                    UserDefaults.standard.set(false, forKey: "force_close_recovered")
                    UserDefaults.standard.synchronize()
                    status = "prepared selected_on_device generation=\(record.generation) \(rust.json)"
                } catch {
                    _ = try? rootStore.completePendingOperation(
                        rootID: record.stableRootID,
                        generation: record.generation
                    )
                    throw error
                }
            } catch {
                status = "fail \(Self.boundedError(error))"
            }
            Self.persistResourceEvidence()
            ProbeDefaults.set("recovery", status)
            DispatchQueue.main.async { [weak self] in
                guard let self, self.selectedToken == token else { return }
                self.publishRecovery(status)
                self.refreshRootState()
            }
        }
    }

    func submitContinuedProbe() {
        guard #available(iOS 26.0, *) else {
            ProbeDefaults.set("continued_result", "unavailable")
            refreshLifecycle()
            return
        }
        let request = BGContinuedProcessingTaskRequest(
            identifier: "com.kgraehl.rstorrent.storage-probe.continued",
            title: "RSTorrent finite check",
            subtitle: "Preparing Rust storage validation"
        )
        request.strategy = .fail
        do {
            try BGTaskScheduler.shared.submit(request)
            ProbeDefaults.set("continued_result", "submitted")
        } catch {
            ProbeDefaults.set("continued_result", "submit-failed \(error.localizedDescription)")
        }
        refreshLifecycle()
    }

    func armOrdinaryExpirationProbe() {
        guard ordinaryBackgroundTask == .invalid else { return }
        ProbeDefaults.set("ordinary_expiration", "armed")
        ordinaryBackgroundTask = UIApplication.shared.beginBackgroundTask(
            withName: "RSTorrent ordinary expiration probe"
        ) { [weak self] in
            ProbeDefaults.set("ordinary_expiration", "expired")
            guard let self, self.ordinaryBackgroundTask != .invalid else { return }
            UIApplication.shared.endBackgroundTask(self.ordinaryBackgroundTask)
            self.ordinaryBackgroundTask = .invalid
        }
        refreshLifecycle()
    }

    func openAppSettings() {
        guard let settingsURL = URL(string: UIApplication.openSettingsURLString) else {
            return
        }
        UIApplication.shared.open(settingsURL)
    }

    func record(phase: ScenePhase) {
        ProbeDefaults.set("last_phase", String(describing: phase))
        if phase == .active {
            refreshLifecycle()
        }
    }

    private func configureRootState() {
        do {
            let appRoot = try rootStore.ensureAppOwned(
                displayLabel: "On My iPhone / RSTorrent Probe"
            )
            appOwnedRoot = appRoot
            let registry = try rootStore.load()
            refreshRootState(registry)

            if let pending = registry.pendingOperation,
               let root = registry.roots.first(where: {
                   $0.stableRootID == pending.rootID
                       && $0.generation == pending.rootGeneration
               })
            {
                recover(pending: pending, root: root)
                if root.kind != .appOwned {
                    runAppOwned()
                }
                if root.kind != .selectedOnDevice {
                    restoreSelectedBookmark(from: registry)
                }
            } else {
                runAppOwned()
                restoreSelectedBookmark(from: registry)
            }
        } catch {
            let status = "fail \(Self.boundedError(error))"
            rootState = status
            ProbeDefaults.set("root_state", status)
            publishAppOwned(status)
        }
    }

    private func restoreSelectedBookmark(from loadedRegistry: ProbeRootRegistry? = nil) {
        let record: ProbeRootRecord
        do {
            let registry = try loadedRegistry ?? rootStore.load()
            guard let selected = registry.roots.first(where: {
                $0.kind == .selectedOnDevice
            }) else {
                self.selected = "none app-owned-only"
                ProbeDefaults.set("selected", self.selected)
                return
            }
            record = selected
        } catch {
            let status = "fail root-load \(Self.boundedError(error))"
            selected = status
            ProbeDefaults.set("selected", status)
            return
        }
        guard let bookmark = record.bookmarkData else {
            let status = "fail selected bookmark unavailable"
            selected = status
            ProbeDefaults.set("selected", status)
            return
        }

        selectedToken &+= 1
        let token = selectedToken
        let rootStore = rootStore
        rootQueue.addOperation { [weak self] in
            var status = "unavailable operation did not complete"
            var eligibilityStatus = "failed-before-decision"
            do {
                let result = try ProbeRootAccess.restoreSelected(bookmarkData: bookmark)
                eligibilityStatus = Self.formatEligibility(result.observed)
                guard
                    result.observed.decision.classification == .selectedOnDevice,
                    let refreshedBookmark = result.bookmarkData,
                    let rust = result.rustResult
                else {
                    status = "unavailable \(eligibilityStatus) fallback=false"
                    Self.persistResourceEvidence()
                    ProbeDefaults.set("eligibility", eligibilityStatus)
                    ProbeDefaults.set("selected", status)
                    DispatchQueue.main.async { [weak self] in
                        guard let self, self.selectedToken == token else { return }
                        self.eligibility = eligibilityStatus
                        self.selected = status
                    }
                    return
                }
                let activeRecord: ProbeRootRecord
                if result.stale {
                    activeRecord = try rootStore.installSelected(
                        bookmarkData: refreshedBookmark,
                        displayLabel: result.observed.displayLabel
                    )
                } else {
                    activeRecord = record
                }
                status = "pass restored=true stale=\(result.stale) generation=\(activeRecord.generation) \(rust.json)"
            } catch {
                status = "unavailable \(Self.boundedError(error)) fallback=false"
            }
            Self.persistResourceEvidence()
            ProbeDefaults.set("eligibility", eligibilityStatus)
            ProbeDefaults.set("selected", status)
            DispatchQueue.main.async { [weak self] in
                guard let self, self.selectedToken == token else { return }
                self.eligibility = eligibilityStatus
                self.selected = status
                self.refreshRootState()
            }
        }
    }

    private func recover(pending: ProbePendingOperation, root: ProbeRootRecord) {
        let token: UInt64
        switch root.kind {
        case .appOwned:
            appOwnedToken &+= 1
            token = appOwnedToken
        case .selectedOnDevice:
            selectedToken &+= 1
            token = selectedToken
        }
        let rootStore = rootStore
        rootQueue.addOperation { [weak self] in
            let status: String
            do {
                let rust: RustResult
                var refreshedBookmark: Data?
                var refreshedLabel: String?
                switch root.kind {
                case .appOwned:
                    (_, rust) = try ProbeRootAccess.runAppOwned(at: ProbePaths.documents)
                case .selectedOnDevice:
                    guard let bookmark = root.bookmarkData else {
                        throw ProbeRootStoreError.invalidBookmark
                    }
                    let result = try ProbeRootAccess.restoreSelected(bookmarkData: bookmark)
                    guard
                        result.observed.decision.classification == .selectedOnDevice,
                        let resultRust = result.rustResult
                    else {
                        throw ProbeRootAccessError.resourceObservationFailed(
                            result.observed.decision.reason.rawValue
                        )
                    }
                    rust = resultRust
                    if result.stale {
                        refreshedBookmark = result.bookmarkData
                        refreshedLabel = result.observed.displayLabel
                    }
                }
                if let refreshedBookmark, let refreshedLabel {
                    _ = try rootStore.completePendingOperationAndRefreshSelected(
                        rootID: pending.rootID,
                        generation: pending.rootGeneration,
                        bookmarkData: refreshedBookmark,
                        displayLabel: refreshedLabel
                    )
                } else {
                    guard try rootStore.completePendingOperation(
                        rootID: pending.rootID,
                        generation: pending.rootGeneration
                    ) else {
                        throw ProbeRootStoreError.invalidPendingOperation
                    }
                }
                UserDefaults.standard.set(false, forKey: "force_close_armed")
                UserDefaults.standard.set(true, forKey: "force_close_recovered")
                UserDefaults.standard.synchronize()
                status = "pass kind=\(root.kind.rawValue) generation=\(root.generation) \(rust.json)"
            } catch {
                status = "fail kind=\(root.kind.rawValue) \(Self.boundedError(error))"
            }
            Self.persistResourceEvidence()
            ProbeDefaults.set("recovery", status)
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                switch root.kind {
                case .appOwned:
                    guard self.appOwnedToken == token else { return }
                    self.appOwned = "recovered \(status)"
                    ProbeDefaults.set("app_owned", self.appOwned)
                case .selectedOnDevice:
                    guard self.selectedToken == token else { return }
                    self.selected = "recovered \(status)"
                    ProbeDefaults.set("selected", self.selected)
                }
                self.publishRecovery(status)
                self.refreshRootState()
            }
        }
    }

    private func runNetworkFromEnvironment() {
        let environment = ProcessInfo.processInfo.environment
        guard
            let host = environment["RSTORRENT_PROBE_HOST"],
            let tcpText = environment["RSTORRENT_PROBE_TCP_PORT"],
            let udpText = environment["RSTORRENT_PROBE_UDP_PORT"],
            let tcpPort = UInt16(tcpText),
            let udpPort = UInt16(udpText)
        else {
            network = "skipped no controlled endpoint"
            ProbeDefaults.set("network", network)
            return
        }
        DispatchQueue.global(qos: .userInitiated).async {
            let result = RustProbe.network(host: host, tcpPort: tcpPort, udpPort: udpPort)
            ProbeDefaults.set("network", result.ok ? "pass \(result.json)" : "fail \(result.json)")
            DispatchQueue.main.async { [weak self] in
                self?.network = ProbeDefaults.string("network")
            }
        }
    }

    private func refreshRootState(_ loadedRegistry: ProbeRootRegistry? = nil) {
        do {
            let registry = try loadedRegistry ?? rootStore.load()
            let appGeneration = registry.roots.first(where: { $0.kind == .appOwned })?.generation
            let selectedGeneration = registry.roots.first(where: {
                $0.kind == .selectedOnDevice
            })?.generation
            let pending = registry.pendingOperation?.phase.rawValue ?? "none"
            let status = "schema=\(registry.schemaVersion) roots=\(registry.roots.count) appGeneration=\(Self.optional(appGeneration)) selectedGeneration=\(Self.optional(selectedGeneration)) pending=\(pending)"
            rootState = status
            ProbeDefaults.set("root_state", status)
        } catch {
            let status = "fail \(Self.boundedError(error))"
            rootState = status
            ProbeDefaults.set("root_state", status)
        }
        resources = ProbeDefaults.string("resources")
        refreshLifecycle()
    }

    private func publishAppOwned(_ status: String) {
        appOwned = status
        ProbeDefaults.set("app_owned", status)
        resources = ProbeDefaults.string("resources")
        refreshLifecycle()
    }

    private func publishRecovery(_ status: String) {
        ProbeDefaults.set("recovery", status)
        resources = ProbeDefaults.string("resources")
        refreshLifecycle()
    }

    private func refreshLifecycle() {
        let launches = UserDefaults.standard.integer(forKey: "launch_count")
        let continued = ProbeDefaults.string("continued_result")
        let ordinary = ProbeDefaults.string("ordinary_expiration")
        let registered = ProbeDefaults.string("continued_registered")
        let phase = ProbeDefaults.string("last_phase")
        let recovery = ProbeDefaults.string("recovery")
        let forceRecovered = UserDefaults.standard.bool(forKey: "force_close_recovered")
        let forceArmed = UserDefaults.standard.bool(forKey: "force_close_armed")
        lifecycle = "launches=\(launches) registered=\(registered) continued=\(continued) ordinary=\(ordinary) phase=\(phase) forceArmed=\(forceArmed) forceRecovered=\(forceRecovered) recovery=\(recovery)"
    }

    nonisolated private static func formatEligibility(_ observed: ProbeObservedRoot) -> String {
        let observation = observed.observation
        return "class=\(observed.decision.classification.rawValue) reason=\(observed.decision.reason.rawValue) fileURL=\(optional(observation.isFileURL)) directory=\(optional(observation.isDirectory)) symlink=\(optional(observation.isSymbolicLink)) overlapsAppOwned=\(optional(observation.overlapsAppOwnedRoot)) ubiquitous=\(optional(observation.isUbiquitousItem)) local=\(optional(observation.volumeIsLocal)) internal=\(optional(observation.volumeIsInternal)) provider=\(observation.fileProviderLookup.rawValue)"
    }

    nonisolated private static func persistResourceEvidence() {
        ProbeDefaults.set("resources", ProbeResourceLedger.shared.evidence())
    }

    nonisolated private static func boundedError(_ error: Error) -> String {
        if let rootAccessError = error as? ProbeRootAccessError {
            return rootAccessError.evidenceCode
        }
        if let rootStoreError = error as? ProbeRootStoreError {
            return "root_store_\(String(describing: rootStoreError))"
        }
        let cocoaError = error as NSError
        let domain = cocoaError.domain
            .unicodeScalars
            .filter { CharacterSet.alphanumerics.contains($0) || $0 == "." || $0 == "-" }
            .prefix(128)
        return "platform_error domain=\(String(String.UnicodeScalarView(domain))) code=\(cocoaError.code)"
    }

    nonisolated private static func optional<T>(_ value: T?) -> String {
        value.map(String.init(describing:)) ?? "nil"
    }
}
