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
    var name: String { value.displayName ?? value.sourceDisplayName ?? "" }
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
    var isComplete: Bool {
        value.state == .complete
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

func torrentDisplayProgress(
    state: TorrentState,
    storageState: StorageState,
    requiredPayloadBytes: String?,
    remainingPayloadBytes: String?,
    pieceCount: UInt32,
    verifiedPieceCount: UInt32
) -> Double {
    if state == .complete {
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
    case "stopped": return String(localized: "torrent_status_stopped")
    case "starting": return String(localized: "engine_status_starting")
    case "downloading": return String(localized: "torrent_status_downloading")
    case "downloading_metadata": return String(localized: "torrent_status_downloading_metadata")
    case "checking": return String(localized: "torrent_status_checking")
    case "seeding": return String(localized: "torrent_status_seeding")
    case "done": return String(localized: "torrent_status_done")
    case "queued": return String(localized: "torrent_status_queued")
    case "error": return String(localized: "torrent_status_error")
    default: return String(localized: "torrent_status_error")
    }
}

func torrentDisplayName(_ torrent: TorrentListItem) -> String {
    torrent.name.isEmpty ? String(localized: "component_torrent_card_unknown_name") : torrent.name
}

func formattedProgress(_ progress: Double) -> String {
    guard progress.isFinite else {
        return 0.formatted(.percent.precision(.fractionLength(0)))
    }
    let bounded = min(max(progress, 0), 1)
    let rounded = Int((bounded * 100).rounded())
    let percentage = bounded < 1 ? min(rounded, 99) : 100
    return (Double(percentage) / 100).formatted(.percent.precision(.fractionLength(0)))
}

func formattedBytesPerSecond(_ value: Int) -> String {
    let formatted = speedFormatter.string(fromByteCount: Int64(max(value, 0)))
    return String(format: String(localized: "ios_speed_value"), locale: Locale.current, formatted)
}

func formattedByteCount(_ value: UInt64?) -> String {
    guard let value, value <= UInt64(Int64.max) else { return String(localized: "tab_details_unknown") }
    return byteCountFormatter.string(fromByteCount: Int64(value))
}

func formattedCountRatio(_ completed: UInt32, _ total: UInt32) -> String {
    String(
        format: String(localized: "ios_count_ratio"),
        locale: .current,
        completed.formatted(),
        total.formatted()
    )
}

func localizedPeerCount(_ count: Int) -> String {
    String(
        localized: LocalizedStringResource(
            "ios_peer_count",
            defaultValue: "\(count) peers",
            comment: "Peer count shown in a torrent row."
        )
    )
}

func localizedVerifiedPieces(_ completed: UInt32, _ total: UInt32) -> String {
    String(
        format: String(localized: "ios_pieces_verified"),
        locale: .current,
        completed.formatted(),
        total.formatted()
    )
}

func enumLabel(_ value: StorageState) -> String {
    switch value {
    case .available: return String(localized: "ios_storage_state_available")
    case .unavailable: return String(localized: "ios_storage_state_unavailable")
    case .needsRepair: return String(localized: "ios_storage_state_needs_repair")
    }
}

func enumLabel(_ value: MediaFileAvailability) -> String {
    switch value {
    case .available: return String(localized: "ios_media_available")
    case .streamable: return String(localized: "ios_media_streamable")
    case .metadataUnavailable: return String(localized: "ios_media_metadata_unavailable")
    case .invalidFile: return String(localized: "ios_media_invalid_file")
    case .padding: return String(localized: "ios_media_padding")
    case .incomplete: return String(localized: "ios_media_incomplete")
    case .checking: return String(localized: "ios_media_checking")
    case .unverified: return String(localized: "ios_media_unverified")
    case .storageUnavailable: return String(localized: "ios_media_storage_unavailable")
    case .removing: return String(localized: "ios_media_removing")
    case .serverUnavailable: return String(localized: "ios_media_server_unavailable")
    case .resourceLimit: return String(localized: "ios_media_resource_limit")
    }
}

func enumLabel(_ value: TrackerStatusView) -> String {
    switch value {
    case .unsupported: return String(localized: "ios_tracker_status_unsupported")
    case .inactive: return String(localized: "ios_tracker_status_inactive")
    case .disabled: return String(localized: "ios_tracker_status_disabled")
    case .idle: return String(localized: "ios_tracker_status_idle")
    case .announcing: return String(localized: "ios_tracker_status_announcing")
    case .retryWait: return String(localized: "ios_tracker_status_retry_wait")
    case .reannounceWait: return String(localized: "ios_tracker_status_reannounce_wait")
    }
}

func enumLabel(_ value: PeerDirection) -> String {
    switch value {
    case .incoming: return String(localized: "ios_peer_direction_incoming")
    case .outgoing: return String(localized: "ios_peer_direction_outgoing")
    }
}

func enumLabel(_ value: PeerTransportKind) -> String {
    switch value {
    case .tcp: return "TCP"
    case .utp: return "µTP"
    }
}

func enumLabel(_ value: PeerRole) -> String {
    switch value {
    case .metadata: return String(localized: "ios_peer_role_metadata")
    case .content: return String(localized: "ios_peer_role_content")
    }
}
