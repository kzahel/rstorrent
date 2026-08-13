import Darwin
import Foundation
import RSTorrentIOS

final class PlatformStorageBridge: @unchecked Sendable {
    private let client: IosApplicationClient
    private let roots: [String: SelectedRootRecord]
    private let leases = CoordinatedLeaseLedger()
    private let workers = DispatchGroup()
    private let stateLock = NSLock()
    private var requestTask: Task<Void, Never>?
    private var releaseTask: Task<Void, Never>?
    private var stopped = false

    init(client: IosApplicationClient, roots: [SelectedRootRecord]) {
        self.client = client
        self.roots = Dictionary(uniqueKeysWithValues: roots.map { ($0.id, $0) })
    }

    func start() {
        stateLock.lock()
        defer { stateLock.unlock() }
        guard requestTask == nil, releaseTask == nil, !stopped else { return }
        requestTask = Task { [weak self] in await self?.requestLoop() }
        releaseTask = Task { [weak self] in await self?.releaseLoop() }
    }

    func stopAfterClientShutdown() async {
        stateLock.lock()
        stopped = true
        let requestTask = requestTask
        let releaseTask = releaseTask
        stateLock.unlock()
        requestTask?.cancel()
        releaseTask?.cancel()
        leases.releaseAll()
        _ = await Task.detached { [workers] in
            workers.wait(timeout: .now() + 5) == .success
        }.value
    }

    func resourceSnapshot() -> PlatformBridgeResourceSnapshot {
        PlatformBridgeResourceSnapshot(
            activeWorkers: workersActiveCount,
            activeLeases: leases.count
        )
    }

    func publish(torrentID: String, storageRoot: String, name: String) throws {
        guard let record = roots[storageRoot] else {
            throw NamespaceTransitionError.unregisteredRoot(storageRoot)
        }
        try withCoordinatedRoot(record: record) { root in
            let staging = root.appendingPathComponent(
                ".\(torrentID).rstorrent-staging",
                isDirectory: true
            )
            let published = root.appendingPathComponent(name, isDirectory: true)
            let stagingExists = FileManager.default.fileExists(atPath: staging.path)
            let publishedExists = FileManager.default.fileExists(atPath: published.path)
            guard !(stagingExists && publishedExists) else {
                throw NamespaceTransitionError.bothPublicationSidesExist
            }
            guard !publishedExists else { return }
            guard stagingExists else { throw NamespaceTransitionError.stagingMissing }
            let status = staging.path.withCString { source in
                published.path.withCString { destination in
                    renameatx_np(AT_FDCWD, source, AT_FDCWD, destination, UInt32(RENAME_EXCL))
                }
            }
            guard status == 0 else {
                throw POSIXFailure(operation: "publish torrent", code: errno)
            }
        }
    }

    func removeManaged(_ plan: IosRemovalPlan) throws {
        guard let record = roots[plan.storageRoot] else {
            throw NamespaceTransitionError.unregisteredRoot(plan.storageRoot)
        }
        try withCoordinatedRoot(record: record) { root in
            for name in Self.managedArtifactNames(
                torrentID: plan.torrentId,
                publishedName: plan.name
            ) {
                let target = root.appendingPathComponent(name, isDirectory: false)
                do {
                    try FileManager.default.removeItem(at: target)
                } catch let error as CocoaError where
                    error.code == .fileNoSuchFile || error.code == .fileReadNoSuchFile
                {
                    continue
                }
            }
        }
    }

