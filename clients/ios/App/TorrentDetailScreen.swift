import SwiftUI
import RSTorrentSession

private enum TorrentDetailSection: CaseIterable, Identifiable {
    case status
    case files
    case trackers
    case peers
    case pieces

    var id: Self { self }
    var titleKey: String {
        switch self {
        case .status: return "tab_status"
        case .files: return "tab_files"
        case .trackers: return "tab_trackers"
        case .peers: return "tab_peers"
        case .pieces: return "tab_pieces"
        }
    }
    var projection: ViewProjection {
        switch self {
        case .status: return .summary
        case .files: return .files
        case .trackers: return .trackers
        case .peers: return .peers
        case .pieces: return .pieceActivity
        }
    }
}

struct TorrentDetailScreen: View {
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var appModel: AppModel
    @ObservedObject var presentation: IOSPresentationRepository
    let torrentID: String

    @State private var selectedSection: TorrentDetailSection = .status
    @State private var pendingRemovalTorrent: TorrentListItem?
    @State private var actionError: String?
    @State private var previewLease: ShareableFileLease?

    private var torrent: TorrentListItem? {
        presentation.torrents.first { $0.torrentId == torrentID }.map(TorrentListItem.init)
    }

    var body: some View {
        Group {
            if let torrent {
                ScrollView {
                    VStack(alignment: .leading, spacing: 20) {
                        TorrentOverviewCard(torrent: torrent)
                        DetailSectionPicker(selectedSection: $selectedSection)
                        if let actionError {
                            Text(actionError).foregroundStyle(.red).font(.footnote)
                        }
                        detailSectionContent(torrent)
                    }
                    .padding(20)
                }
                .navigationTitle(torrentDisplayName(torrent))
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItemGroup(placement: .navigationBarTrailing) {
                        Button { toggle(torrent) } label: {
                            Image(systemName: torrent.isStopped ? "play.fill" : "pause.fill")
                        }
                        .accessibilityLabel(
                            torrent.isStopped
                                ? L10n.string("torrent_detail_resume_button")
                                : L10n.string("torrent_detail_pause_button")
                        )
                        Button(role: .destructive) { pendingRemovalTorrent = torrent } label: {
                            Image(systemName: "trash")
                        }
                        .accessibilityLabel(L10n.string("torrent_detail_remove_button"))
                    }
                }
                .task(id: selectedSection) {
                    await presentation.present(
                        torrentID: torrentID,
                        projection: selectedSection.projection
                    )
                }
                .onDisappear { presentation.clearDetail() }
                .sheet(item: $pendingRemovalTorrent) { torrent in
                    RemoveTorrentSheet(
                        torrent: torrent,
                        onConfirm: { deleteFiles in
                            command(
                                .removeTorrent(
                                    torrentId: torrent.id,
                                    data: deleteFiles ? .deleteManaged : .keep
                                ),
                                dismissAfter: true
                            )
                            pendingRemovalTorrent = nil
                        },
                        onCancel: { pendingRemovalTorrent = nil }
                    )
                }
                .fullScreenCover(item: $previewLease, onDismiss: {
                    previewLease = nil
                }) { lease in
                    SystemFilePreview(url: lease.url) {
                        previewLease = nil
                    }
                    .onDisappear { lease.release() }
                }
            } else {
                VStack(spacing: 12) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.largeTitle)
                        .foregroundStyle(.secondary)
                    Text(L10n.string("torrent_detail_error_title"))
                        .font(.headline)
                    Text(torrentID)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
                .padding()
            }
        }
    }

    @ViewBuilder
    private func detailSectionContent(_ torrent: TorrentListItem) -> some View {
        switch selectedSection {
        case .status:
            TorrentStatusSection(
                torrent: torrent,
                onForceRecheck: { command(.forceRecheck(torrentId: torrent.id)) }
            )
        case .files:
            TorrentFilesSection(
                files: presentation.files[torrentID] ?? [],
                onPriority: { indices, priority in
                    command(
                        .setFilePriority(
                            torrentId: torrentID,
                            fileIndices: indices,
                            priority: priority
                        )
                    )
                },
                onDownloadNow: { index in
                    command(.downloadFiles(torrentId: torrentID, fileIndices: [index]))
                },
                onOpen: { file in
                    Task {
                        do {
                            previewLease = try await appModel.shareableFile(
                                torrentID: torrentID,
                                fileIndex: file.fileIndex
                            )
                            actionError = nil
                        } catch {
                            actionError = error.localizedDescription
                        }
                    }
                }
            )
        case .trackers:
            TorrentTrackersSection(trackers: presentation.trackers[torrentID] ?? [])
        case .peers:
            TorrentPeersSection(peers: presentation.peers[torrentID] ?? [])
        case .pieces:
            TorrentPiecesSection(activity: presentation.pieces[torrentID])
        }
    }

    private func toggle(_ torrent: TorrentListItem) {
        command(
            torrent.isStopped
                ? .resume(torrentId: torrent.id) : .pause(torrentId: torrent.id)
        )
    }

    private func command(_ command: Command, dismissAfter: Bool = false) {
        Task {
            do {
                _ = try await appModel.dispatch(command)
                actionError = nil
                if dismissAfter { dismiss() }
            } catch {
                actionError = error.localizedDescription
            }
        }
    }
}

