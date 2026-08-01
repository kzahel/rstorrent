import { useMemo } from "react";

import { useInspectionStore } from "../context";
import { formatBytes, formatProgress, formatRate } from "../format";
import type { PeerRow, ViewMaterialization } from "../model";
import { VirtualTable, type VirtualColumn } from "./VirtualTable";
import styles from "./PeerTable.module.css";

const COLUMNS: readonly VirtualColumn<PeerRow>[] = [
  {
    id: "state",
    label: "State",
    width: 104,
    sortable: true,
    sortValue: (row) => row.state,
    sortOrder: ["connected", "choked", "stalled", "handshaking", "connecting", "disconnecting"],
    render: (row) => (
      <span className={styles.state} data-state={row.state}>
        <span aria-hidden="true" />
        {row.state}
      </span>
    ),
  },
  {
    id: "endpoint",
    label: "Address",
    width: 188,
    minimumViewport: 650,
    sortable: true,
    sortValue: (row) => row.endpoint,
    render: (row) => <span title={row.endpoint}>{row.endpoint}</span>,
  },
  {
    id: "client",
    label: "Client",
    width: 154,
    sortable: true,
    sortValue: (row) => row.client,
    render: (row) => <span title={row.client ?? undefined}>{row.client ?? "—"}</span>,
  },
  {
    id: "source",
    label: "Source",
    width: 74,
    minimumViewport: 920,
    sortable: true,
    sortValue: (row) => row.source,
    render: (row) => row.source.toUpperCase(),
  },
  {
    id: "progress",
    label: "%",
    width: 64,
    minimumViewport: 500,
    align: "right",
    sortable: true,
    sortValue: (row) => row.progress,
    sortKind: "number",
    render: (row) => (row.progress === null ? "—" : formatProgress(row.progress)),
  },
  {
    id: "down",
    label: "Down",
    width: 96,
    align: "right",
    sortable: true,
    sortValue: (row) => row.downloadRate,
    sortKind: "number",
    render: (row) => formatRate(row.downloadRate),
  },
  {
    id: "downloaded",
    label: "Downloaded",
    width: 104,
    minimumViewport: 1060,
    align: "right",
    sortable: true,
    sortValue: (row) => row.downloadedBytes,
    sortKind: "number",
    render: (row) => formatBytes(row.downloadedBytes),
  },
  {
    id: "requests",
    label: "Reqs",
    width: 62,
    align: "right",
    sortable: true,
    sortValue: (row) => row.requestsPending,
    sortKind: "number",
    render: (row) => row.requestsPending ?? "—",
  },
  {
    id: "age",
    label: "Oldest",
    width: 78,
    minimumViewport: 790,
    align: "right",
    sortable: true,
    sortValue: (row) => row.oldestRequestMs,
    sortKind: "number",
    render: (row) =>
      row.oldestRequestMs === null
        ? "—"
        : row.oldestRequestMs >= 1000
          ? `${(row.oldestRequestMs / 1000).toFixed(1)}s`
          : `${row.oldestRequestMs}ms`,
  },
  {
    id: "flags",
    label: "Flags",
    width: 68,
    minimumViewport: 560,
    align: "center",
    sortable: true,
    sortValue: (row) => row.flags,
    render: (row) => <code className={styles.flags}>{row.flags || "—"}</code>,
  },
];

export function PeerTable({ torrentId }: { readonly torrentId: string }) {
  const peerSet = useInspectionStore(
    (state) => state.peersByTorrent[torrentId],
  );
  const selectedPeerId = useInspectionStore(
    (state) => state.presentation.selectedPeerId,
  );
  const selectPeer = useInspectionStore((state) => state.selectPeer);
  const materialization = useInspectionStore((state) => state.viewStatus.peers);
  const rows = useMemo(
    () =>
      (peerSet?.order ?? [])
        .map((id) => peerSet?.rows[id])
        .filter((row): row is PeerRow => row !== undefined),
    [peerSet],
  );

  return (
    <VirtualTable
      tableId="peers"
      label="Active peer connections"
      rows={rows}
      getRowId={(row) => row.connectionId}
      columns={COLUMNS}
      selectedId={selectedPeerId}
      onSelect={(row) => selectPeer(row.connectionId)}
      emptyMessage={peerEmptyMessage(materialization)}
    />
  );
}

function peerEmptyMessage(materialization: ViewMaterialization): string {
  switch (materialization.status) {
    case "not_requested":
      return "Peer inspection is not requested.";
    case "loading":
      return "Loading active peer connections…";
    case "unavailable":
    case "unsupported":
    case "stale":
      return materialization.reason;
    case "ready":
      return "No peer connections are currently active.";
  }
}