    func openShareableFile(_ plan: IosPublishedFilePlan) throws -> ShareableFileLease {
        guard let record = roots[plan.storageRoot] else {
            throw NamespaceTransitionError.unregisteredRoot(plan.storageRoot)
        }
        let root = try RootAccess.resolveBookmark(record.bookmarkData)
        guard root.startAccessingSecurityScopedResource() else {
            throw RootAccessError.securityScopeDenied
        }
        do {
            let target = try safeTarget(root: root, components: plan.components)
            var coordinationError: NSError?
            var result: Result<URL, Error>?
            NSFileCoordinator(filePresenter: nil).coordinate(
                readingItemAt: target,
                options: .withoutChanges,
                error: &coordinationError
            ) { coordinatedURL in
                result = Result {
                    let values = try coordinatedURL.resourceValues(forKeys: [
                        .isRegularFileKey,
                        .fileSizeKey,
                    ])
                    guard values.isRegularFile == true else {
                        throw NamespaceTransitionError.shareTargetIsNotFile
                    }
                    guard values.fileSize.map(UInt64.init) == plan.length else {
                        throw NamespaceTransitionError.shareTargetLengthChanged
                    }
                    return coordinatedURL
                }
            }
            if let coordinationError {
                throw RootAccessError.coordinationFailed(
                    coordinationError.localizedDescription
                )
            }
            guard let result else {
                throw RootAccessError.coordinationAccessorDidNotRun
            }
            return ShareableFileLease(url: try result.get(), scopedRoot: root)
        } catch {
            root.stopAccessingSecurityScopedResource()
            throw error
        }
    }

    static func managedArtifactNames(torrentID: String, publishedName: String) -> [String] {
        [
            publishedName,
            ".\(torrentID).rstorrent-staging",
            ".\(torrentID).rstorrent-parts",
        ]
    }

    private var workersActiveCount: Int {
        workerCounterLock.lock()
        defer { workerCounterLock.unlock() }
        return workerCounter
    }

    private func withCoordinatedRoot<T>(
        record: SelectedRootRecord,
        body: (URL) throws -> T
    ) throws -> T {
        let url = try RootAccess.resolveBookmark(record.bookmarkData)
        return try RootAccess.withSecurityScope(url) {
            var coordinationError: NSError?
            var result: Result<T, Error>?
            NSFileCoordinator(filePresenter: nil).coordinate(
                writingItemAt: url,
                options: .forMerging,
                error: &coordinationError
            ) { coordinatedRoot in
                result = Result { try body(coordinatedRoot) }
            }
            if let coordinationError {
                throw RootAccessError.coordinationFailed(coordinationError.localizedDescription)
            }
            guard let result else {
                throw RootAccessError.coordinationAccessorDidNotRun
            }
            return try result.get()
        }
    }

    private let workerCounterLock = NSLock()
    private var workerCounter = 0

    private func requestLoop() async {
        while !Task.isCancelled, let request = await client.nextStorageRequest() {
            workers.enter()
            workerCounterLock.lock()
            workerCounter += 1
            workerCounterLock.unlock()
            Task.detached { [weak self] in
                defer {
                    if let self {
                        self.workerCounterLock.lock()
                        self.workerCounter -= 1
                        self.workerCounterLock.unlock()
                        self.workers.leave()
                    }
                }
                self?.perform(request)
            }
        }
    }

    private func releaseLoop() async {
        while !Task.isCancelled, let releaseID = await client.nextStorageRelease() {
            guard let lease = leases.take(releaseID) else {
                _ = client.acknowledgeStorageRelease(releaseId: releaseID)
                continue
            }
            await Task.detached {
                lease.release()
                lease.waitUntilFinished()
            }.value
            _ = client.acknowledgeStorageRelease(releaseId: releaseID)
        }
    }

    private func perform(_ request: IosStorageRequest) {
        guard let record = roots[request.rootId] else {
            fail(request, .grantUnavailable, "selected root is not registered")
            return
        }
        do {
            let rootURL = try RootAccess.resolveBookmark(record.bookmarkData)
            try RootAccess.withSecurityScope(rootURL) {
                switch request.operation {
                case .open:
                    try coordinatedOpen(request, rootURL: rootURL)
                case .observe:
                    try coordinatedObservation(request, rootURL: rootURL)
                case .delete:
                    try coordinatedDelete(request, rootURL: rootURL)
                }
            }
        } catch let error as RootAccessError {
            fail(request, .grantUnavailable, error.localizedDescription)
        } catch let error as CocoaError where error.code == .fileReadNoSuchFile {
            fail(request, .missing, error.localizedDescription)
        } catch {
            fail(request, .providerRefused, error.localizedDescription)
        }
    }

