import { useMemo } from "react";

import { useInspectionStore } from "../context";
import { formatBytes, formatProgress, formatRate } from "../format";
import type { PeerRow } from "../model";
import { VirtualTable, type VirtualColumn } from "./VirtualTable";
import styles from "./PeerTable.module.css";

const COLUMNS: readonly VirtualColumn<PeerRow>[] = [
  {
    id: "state",
    label: "State",
    width: 104,
    sortable: true,
    sortValue: (row) => row.state,
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
    render: (row) => <span title={row.client}>{row.client}</span>,
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
    render: (row) => formatProgress(row.progress),
  },
  {
    id: "down",
    label: "Down",
    width: 96,
    align: "right",
    sortable: true,
    sortValue: (row) => row.downloadRate,
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
    render: (row) => formatBytes(row.downloadedBytes),
  },
  {
    id: "requests",
    label: "Reqs",
    width: 62,
    align: "right",
    sortable: true,
    sortValue: (row) => row.requestsPending,
    render: (row) => row.requestsPending || "—",
  },
  {
    id: "age",
    label: "Oldest",
    width: 78,
    minimumViewport: 790,
    align: "right",
    sortable: true,
    sortValue: (row) => row.oldestRequestMs,
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
  const rows = useMemo(
    () =>
      (peerSet?.order ?? [])
        .map((id) => peerSet?.rows[id])
        .filter((row): row is PeerRow => row !== undefined),
    [peerSet],
  );

  return (
    <VirtualTable
      label="Connected and candidate peers"
      rows={rows}
      getRowId={(row) => row.connectionId}
      columns={COLUMNS}
      selectedId={selectedPeerId}
      onSelect={(row) => selectPeer(row.connectionId)}
      emptyMessage="No peer rows are available for this demo state."
      initialSort={{ columnId: "down", direction: "desc" }}
    />
  );
}