private struct TorrentOverviewCard: View {
    let torrent: TorrentListItem
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(localizedTorrentStatus(torrent.status)).font(.headline)
                    Text(torrentDisplayName(torrent))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
                Spacer()
                Text(formattedProgress(torrent.progress))
                    .font(.title3.monospacedDigit().weight(.semibold))
            }
            ProgressView(value: min(max(torrent.progress, 0), 1))
            HStack(spacing: 12) {
                OverviewPill(
                    title: L10n.string("ios_torrent_row_download_label"),
                    value: formattedBytesPerSecond(torrent.downloadSpeed)
                )
                OverviewPill(
                    title: L10n.string("ios_torrent_row_upload_label"),
                    value: "—"
                )
                OverviewPill(
                    title: L10n.string("ios_torrent_row_peers_label"),
                    value: String(torrent.numPeers)
                )
            }
        }
        .padding(18)
        .background(
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .fill(Color(.secondarySystemGroupedBackground))
        )
    }
}

private struct OverviewPill: View {
    let title: String
    let value: String
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.caption).foregroundStyle(.secondary)
            Text(value).font(.subheadline.monospacedDigit())
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(Color(.systemBackground))
        )
    }
}

private struct DetailSectionPicker: View {
    @Binding var selectedSection: TorrentDetailSection
    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(TorrentDetailSection.allCases) { section in
                    Button { selectedSection = section } label: {
                        Text(L10n.string(section.titleKey))
                            .font(.subheadline.weight(.semibold))
                            .padding(.horizontal, 14)
                            .padding(.vertical, 8)
                            .background(
                                Capsule(style: .continuous).fill(
                                    section == selectedSection
                                        ? Color.accentColor : Color(.secondarySystemFill)
                                )
                            )
                            .foregroundStyle(section == selectedSection ? .white : .primary)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }
}

private struct TorrentStatusSection: View {
    let torrent: TorrentListItem
    let onForceRecheck: () -> Void
    private let columns = [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)]

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(L10n.string("tab_status")).font(.headline)
            LazyVGrid(columns: columns, spacing: 12) {
                DetailMetricCard(
                    title: L10n.string("tab_status_download_label"),
                    value: formattedBytesPerSecond(torrent.downloadSpeed)
                )
                DetailMetricCard(
                    title: L10n.string("tab_status_connected_peers_label"),
                    value: String(torrent.numPeers)
                )
                DetailMetricCard(
                    title: L10n.string("tab_pieces_progress"),
                    value: formattedProgress(torrent.progress)
                )
                DetailMetricCard(
                    title: L10n.string("tab_details_piece_count"),
                    value: "\(torrent.value.verifiedPieceCount) / \(torrent.value.pieceCount)"
                )
            }
            DetailFactRow(
                title: L10n.string("tab_details_save_location"),
                value: enumLabel(torrent.value.storageState)
            )
            if let v1 = torrent.value.protocolIdentities.v1,
               let v2 = torrent.value.protocolIdentities.v2 {
                DetailFactRow(
                    title: L10n.string("tab_details_info_hash_v1"),
                    value: v1
                )
                DetailFactRow(
                    title: L10n.string("tab_details_info_hash_v2"),
                    value: v2
                )
            } else {
                DetailFactRow(
                    title: L10n.string("tab_details_info_hash"),
                    value: torrent.infoHash
                )
            }
            if let error = torrent.value.error {
                DetailFactRow(title: L10n.string("torrent_list_error"), value: error)
            }
            if torrent.value.forceRecheckAvailable {
                Button(action: onForceRecheck) {
                    Label("Force recheck", systemImage: "checkmark.arrow.trianglehead.counterclockwise")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
            }
        }
        .detailCard()
    }
}

