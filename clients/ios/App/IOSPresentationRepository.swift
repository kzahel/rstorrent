import Foundation
import RSTorrentIOS
import RSTorrentSession

struct IOSStreamPosition: Equatable {
    var epoch: String
    var sequence: UInt64
    var revision: String
}

struct IOSPieceActivity: Equatable {
    var pieceCount: UInt32
    var verified: [IndexRange]
    var active: [ActivePiece]
}

enum IOSPresentationError: Error, LocalizedError {
    case unsupportedContract(UInt16)
    case invalidSequence
    case discontinuity

    var errorDescription: String? {
        switch self {
        case .unsupportedContract(let version):
            return "Unsupported application view contract \(version)."
        case .invalidSequence:
            return "The application view returned an invalid sequence."
        case .discontinuity:
            return "The application view needs a fresh snapshot."
        }
    }
}

@MainActor
final class IOSPresentationRepository: ObservableObject {
    @Published private(set) var torrents: [TorrentView] = []
    @Published private(set) var storage: StorageSettingsSnapshot?
    @Published private(set) var error: String?
    @Published private(set) var files: [String: [FileView]] = [:]
    @Published private(set) var trackers: [String: [TrackerView]] = [:]
    @Published private(set) var peers: [String: [PeerView]] = [:]
    @Published private(set) var pieces: [String: IOSPieceActivity] = [:]

    private var positions: [String: IOSStreamPosition] = [:]
    private var task: Task<Void, Never>?
    private var subscription: IosViewSubscription?
    private var detailTask: Task<Void, Never>?
    private var detailSubscription: IosViewSubscription?
    private var onProductUpdate: (([TorrentView]) -> Void)?
    private var currentClient: IosApplicationClient?

    func start(
        client: IosApplicationClient,
        onProductUpdate: @escaping ([TorrentView]) -> Void
    ) async throws {
        stop()
        currentClient = client
        self.onProductUpdate = onProductUpdate
        let subscription = try await client.subscribe(
            spec: SubscriptionSpec(
                selector: .torrentList,
                projection: .summary,
                delivery: DeliveryPolicy(
                    minIntervalMillis: 250,
                    maxQueueBytes: 256 * 1024
                ),
                diagnostics: nil,
                catalogPage: nil
            )
        )
        self.subscription = subscription
        task = Task { [weak self] in
            guard let self else { return }
            while !Task.isCancelled, let update = await subscription.nextUpdate() {
                do {
                    try apply(update)
                } catch IOSPresentationError.discontinuity {
                    do {
                        try subscription.resync()
                    } catch {
                        self.error = error.localizedDescription
                    }
                } catch {
                    self.error = error.localizedDescription
                }
            }
        }
    }

    func stop() {
        task?.cancel()
        task = nil
        subscription = nil
        detailTask?.cancel()
        detailTask = nil
        detailSubscription = nil
        positions.removeAll()
        onProductUpdate = nil
        currentClient = nil
    }

