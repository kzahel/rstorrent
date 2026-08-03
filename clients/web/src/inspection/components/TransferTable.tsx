import { useMemo } from "react";

import { useInspectionStore } from "../context";
import {
  formatBytes,
  formatProgress,
  formatRate,
} from "../format";
import type { TorrentRow, ViewMaterialization } from "../model";
import { torrentMatchesCategory } from "../state";
import {
  VirtualTable,
  type VirtualColumn,
} from "./VirtualTable";
import tableStyles from "./TorrentTable.module.css";

const COLUMNS: readonly VirtualColumn<TorrentRow>[] = [
  {
    id: "name",
    label: "Name",
    width: 430,
    minimumWidth: 210,
    sortable: true,
    sortValue: (row) => row.name,
    render: (row) => (
      <span className={tableStyles.name} title={row.name}>
        <span
          className={tableStyles.stateDot}
          data-status={row.status}
          aria-hidden="true"
        />
        <span>{row.name}</span>
      </span>
    ),
  },
  {
    id: "status",
    label: "Status",
    width: 132,
    sortable: true,
    sortValue: (row) => row.status,
    render: (row) => (
      <span className={tableStyles.status} data-status={row.status}>
        {statusLabel(row)}
      </span>
    ),
  },
  {
    id: "progress",
    label: "Progress",
    width: 150,
    align: "right",
    sortable: true,
    sortValue: (row) => row.progress,
    render: (row) => (
      <span className={tableStyles.progressCell}>
        <span className={tableStyles.progressLabel}>
          {formatProgress(row.progress)}
        </span>
        <span className={tableStyles.progressTrack} aria-hidden="true">
          <span
            style={{ width: `${Math.round((row.progress ?? 0) * 100)}%` }}
          />
        </span>
      </span>
    ),
  },
  {
    id: "rate",
    label: "Rate",
    width: 120,
    minimumViewport: 560,
    align: "right",
    sortable: true,
    sortValue: (row) => Math.max(row.downloadRate, row.uploadRate ?? 0),
    render: (row) => transferRate(row),
  },
  {
    id: "size",
    label: "Size",
    width: 104,
    minimumViewport: 720,
    align: "right",
    sortable: true,
    sortValue: (row) => row.sizeBytes,
    render: (row) => formatBytes(row.sizeBytes),
  },
];

export function TransferTable() {
  const order = useInspectionStore((state) => state.torrentOrder);
  const torrents = useInspectionStore((state) => state.torrents);
  const category = useInspectionStore(
    (state) => state.presentation.transfersCategory,
  );
  const selectedIds = useInspectionStore(
    (state) => state.presentation.selectedTorrentIds,
  );
  const selectedId = useInspectionStore(
    (state) => state.presentation.selectedTorrentId,
  );
  const selectionMode = useInspectionStore(
    (state) => state.presentation.torrentSelectionMode,
  );
  const focusTorrent = useInspectionStore((state) => state.focusTorrent);
  const clearTorrentFocus = useInspectionStore(
    (state) => state.clearTorrentFocus,
  );
  const enterTorrentSelection = useInspectionStore(
    (state) => state.enterTorrentSelection,
  );
  const exitTorrentSelection = useInspectionStore(
    (state) => state.exitTorrentSelection,
  );
  const toggleTorrentSelection = useInspectionStore(
    (state) => state.toggleTorrentSelection,
  );
  const replaceTorrentSelection = useInspectionStore(
    (state) => state.replaceTorrentSelection,
  );
  const demo = useInspectionStore((state) => state.demo);
  const materialization = useInspectionStore(
    (state) => state.viewStatus.library,
  );
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
  const selectedIdSet = useMemo(() => new Set(selectedIds), [selectedIds]);

  return (
    <VirtualTable
      tableId="transfers"
      label="Transfer queue"
      rows={rows}
      getRowId={(row) => row.id}
      columns={COLUMNS}
      interfaceSize={interfaceSize}
      selectedId={selectedId}
      selection={{
        active: selectionMode,
        selectedIds: selectedIdSet,
        getRowLabel: (row) => row.name,
        onEnter: (row) => enterTorrentSelection(row?.id),
        onExit: exitTorrentSelection,
        onToggle: (row) => toggleTorrentSelection(row.id),
        onSetAll: (visibleRows, selected) => {
          const visibleIds = new Set(visibleRows.map((row) => row.id));
          replaceTorrentSelection(
            selected
              ? [...selectedIds, ...visibleIds]
              : selectedIds.filter((id) => !visibleIds.has(id)),
          );
        },
      }}
      onSelect={(row) => focusTorrent(row.id)}
      onClear={clearTorrentFocus}
      emptyMessage={
        materialization.status !== "ready"
          ? materializationMessage(materialization)
          : category === "all" && demo === null
            ? "No transfers are present in the live engine."
            : category === "all"
              ? "No transfers yet. Add a generated demo transfer or choose another scenario."
              : `No transfers in ${category}.`
      }
      initialSort={{ columnId: "name", direction: "asc" }}
    />
  );
}

function transferRate(row: TorrentRow): string {
  if (row.downloadRate > 0) return formatRate(row.downloadRate);
  if ((row.uploadRate ?? 0) > 0) return `${formatRate(row.uploadRate)} up`;
  return "—";
}

function statusLabel(row: TorrentRow): string {
  if (row.status === "complete" && (row.uploadRate ?? 0) > 0) return "Complete";
  return row.status.slice(0, 1).toUpperCase() + row.status.slice(1);
}

function materializationMessage(materialization: ViewMaterialization): string {
  switch (materialization.status) {
    case "not_requested":
      return "Transfer collection is not requested in this layout.";
    case "loading":
      return "Loading transfers…";
    case "unavailable":
    case "unsupported":
    case "stale":
      return materialization.reason;
    case "ready":
      return "No transfers are present.";
  }
}
