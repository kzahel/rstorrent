import { useMemo } from "react";

import { useInspectionStore } from "../context";
import {
  formatBytes,
  formatEta,
  formatProgress,
  formatRate,
} from "../format";
import type { TorrentRow, ViewMaterialization } from "../model";
import { torrentMatchesCategory } from "../state";
import { VirtualTable, type VirtualColumn } from "./VirtualTable";
import styles from "./TorrentTable.module.css";

const COLUMNS: readonly VirtualColumn<TorrentRow>[] = [
  {
    id: "name",
    label: "Name",
    width: 330,
    sortable: true,
    sortValue: (row) => row.name,
    render: (row) => (
      <span className={styles.name} title={row.name}>
        <span className={styles.stateDot} data-status={row.status} aria-hidden="true" />
        <span>{row.name}</span>
      </span>
    ),
  },
  {
    id: "size",
    label: "Size",
    width: 92,
    minimumViewport: 780,
    align: "right",
    sortable: true,
    sortValue: (row) => row.sizeBytes,
    render: (row) => formatBytes(row.sizeBytes),
  },
  {
    id: "progress",
    label: "Done",
    width: 122,
    align: "right",
    sortable: true,
    sortValue: (row) => row.progress,
    render: (row) => (
      <span className={styles.progressCell}>
        <span className={styles.progressLabel}>{formatProgress(row.progress)}</span>
        <span className={styles.progressTrack} aria-hidden="true">
          <span style={{ width: `${Math.round((row.progress ?? 0) * 100)}%` }} />
        </span>
      </span>
    ),
  },
  {
    id: "status",
    label: "Status",
    width: 112,
    minimumViewport: 440,
    sortable: true,
    sortValue: (row) => row.status,
    render: (row) => (
      <span className={styles.status} data-status={row.status}>
        {row.status}
      </span>
    ),
  },
  {
    id: "down",
    label: "Down",
    width: 100,
    minimumViewport: 620,
    align: "right",
    sortable: true,
    sortValue: (row) => row.downloadRate,
    render: (row) => formatRate(row.downloadRate),
  },
  {
    id: "up",
    label: "Up",
    width: 92,
    minimumViewport: 980,
    align: "right",
    sortable: true,
    sortValue: (row) => row.uploadRate,
    render: (row) => formatRate(row.uploadRate),
  },
  {
    id: "peers",
    label: "Peers",
    width: 72,
    minimumViewport: 860,
    align: "right",
    sortable: true,
    sortValue: (row) => row.peersConnected,
    render: (row) =>
      row.peersKnown === null
        ? row.peersConnected.toLocaleString()
        : `${row.peersConnected}/${row.peersKnown}`,
  },
  {
    id: "eta",
    label: "ETA",
    width: 84,
    minimumViewport: 700,
    align: "right",
    sortable: true,
    sortValue: (row) => row.etaSeconds,
    render: (row) => formatEta(row.etaSeconds),
  },
];

export function TorrentTable() {
  const order = useInspectionStore((state) => state.torrentOrder);
  const torrents = useInspectionStore((state) => state.torrents);
  const category = useInspectionStore(
    (state) => state.presentation.workbenchCategory,
  );
  const batchSelectedIds = useInspectionStore(
    (state) => state.presentation.batchSelectedTorrentIds,
  );
  const activeTorrentId = useInspectionStore(
    (state) => state.presentation.activeTorrentId,
  );
  const batchSelectionMode = useInspectionStore(
    (state) => state.presentation.torrentBatchSelectionMode,
  );
  const openTorrentDetail = useInspectionStore(
    (state) => state.openTorrentDetail,
  );
  const clearActiveTorrent = useInspectionStore(
    (state) => state.clearActiveTorrent,
  );
  const enterTorrentBatchSelection = useInspectionStore(
    (state) => state.enterTorrentBatchSelection,
  );
  const exitTorrentBatchSelection = useInspectionStore(
    (state) => state.exitTorrentBatchSelection,
  );
  const toggleTorrentBatchSelection = useInspectionStore(
    (state) => state.toggleTorrentBatchSelection,
  );
  const replaceTorrentBatchSelection = useInspectionStore(
    (state) => state.replaceTorrentBatchSelection,
  );
  const demo = useInspectionStore((state) => state.demo);
  const materialization = useInspectionStore((state) => state.viewStatus.library);
  const interfaceSize = useInspectionStore(
    (state) => state.presentation.interfaceSize,
  );
  const rows = useMemo(
    () =>
      order
        .map((id) => torrents[id])
        .filter((row): row is TorrentRow => row !== undefined)
        .filter((row) => torrentMatchesCategory(row, category)),
    [category, order, torrents],
  );
  const batchSelectedIdSet = useMemo(
    () => new Set(batchSelectedIds),
    [batchSelectedIds],
  );

  return (
    <VirtualTable
      tableId="torrents"
      label="Torrent library"
      rows={rows}
      getRowId={(row) => row.id}
      columns={COLUMNS}
      interfaceSize={interfaceSize}
      activeRowId={activeTorrentId}
      batchSelection={{
        active: batchSelectionMode,
        batchSelectedIds: batchSelectedIdSet,
        getRowLabel: (row) => row.name,
        onEnterBatch: (row) => enterTorrentBatchSelection(row?.id),
        onExitBatch: exitTorrentBatchSelection,
        onToggleBatch: (row) => toggleTorrentBatchSelection(row.id),
        onReplaceBatch: (rangeRows) =>
          replaceTorrentBatchSelection(rangeRows.map((row) => row.id)),
        onSetAllBatch: (visibleRows, selected) => {
          const visibleIds = new Set(visibleRows.map((row) => row.id));
          replaceTorrentBatchSelection(
            selected
              ? [...batchSelectedIds, ...visibleIds]
              : batchSelectedIds.filter((id) => !visibleIds.has(id)),
          );
        },
      }}
      onActivate={(row) => openTorrentDetail(row.id)}
      onClearActive={clearActiveTorrent}
      emptyMessage={
        materialization.status !== "ready"
          ? materializationMessage(materialization)
          : category === "all" && demo === null
            ? "No torrents are present in the live engine."
            : category === "all"
          ? "No torrents yet. Add a generated demo transfer or choose another scenario."
          : `No torrents in ${category}.`
      }
      initialSort={{ columnId: "name", direction: "asc" }}
    />
  );
}

function materializationMessage(materialization: ViewMaterialization): string {
  switch (materialization.status) {
    case "not_requested":
      return "Torrent library is not requested in this layout.";
    case "loading":
      return "Loading torrent library…";
    case "unavailable":
    case "unsupported":
    case "stale":
      return materialization.reason;
    case "ready":
      return "No torrents are present.";
  }
}