    func present(torrentID: String, projection: ViewProjection) async {
        detailTask?.cancel()
        detailTask = nil
        detailSubscription = nil
        guard projection != .summary, let client = currentClient else { return }
        do {
            let paged = projection == .files || projection == .trackers
            let detail = try await client.subscribe(
                spec: SubscriptionSpec(
                    selector: .torrent(torrentId: torrentID),
                    projection: projection,
                    delivery: DeliveryPolicy(
                        minIntervalMillis: projection == .pieceActivity ? 100 : 250,
                        maxQueueBytes: 512 * 1024
                    ),
                    diagnostics: nil,
                    catalogPage: paged ? CatalogPageRequest(offset: 0, limit: 1_024) : nil
                )
            )
            detailSubscription = detail
            detailTask = Task { [weak self] in
                guard let self else { return }
                while !Task.isCancelled, let update = await detail.nextUpdate() {
                    do {
                        try apply(update)
                    } catch IOSPresentationError.discontinuity {
                        try? detail.resync()
                    } catch {
                        self.error = error.localizedDescription
                    }
                }
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    func clearDetail() {
        detailTask?.cancel()
        detailTask = nil
        detailSubscription = nil
    }

    func apply(_ update: ViewUpdate) throws {
        guard update.contractVersion == 2 else {
            throw IOSPresentationError.unsupportedContract(update.contractVersion)
        }
        guard let sequence = UInt64(update.sequence) else {
            throw IOSPresentationError.invalidSequence
        }
        switch update.payload {
        case .resetRequired:
            throw IOSPresentationError.discontinuity
        case .snapshot(let snapshot):
            positions[update.streamId] = IOSStreamPosition(
                epoch: update.epoch,
                sequence: sequence,
                revision: update.revision
            )
            applySnapshot(snapshot)
        case .patch(let patch):
            guard
                let current = positions[update.streamId],
                current.epoch == update.epoch,
                current.sequence < UInt64.max,
                current.sequence + 1 == sequence,
                current.revision == update.baseRevision
            else {
                throw IOSPresentationError.discontinuity
            }
            positions[update.streamId] = IOSStreamPosition(
                epoch: update.epoch,
                sequence: sequence,
                revision: update.revision
            )
            applyPatch(patch)
        }
        error = nil
    }

    private func applySnapshot(_ snapshot: ViewSnapshot) {
        switch snapshot {
        case .torrentList(let torrents, let storage, _):
            self.torrents = Self.sorted(torrents)
            self.storage = storage
            onProductUpdate?(self.torrents)
        case .torrent(let torrent):
            guard let torrent else { return }
            replaceTorrent(torrent)
        case .files(let torrentID, _, _, _, let files):
            self.files[torrentID] = files.sorted { $0.fileIndex < $1.fileIndex }
        case .trackers(let torrentID, _, _, let trackers):
            self.trackers[torrentID] = trackers.sorted(by: Self.trackerOrder)
        case .peers(let torrentID, let peers):
            self.peers[torrentID] = peers.sorted { $0.connectionId < $1.connectionId }
        case .pieceActivity(let torrentID, let count, let verified, let active):
            pieces[torrentID] = IOSPieceActivity(
                pieceCount: count,
                verified: verified,
                active: active
            )
        default:
            return
        }
    }

    private func applyPatch(_ patch: ViewPatch) {
        switch patch {
        case .torrentList(let upsert, let removed, let storage, _):
            var values = Dictionary(uniqueKeysWithValues: torrents.map { ($0.torrentId, $0) })
            removed.forEach { values.removeValue(forKey: $0) }
            upsert.forEach { values[$0.torrentId] = $0 }
            torrents = Self.sorted(Array(values.values))
            if let storage { self.storage = storage }
            onProductUpdate?(torrents)
        case .torrent(let torrent):
            guard let torrent else { return }
            replaceTorrent(torrent)
        case .files(let torrentID, let upsert, let removed):
            var values = Dictionary(
                uniqueKeysWithValues: files[torrentID, default: []].map { ($0.fileId, $0) }
            )
            removed.forEach { values.removeValue(forKey: $0) }
            upsert.forEach { values[$0.fileId] = $0 }
            files[torrentID] = values.values.sorted { $0.fileIndex < $1.fileIndex }
        case .trackers(let torrentID, let upsert, let removed):
            var values = Dictionary(
                uniqueKeysWithValues: trackers[torrentID, default: []].map { ($0.trackerId, $0) }
            )
            removed.forEach { values.removeValue(forKey: $0) }
            upsert.forEach { values[$0.trackerId] = $0 }
            trackers[torrentID] = values.values.sorted(by: Self.trackerOrder)
        case .peers(let torrentID, let upsert, let removed):
            var values = Dictionary(
                uniqueKeysWithValues: peers[torrentID, default: []].map { ($0.connectionId, $0) }
            )
            removed.forEach { values.removeValue(forKey: $0) }
            upsert.forEach { values[$0.connectionId] = $0 }
            peers[torrentID] = values.values.sorted { $0.connectionId < $1.connectionId }
        case .pieceActivity(
            let torrentID,
            let count,
            let verified,
            let cleared,
            let activeUpsert,
            let activeRemoved
        ):
            var ranges = pieces[torrentID]?.verified ?? []
            cleared.forEach { ranges = Self.remove($0, from: ranges) }
            verified.forEach { ranges = Self.insert($0, into: ranges) }
            var active = Dictionary(
                uniqueKeysWithValues: (pieces[torrentID]?.active ?? []).map { ($0.pieceId, $0) }
            )
            activeRemoved.forEach { active.removeValue(forKey: $0) }
            activeUpsert.forEach { active[$0.pieceId] = $0 }
            pieces[torrentID] = IOSPieceActivity(
                pieceCount: count,
                verified: ranges,
                active: active.values.sorted { $0.pieceIndex < $1.pieceIndex }
            )
        default:
            return
        }
    }

    private func replaceTorrent(_ torrent: TorrentView) {
        var values = Dictionary(uniqueKeysWithValues: torrents.map { ($0.torrentId, $0) })
        values[torrent.torrentId] = torrent
        torrents = Self.sorted(Array(values.values))
        onProductUpdate?(torrents)
    }

    private static func insert(_ inserted: IndexRange, into ranges: [IndexRange]) -> [IndexRange] {
        var start = inserted.start
        var end = inserted.endExclusive
        var output: [IndexRange] = []
        var placed = false
        for range in ranges {
            if range.endExclusive < start {
                output.append(range)
            } else if end < range.start {
                if !placed {
                    output.append(IndexRange(start: start, endExclusive: end))
                    placed = true
                }
                output.append(range)
            } else {
                start = min(start, range.start)
                end = max(end, range.endExclusive)
            }
        }
        if !placed { output.append(IndexRange(start: start, endExclusive: end)) }
        return output
    }

    private static func remove(_ removed: IndexRange, from ranges: [IndexRange]) -> [IndexRange] {
        ranges.flatMap { range -> [IndexRange] in
            if range.endExclusive <= removed.start || range.start >= removed.endExclusive {
                return [range]
            }
            var fragments: [IndexRange] = []
            if range.start < removed.start {
                fragments.append(IndexRange(start: range.start, endExclusive: removed.start))
            }
            if range.endExclusive > removed.endExclusive {
                fragments.append(
                    IndexRange(start: removed.endExclusive, endExclusive: range.endExclusive)
                )
            }
            return fragments
        }
    }

    private static func sorted(_ torrents: [TorrentView]) -> [TorrentView] {
        torrents.sorted {
            let left = $0.downloadQueuePosition ?? UInt32.max
            let right = $1.downloadQueuePosition ?? UInt32.max
            if left != right { return left < right }
            return ($0.displayName ?? $0.torrentId)
                .localizedStandardCompare($1.displayName ?? $1.torrentId) == .orderedAscending
        }
    }

    private static func trackerOrder(_ first: TrackerView, _ second: TrackerView) -> Bool {
        if first.tier != second.tier { return first.tier < second.tier }
        return first.url < second.url
    }
}
