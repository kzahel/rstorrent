import Foundation
import RSTorrentSession

private let speedFormatter: ByteCountFormatter = {
    let formatter = ByteCountFormatter()
    formatter.allowedUnits = [.useBytes, .useKB, .useMB, .useGB]
    formatter.countStyle = .file
    formatter.includesUnit = true
    formatter.isAdaptive = true
    return formatter
}()

private let byteCountFormatter: ByteCountFormatter = {
    let formatter = ByteCountFormatter()
    formatter.allowedUnits = [.useBytes, .useKB, .useMB, .useGB, .useTB]
    formatter.countStyle = .file
    formatter.includesUnit = true
    formatter.isAdaptive = true
    return formatter
}()

struct TorrentListItem: Identifiable, Equatable {
    let value: TorrentView

    var id: String { value.torrentId }
    var infoHash: String { value.protocolIdentities.v1 ?? value.torrentId }
    var name: String { value.displayName ?? "" }
    var status: String {
        switch value.operationalState {
        case .queued: return "queued"
        case .starting: return value.metadataAvailable ? "starting" : "downloading_metadata"
        case .downloading: return "downloading"
        case .checking: return "checking"
        case .stopping, .paused: return "stopped"
        case .seeding: return "seeding"
        case .error: return "error"
        }
    }
    var isStopped: Bool {
        value.operationalState == .paused || value.operationalState == .error
    }
    var isPublishedComplete: Bool {
        torrentIsPublishedComplete(state: value.state, storageState: value.storageState)
    }
    var progress: Double {
        torrentDisplayProgress(
            state: value.state,
            storageState: value.storageState,
            requiredPayloadBytes: value.requiredPayloadBytes,
            remainingPayloadBytes: value.remainingPayloadBytes,
            pieceCount: value.pieceCount,
            verifiedPieceCount: value.verifiedPieceCount
        )
    }
    var downloadSpeed: Int { Int(value.payloadDownloadRateBytes) ?? 0 }
    var numPeers: Int { Int(value.activePeerConnections) }
}

func torrentIsPublishedComplete(state: TorrentState, storageState: StorageState) -> Bool {
    state == .complete && storageState == .published
}

func torrentDisplayProgress(
    state: TorrentState,
    storageState: StorageState,
    requiredPayloadBytes: String?,
    remainingPayloadBytes: String?,
    pieceCount: UInt32,
    verifiedPieceCount: UInt32
) -> Double {
    if torrentIsPublishedComplete(state: state, storageState: storageState) {
        return 1
    }

    let fraction: Double
    if
        let requiredText = requiredPayloadBytes,
        let remainingText = remainingPayloadBytes,
        let required = Double(requiredText),
        let remaining = Double(remainingText),
        required.isFinite,
        remaining.isFinite,
        required > 0
    {
        fraction = (required - remaining) / required
    } else if pieceCount > 0 {
        fraction = Double(verifiedPieceCount) / Double(pieceCount)
    } else {
        fraction = 0
    }

    return min(max(fraction, 0), 0.99)
}

func localizedTorrentStatus(_ status: String) -> String {
    switch status {
    case "stopped": return L10n.string("torrent_status_stopped")
    case "starting": return L10n.string("engine_status_starting")
    case "downloading": return L10n.string("torrent_status_downloading")
    case "downloading_metadata": return L10n.string("torrent_status_downloading_metadata")
    case "checking": return L10n.string("torrent_status_checking")
    case "seeding": return L10n.string("torrent_status_seeding")
    case "done": return L10n.string("torrent_status_done")
    case "queued": return L10n.string("torrent_status_queued")
    case "error": return L10n.string("torrent_status_error")
    default: return status.replacingOccurrences(of: "_", with: " ").localizedCapitalized
    }
}

func torrentDisplayName(_ torrent: TorrentListItem) -> String {
    torrent.name.isEmpty ? L10n.string("component_torrent_card_unknown_name") : torrent.name
}

func formattedProgress(_ progress: Double) -> String {
    guard progress.isFinite else { return "0%" }
    let bounded = min(max(progress, 0), 1)
    let rounded = Int((bounded * 100).rounded())
    return "\(bounded < 1 ? min(rounded, 99) : 100)%"
}

func formattedBytesPerSecond(_ value: Int) -> String {
    let formatted = speedFormatter.string(fromByteCount: Int64(max(value, 0)))
    return L10n.formatted("ios_speed_value", formatted)
}

func formattedByteCount(_ value: UInt64?) -> String {
    guard let value, value <= UInt64(Int64.max) else { return L10n.string("tab_details_unknown") }
    return byteCountFormatter.string(fromByteCount: Int64(value))
}

func enumLabel<T>(_ value: T) -> String {
    String(describing: value)
        .replacingOccurrences(of: "_", with: " ")
        .localizedCapitalized
}
