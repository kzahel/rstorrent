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
        "app_owned", "selected", "network", "continued_registered",
        "continued_result", "ordinary_expiration", "last_phase"
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
        for key in ["launch_count"] {
            evidence[key] = UserDefaults.standard.integer(forKey: key)
        }
        for key in ["force_close_armed", "force_close_recovered"] {
            evidence[key] = UserDefaults.standard.bool(forKey: key)
        }
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
    @Published private(set) var selected = ProbeDefaults.string("selected")
    @Published private(set) var network = ProbeDefaults.string("network")
    @Published private(set) var lifecycle = "pending"
    @Published var presentsPicker = false

    private let bookmarkKey = "selected_bookmark"
    private var ordinaryBackgroundTask: UIBackgroundTaskIdentifier = .invalid

    init() {
        let environment = ProcessInfo.processInfo.environment
        let launches = UserDefaults.standard.integer(forKey: "launch_count") + 1
        UserDefaults.standard.set(launches, forKey: "launch_count")
        if UserDefaults.standard.bool(forKey: "force_close_armed") {
            UserDefaults.standard.set(true, forKey: "force_close_recovered")
            UserDefaults.standard.set(false, forKey: "force_close_armed")
        }
        if environment["RSTORRENT_PROBE_ARM_FORCE_CLOSE"] == "1" {
            UserDefaults.standard.set(true, forKey: "force_close_armed")
        }
        ProbeDefaults.exportEvidence()
        preparePickerFixture()
        refreshLifecycle()
        runAppOwned()
        runNetworkFromEnvironment()
        if environment["RSTORRENT_PROBE_SELECTED_FIXTURE"] == "1" {
            selectedFolder(
                ProbePaths.documents.appendingPathComponent("PickerRoot", isDirectory: true)
            )
        } else {
            restoreSelectedBookmark()
        }
        if environment["RSTORRENT_PROBE_ARM_EXPIRATION"] == "1" {
            DispatchQueue.main.async { [weak self] in self?.armOrdinaryExpirationProbe() }
        }
        if environment["RSTORRENT_PROBE_SUBMIT_CONTINUED"] == "1" {
            DispatchQueue.main.async { [weak self] in self?.submitContinuedProbe() }
        }
    }

    func runAppOwned() {
        DispatchQueue.global(qos: .userInitiated).async {
            let result = RustProbe.storage(at: ProbePaths.documents)
            ProbeDefaults.set("app_owned", result.ok ? "pass \(result.json)" : "fail \(result.json)")
            DispatchQueue.main.async { [weak self] in
                self?.appOwned = ProbeDefaults.string("app_owned")
            }
        }
    }

    func chooseFolder() {
        presentsPicker = true
    }

    func selectedFolder(_ url: URL) {
        presentsPicker = false
        do {
            let bookmark = try url.bookmarkData(
                options: [],
                includingResourceValuesForKeys: [.nameKey, .isDirectoryKey],
                relativeTo: nil
            )
            UserDefaults.standard.set(bookmark, forKey: bookmarkKey)
            runSelected(url: url, restored: false, stale: false)
        } catch {
            selected = "fail bookmark-create \(error.localizedDescription)"
            ProbeDefaults.set("selected", selected)
        }
    }

    func cancelPicker() {
        presentsPicker = false
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

    func armForceCloseProbe() {
        UserDefaults.standard.set(true, forKey: "force_close_armed")
        UserDefaults.standard.synchronize()
        ProbeDefaults.exportEvidence()
        refreshLifecycle()
    }

    func record(phase: ScenePhase) {
        ProbeDefaults.set("last_phase", String(describing: phase))
        if phase == .active {
            refreshLifecycle()
        }
    }

    private func preparePickerFixture() {
        try? FileManager.default.createDirectory(
            at: ProbePaths.documents.appendingPathComponent("PickerRoot", isDirectory: true),
            withIntermediateDirectories: true
        )
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

    private func restoreSelectedBookmark() {
        guard let bookmark = UserDefaults.standard.data(forKey: bookmarkKey) else { return }
        do {
            var stale = false
            let url = try URL(
                resolvingBookmarkData: bookmark,
                options: [],
                relativeTo: nil,
                bookmarkDataIsStale: &stale
            )
            if stale {
                let refreshed = try url.bookmarkData(
                    options: [],
                    includingResourceValuesForKeys: [.nameKey, .isDirectoryKey],
                    relativeTo: nil
                )
                UserDefaults.standard.set(refreshed, forKey: bookmarkKey)
            }
            runSelected(url: url, restored: true, stale: stale)
        } catch {
            selected = "fail bookmark-restore \(error.localizedDescription)"
            ProbeDefaults.set("selected", selected)
        }
    }

    private func runSelected(url: URL, restored: Bool, stale: Bool) {
        DispatchQueue.global(qos: .userInitiated).async {
            let scoped = url.startAccessingSecurityScopedResource()
            defer {
                if scoped { url.stopAccessingSecurityScopedResource() }
            }
            let coordinator = NSFileCoordinator(filePresenter: nil)
            var coordinationError: NSError?
            var rust = RustResult(json: #"{"ok":false,"error":"coordination accessor did not run"}"#, ok: false)
            coordinator.coordinate(
                writingItemAt: url,
                options: .forMerging,
                error: &coordinationError
            ) { coordinatedURL in
                rust = RustProbe.storage(at: coordinatedURL)
            }
            let result: String
            if let coordinationError {
                result = "fail coordinated \(coordinationError.localizedDescription)"
            } else {
                let prefix = rust.ok ? "pass" : "fail"
                result = "\(prefix) restored=\(restored) stale=\(stale) scope=\(scoped) coordinated=true \(rust.json)"
            }
            ProbeDefaults.set("selected", result)
            DispatchQueue.main.async { [weak self] in self?.selected = result }
        }
    }

    private func refreshLifecycle() {
        let launches = UserDefaults.standard.integer(forKey: "launch_count")
        let continued = ProbeDefaults.string("continued_result")
        let ordinary = ProbeDefaults.string("ordinary_expiration")
        let registered = ProbeDefaults.string("continued_registered")
        let phase = ProbeDefaults.string("last_phase")
        let forceRecovered = UserDefaults.standard.bool(forKey: "force_close_recovered")
        let forceArmed = UserDefaults.standard.bool(forKey: "force_close_armed")
        lifecycle = "launches=\(launches) registered=\(registered) continued=\(continued) ordinary=\(ordinary) phase=\(phase) forceArmed=\(forceArmed) forceRecovered=\(forceRecovered)"
    }
}