private struct TorrentFilesSection: View {
    let files: [FileView]
    let onPriority: ([UInt32], FilePriority) -> Void
    let onDownloadNow: (UInt32) -> Void
    let onOpen: (FileView) -> Void

    var body: some View {
        if files.isEmpty {
            DetailPlaceholderCard(
                title: L10n.string("tab_files_empty_title"),
                message: L10n.string("tab_files_empty_description")
            )
        } else {
            VStack(alignment: .leading, spacing: 16) {
                Text(L10n.string("tab_files")).font(.headline)
                ForEach(files, id: \.fileId) { file in
                    HStack(alignment: .top, spacing: 12) {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(file.path.last ?? file.fileId).font(.subheadline.weight(.semibold))
                            Text(file.path.joined(separator: "/"))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(2)
                            let done = UInt64(file.doneBytes) ?? 0
                            let length = UInt64(file.lengthBytes) ?? 0
                            ProgressView(value: length == 0 ? 0 : Double(done) / Double(length))
                            HStack {
                                Text(formattedByteCount(length))
                                Spacer()
                                Text(enumLabel(file.mediaAvailability))
                            }
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                        }
                        Menu {
                            Button(L10n.string("file_priority_normal")) {
                                onPriority([file.fileIndex], .normal)
                            }
                            Button(L10n.string("file_priority_skip")) {
                                onPriority([file.fileIndex], .skip)
                            }
                            if file.selection == .skipped {
                                Button("Download now") { onDownloadNow(file.fileIndex) }
                            }
                            if file.mediaAvailability == .available {
                                Button(L10n.string("torrent_detail_open_with")) {
                                    onOpen(file)
                                }
                            }
                        } label: {
                            Image(systemName: "ellipsis.circle")
                        }
                    }
                    if file.fileId != files.last?.fileId { Divider() }
                }
            }
            .detailCard()
        }
    }
}

private struct TorrentTrackersSection: View {
    let trackers: [TrackerView]
    var body: some View {
        if trackers.isEmpty {
            DetailPlaceholderCard(
                title: L10n.string("tab_trackers_empty_title"),
                message: L10n.string("tab_trackers_empty_description")
            )
        } else {
            VStack(alignment: .leading, spacing: 14) {
                Text(L10n.string("tab_trackers")).font(.headline)
                ForEach(trackers, id: \.trackerId) { tracker in
                    VStack(alignment: .leading, spacing: 5) {
                        Text(tracker.url).font(.subheadline).textSelection(.enabled)
                        HStack {
                            Label(enumLabel(tracker.status), systemImage: "antenna.radiowaves.left.and.right")
                            Spacer()
                            Text("Tier \(tracker.tier)")
                        }
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        if let error = tracker.lastError {
                            Text(error).font(.caption2).foregroundStyle(.red)
                        }
                    }
                    if tracker.trackerId != trackers.last?.trackerId { Divider() }
                }
            }
            .detailCard()
        }
    }
}