    private func coordinatedOpen(_ request: IosStorageRequest, rootURL: URL) throws {
        var coordinationError: NSError?
        var accessorError: Error?
        var accessorRan = false
        let lease = CoordinatedLease()
        let releaseID = leases.allocate(lease)
        NSFileCoordinator(filePresenter: nil).coordinate(
            writingItemAt: rootURL,
            options: .forMerging,
            error: &coordinationError
        ) { coordinatedRoot in
            accessorRan = true
            do {
                let target = try safeTarget(root: coordinatedRoot, components: request.path)
                if request.access == .readWriteCreate {
                    try FileManager.default.createDirectory(
                        at: target.deletingLastPathComponent(),
                        withIntermediateDirectories: true
                    )
                }
                let flags: Int32
                switch request.access {
                case .readExisting:
                    flags = O_RDONLY | O_CLOEXEC
                case .readWriteExisting:
                    flags = O_RDWR | O_CLOEXEC
                case .readWriteCreate:
                    flags = O_RDWR | O_CREAT | O_CLOEXEC
                }
                let descriptor = Darwin.open(target.path, flags, S_IRUSR | S_IWUSR)
                guard descriptor >= 0 else {
                    throw POSIXFailure(operation: "open", code: errno)
                }
                defer { Darwin.close(descriptor) }
                let accepted = try client.completeStorageRequest(
                    requestId: request.requestId,
                    fd: descriptor,
                    access: request.access,
                    releaseId: releaseID
                )
                if accepted {
                    lease.waitForRelease()
                } else {
                    leases.cancel(releaseID)
                }
            } catch {
                leases.cancel(releaseID)
                accessorError = error
            }
        }
        lease.finished()
        if let coordinationError {
            throw RootAccessError.coordinationFailed(coordinationError.localizedDescription)
        }
        guard accessorRan else {
            throw RootAccessError.coordinationAccessorDidNotRun
        }
        if let accessorError { throw accessorError }
    }

    private func coordinatedObservation(_ request: IosStorageRequest, rootURL: URL) throws {
        var coordinationError: NSError?
        var accessorError: Error?
        var accessorRan = false
        NSFileCoordinator(filePresenter: nil).coordinate(
            readingItemAt: rootURL,
            options: [],
            error: &coordinationError
        ) { coordinatedRoot in
            accessorRan = true
            do {
                let target = try safeTarget(root: coordinatedRoot, components: request.path)
                let observation: IosStorageObservation
                do {
                    let values = try target.resourceValues(forKeys: [
                        .isRegularFileKey,
                        .isDirectoryKey,
                        .fileSizeKey,
                        .contentModificationDateKey,
                    ])
                    let kind: IosStorageObjectKind
                    if values.isRegularFile == true {
                        kind = .file
                    } else if values.isDirectory == true {
                        kind = .directory
                    } else {
                        kind = .other
                    }
                    let length = kind == .file ? values.fileSize.map(UInt64.init) : nil
                    let modified = values.contentModificationDate.map {
                        Int64($0.timeIntervalSince1970 * 1_000_000_000)
                    }
                    observation = IosStorageObservation(
                        exists: true,
                        kind: kind,
                        length: length,
                        opaqueToken: modified.map { "mtime-ns:\($0)" }
                    )
                } catch let error as CocoaError where error.code == .fileReadNoSuchFile {
                    observation = IosStorageObservation(
                        exists: false,
                        kind: nil,
                        length: nil,
                        opaqueToken: nil
                    )
                }
                _ = try client.completeStorageObservation(
                    requestId: request.requestId,
                    observation: observation
                )
            } catch {
                accessorError = error
            }
        }
        if let coordinationError {
            throw RootAccessError.coordinationFailed(coordinationError.localizedDescription)
        }
        guard accessorRan else {
            throw RootAccessError.coordinationAccessorDidNotRun
        }
        if let accessorError { throw accessorError }
    }

