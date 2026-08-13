import Foundation
import RSTorrentIOS

struct RootDisplayItem: Identifiable, Equatable {
    var id: String
    var label: String
    var available: Bool
    var detail: String
}

@MainActor
final class AppModel: ObservableObject {
    @Published private(set) var roots: [RootDisplayItem] = []
    @Published private(set) var engineStatus = "Starting…"
    @Published private(set) var selectionStatus = "No external folder selected"
    @Published private(set) var isBusy = false
    @Published var isFolderPickerPresented = false

    private let documentsURL: URL
    private let profileURL: URL
    private let rootStore: RootRegistryStore
    private var client: IosApplicationClient?
    private var storageBridge: PlatformStorageBridge?

    init(fileManager: FileManager = .default) {
        documentsURL = fileManager.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let support = fileManager.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        )[0].appendingPathComponent("RSTorrent", isDirectory: true)
        profileURL = support.appendingPathComponent("profile", isDirectory: true)
        rootStore = RootRegistryStore(
            fileURL: support.appendingPathComponent("roots-v1.json", isDirectory: false)
        )
    }

    func start() async {
        guard client == nil else { return }
        isBusy = true
        defer { isBusy = false }
        do {
            try FileManager.default.createDirectory(
                at: documentsURL,
                withIntermediateDirectories: true
            )
            try FileManager.default.createDirectory(
                at: profileURL,
                withIntermediateDirectories: true
            )
            let document = try await rootStore.load()
            let restored = await restore(document.selectedRoots)
            try await open(records: document.selectedRoots)
            roots = [
                RootDisplayItem(
                    id: "ios-documents",
                    label: "RSTorrent Documents",
                    available: true,
                    detail: "On My iPhone"
                )
            ] + restored
            engineStatus = "Ready"
            if document.selectedRoots.isEmpty {
                selectionStatus = "No external folder selected"
            } else if restored.allSatisfy(\.available) {
                selectionStatus = "External folders ready"
            } else {
                selectionStatus = "An external folder needs repair"
            }
        } catch {
            engineStatus = "Unavailable: \(error.localizedDescription)"
        }
    }

    func selectFolder(_ url: URL) async {
        guard !isBusy else { return }
        isBusy = true
        selectionStatus = "Checking selected folder…"
        defer { isBusy = false }
        do {
            let document = try await rootStore.load()
            let existingURLs = await resolvedURLs(document.selectedRoots)
            let documentsURL = documentsURL
            let qualified = try await Task.detached {
                try RootAccess.qualifySelection(
                    url,
                    registeredURLs: [documentsURL] + existingURLs
                )
            }.value
            let record = try await rootStore.install(
                bookmarkData: qualified.bookmarkData,
                displayLabel: qualified.displayLabel
            )
            try await restart()
            selectionStatus = "\(record.displayLabel) is ready"
        } catch {
            selectionStatus = error.localizedDescription
        }
    }

    func shutdown() async throws {
        guard let client else { return }
        try await client.shutdown()
        await storageBridge?.stopAfterClientShutdown()
        self.client = nil
        storageBridge = nil
        engineStatus = "Stopped"
    }

    private func restart() async throws {
        if let client {
            try await client.shutdown()
            await storageBridge?.stopAfterClientShutdown()
            self.client = nil
            storageBridge = nil
        }
        let document = try await rootStore.load()
        let restored = await restore(document.selectedRoots)
        try await open(records: document.selectedRoots)
        roots = [
            RootDisplayItem(
                id: "ios-documents",
                label: "RSTorrent Documents",
                available: true,
                detail: "On My iPhone"
            )
        ] + restored
    }

    private func open(records: [SelectedRootRecord]) async throws {
        let roots = [
            IosStorageRootConfig(
                id: "ios-documents",
                label: "RSTorrent Documents",
                path: documentsURL.path
            )
        ] + records.map {
            IosStorageRootConfig(id: $0.id, label: $0.displayLabel, path: nil)
        }
        let opened = try await IosApplicationClient.open(
            config: IosApplicationConfig(
                profileRoot: profileURL.path,
                profileId: "ios",
                storageRoots: roots,
                networkPolicy: .online,
                peerConnectTimeoutSeconds: 10,
                peerIoTimeoutSeconds: 30
            )
        )
        let bridge = PlatformStorageBridge(client: opened, roots: records)
        bridge.start()
        client = opened
        storageBridge = bridge
        if !records.isEmpty {
            _ = try await opened.probePlatformStorageRoots()
        }
    }

    private func restore(_ records: [SelectedRootRecord]) async -> [RootDisplayItem] {
        var registered = [documentsURL]
        var displays: [RootDisplayItem] = []
        for record in records {
            do {
                let registeredURLs = registered
                let restored = try await Task.detached {
                    try RootAccess.restore(record, registeredURLs: registeredURLs)
                }.value
                registered.append(restored.coordinatedURL)
                if restored.stale {
                    _ = try await rootStore.replace(
                        id: record.id,
                        bookmarkData: restored.bookmarkData,
                        displayLabel: restored.displayLabel
                    )
                }
                displays.append(
                    RootDisplayItem(
                        id: record.id,
                        label: restored.displayLabel,
                        available: true,
                        detail: "Qualified on-device folder"
                    )
                )
            } catch {
                displays.append(
                    RootDisplayItem(
                        id: record.id,
                        label: record.displayLabel,
                        available: false,
                        detail: error.localizedDescription
                    )
                )
            }
        }
        return displays
    }

    private func resolvedURLs(_ records: [SelectedRootRecord]) async -> [URL] {
        await Task.detached {
            records.compactMap { try? RootAccess.resolveBookmark($0.bookmarkData) }
        }.value
    }
}