private struct TorrentPeersSection: View {
    let peers: [PeerView]
    var body: some View {
        if peers.isEmpty {
            DetailPlaceholderCard(
                title: L10n.string("tab_peers_empty_title"),
                message: L10n.string("tab_peers_empty_description")
            )
        } else {
            VStack(alignment: .leading, spacing: 14) {
                Text(L10n.string("tab_peers")).font(.headline)
                ForEach(peers, id: \.connectionId) { peer in
                    VStack(alignment: .leading, spacing: 5) {
                        Text(peer.clientName ?? peer.remoteEndpoint).font(.subheadline.weight(.semibold))
                        Text(peer.remoteEndpoint).font(.caption.monospaced()).foregroundStyle(.secondary)
                        HStack {
                            Text(enumLabel(peer.direction))
                            Text(enumLabel(peer.transport))
                            Text(enumLabel(peer.role))
                            Spacer()
                            Text(formattedBytesPerSecond(Int(peer.payloadDownloadRateBytes ?? "0") ?? 0))
                        }
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                    }
                    if peer.connectionId != peers.last?.connectionId { Divider() }
                }
            }
            .detailCard()
        }
    }
}

private struct TorrentPiecesSection: View {
    let activity: IOSPieceActivity?
    var body: some View {
        if let activity, activity.pieceCount > 0 {
            VStack(alignment: .leading, spacing: 16) {
                Text(L10n.string("tab_pieces")).font(.headline)
                PieceMapView(activity: activity)
                    .frame(height: 180)
                    .accessibilityLabel(
                        "\(verifiedCount(activity)) of \(activity.pieceCount) pieces verified"
                    )
                DetailFactRow(
                    title: L10n.string("tab_pieces_progress"),
                    value: "\(verifiedCount(activity)) / \(activity.pieceCount)"
                )
            }
            .detailCard()
        } else {
            DetailPlaceholderCard(
                title: L10n.string("tab_pieces_empty_title"),
                message: L10n.string("tab_pieces_empty_description")
            )
        }
    }

    private func verifiedCount(_ activity: IOSPieceActivity) -> UInt32 {
        activity.verified.reduce(0) { $0 + ($1.endExclusive - $1.start) }
    }
}

private struct PieceMapView: View {
    let activity: IOSPieceActivity
    var body: some View {
        Canvas { context, size in
            let columns = max(Int(size.width / 7), 1)
            let rows = Int(ceil(Double(activity.pieceCount) / Double(columns)))
            let cellWidth = size.width / CGFloat(columns)
            let cellHeight = size.height / CGFloat(max(rows, 1))
            let active = Set(activity.active.map(\.pieceIndex))
            for index in 0..<activity.pieceCount {
                let column = Int(index) % columns
                let row = Int(index) / columns
                let rect = CGRect(
                    x: CGFloat(column) * cellWidth + 0.5,
                    y: CGFloat(row) * cellHeight + 0.5,
                    width: max(cellWidth - 1, 1),
                    height: max(cellHeight - 1, 1)
                )
                let verified = activity.verified.contains {
                    index >= $0.start && index < $0.endExclusive
                }
                context.fill(
                    Path(roundedRect: rect, cornerRadius: 1),
                    with: .color(verified ? .green : active.contains(index) ? .blue : .gray.opacity(0.25))
                )
            }
        }
    }
}

private struct DetailMetricCard: View {
    let title: String
    let value: String
    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(title).font(.caption).foregroundStyle(.secondary)
            Text(value).font(.body.monospacedDigit().weight(.semibold))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(Color(.systemBackground))
        )
    }
}

private struct DetailFactRow: View {
    let title: String
    let value: String
    var body: some View {
        HStack(alignment: .top) {
            Text(title).foregroundStyle(.secondary)
            Spacer()
            Text(value).multilineTextAlignment(.trailing).textSelection(.enabled)
        }
        .font(.subheadline)
    }
}

private struct DetailPlaceholderCard: View {
    let title: String
    let message: String
    var body: some View {
        VStack(spacing: 8) {
            Text(title).font(.headline)
            Text(message).font(.subheadline).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .detailCard()
    }
}

private extension View {
    func detailCard() -> some View {
        padding(18)
            .background(
                RoundedRectangle(cornerRadius: 20, style: .continuous)
                    .fill(Color(.secondarySystemGroupedBackground))
            )
    }
}