    private func coordinatedDelete(_ request: IosStorageRequest, rootURL: URL) throws {
        var coordinationError: NSError?
        var accessorError: Error?
        var accessorRan = false
        NSFileCoordinator(filePresenter: nil).coordinate(
            writingItemAt: rootURL,
            options: .forMerging,
            error: &coordinationError
        ) { coordinatedRoot in
            accessorRan = true
            do {
                let target = try safeTarget(root: coordinatedRoot, components: request.path)
                do {
                    try FileManager.default.removeItem(at: target)
                } catch let error as CocoaError where error.code == .fileNoSuchFile {
                    // Deletion is idempotent.
                }
                _ = client.completeStorageDelete(requestId: request.requestId)
            } catch {
                accessorError = error
            }
        }
        if let coordinationError {
            throw RootAccessError.coordinationFailed(coordinationError.localizedDescription)
        }
        guard accessorRan else {
            throw RootAccessError.coordinationAccessorDidNotRun
        }
        if let accessorError { throw accessorError }
    }

    private func safeTarget(root: URL, components: [String]) throws -> URL {
        var target = root
        for component in components {
            guard
                !component.isEmpty,
                component != ".",
                component != "..",
                !component.contains("/"),
                !component.utf8.contains(0)
            else {
                throw POSIXFailure(operation: "validate path component", code: EINVAL)
            }
            target.appendPathComponent(component, isDirectory: false)
        }
        return target
    }

    private func fail(
        _ request: IosStorageRequest,
        _ kind: IosStorageFailureKind,
        _ detail: String
    ) {
        _ = client.failStorageRequest(
            requestId: request.requestId,
            kind: kind,
            message: String(detail.prefix(1_024))
        )
    }
}

struct PlatformBridgeResourceSnapshot {
    var activeWorkers: Int
    var activeLeases: Int
}

private struct POSIXFailure: Error, LocalizedError {
    var operation: String
    var code: Int32

    var errorDescription: String? {
        "\(operation): \(String(cString: strerror(code)))"
    }
}

private enum NamespaceTransitionError: Error, LocalizedError {
    case bothPublicationSidesExist
    case stagingMissing
    case unregisteredRoot(String)
    case shareTargetIsNotFile
    case shareTargetLengthChanged

    var errorDescription: String? {
        switch self {
        case .bothPublicationSidesExist:
            return "both staging and published torrent outputs exist"
        case .stagingMissing:
            return "torrent staging output is absent"
        case .unregisteredRoot(let rootID):
            return "storage root \(rootID) is not registered with the platform bridge"
        case .shareTargetIsNotFile:
            return "the published item is not a regular file"
        case .shareTargetLengthChanged:
            return "the published file length changed after verification"
        }
    }
}

final class ShareableFileLease: Identifiable {
    let id = UUID()
    let url: URL
    private var scopedRoot: URL?

    init(url: URL, scopedRoot: URL) {
        self.url = url
        self.scopedRoot = scopedRoot
    }

    func release() {
        scopedRoot?.stopAccessingSecurityScopedResource()
        scopedRoot = nil
    }

    deinit { release() }
}

private final class CoordinatedLease: @unchecked Sendable {
    private let releaseSemaphore = DispatchSemaphore(value: 0)
    private let finishedSemaphore = DispatchSemaphore(value: 0)

    func waitForRelease() { releaseSemaphore.wait() }
    func release() { releaseSemaphore.signal() }
    func finished() { finishedSemaphore.signal() }
    func waitUntilFinished() { finishedSemaphore.wait() }
}

private final class CoordinatedLeaseLedger: @unchecked Sendable {
    private let lock = NSLock()
    private var nextID: UInt64 = 1
    private var leases: [UInt64: CoordinatedLease] = [:]

    var count: Int {
        lock.lock()
        defer { lock.unlock() }
        return leases.count
    }

    func allocate(_ lease: CoordinatedLease) -> UInt64 {
        lock.lock()
        defer { lock.unlock() }
        let id = nextID
        nextID = nextID == UInt64.max ? 1 : nextID + 1
        precondition(leases[id] == nil, "release ID wrapped into a live lease")
        leases[id] = lease
        return id
    }

    func take(_ id: UInt64) -> CoordinatedLease? {
        lock.lock()
        defer { lock.unlock() }
        return leases.removeValue(forKey: id)
    }

    func cancel(_ id: UInt64) {
        take(id)?.release()
    }

    func releaseAll() {
        lock.lock()
        let active = Array(leases.values)
        leases.removeAll()
        lock.unlock()
        active.forEach { $0.release() }
    }
}
