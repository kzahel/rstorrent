import { useMemo } from "react";

import { useInspectionStore } from "../context";
import type { DataUnits } from "../appearance";
import {
  etaAccessibleLabel,
  etaSortValue,
  formatBytes,
  formatEta,
  formatProgress,
  formatRate,
} from "../format";
import type { TorrentRow, ViewMaterialization } from "../model";
import { torrentMatchesCategory } from "../state";
import { VirtualTable, type VirtualColumn } from "./VirtualTable";
import { TorrentStatus } from "./TorrentStatus";
import { TorrentContextMenu } from "./TorrentContextMenu";
import tableStyles from "./TorrentTable.module.css";

const columns = (
  dataUnits: DataUnits,
): readonly VirtualColumn<TorrentRow>[] => [
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
    render: (row) => <TorrentStatus row={row} label={statusLabel(row)} />,
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
    render: (row) => transferRate(row, dataUnits),
  },
  {
    id: "eta",
    label: "ETA",
    width: 84,
    minimumViewport: 640,
    align: "right",
    sortable: true,
    sortKind: "decimal",
    sortValue: (row) => etaSortValue(row.eta),
    render: (row) => {
      const label = etaAccessibleLabel(row.eta);
      return (
        <span aria-label={label} title={label}>
          {formatEta(row.eta)}
        </span>
      );
    },
  },
  {
    id: "size",
    label: "Size",
    width: 104,
    minimumViewport: 720,
    align: "right",
    sortable: true,
    sortValue: (row) => row.sizeBytes,
    render: (row) => formatBytes(row.sizeBytes, dataUnits),
  },
];

export function TransferTable() {
  const order = useInspectionStore((state) => state.torrentOrder);
  const torrents = useInspectionStore((state) => state.torrents);
  const category = useInspectionStore(
    (state) => state.presentation.transfersCategory,
  );
  const selectedTorrentIds = useInspectionStore(
    (state) => state.presentation.selectedTorrentIds,
  );
  const currentTorrentId = useInspectionStore(
    (state) => state.presentation.currentTorrentId,
  );
  const setTorrentSelection = useInspectionStore(
    (state) => state.setTorrentSelection,
  );
  const demo = useInspectionStore((state) => state.demo);
  const materialization = useInspectionStore(
    (state) => state.viewStatus.library,
  );
  const interfaceSize = useInspectionStore(
    (state) => state.presentation.interfaceSize,
  );
  const dataUnits = useInspectionStore((state) => state.presentation.dataUnits);
  const displayColumns = useMemo(() => columns(dataUnits), [dataUnits]);
  const rows = useMemo(
    () =>
      order
        .map((id) => torrents[id])
        .filter((row): row is TorrentRow => row !== undefined)
        .filter((row) => torrentMatchesCategory(row, category)),
    [category, order, torrents],
  );
  const selectedIdSet = useMemo(
    () => new Set(selectedTorrentIds),
    [selectedTorrentIds],
  );

  return (
    <VirtualTable
      tableId="transfers"
      label="Transfer queue"
      rows={rows}
      getRowId={(row) => row.id}
      columns={displayColumns}
      interfaceSize={interfaceSize}
      currentRowId={currentTorrentId}
      selection={{
        selectedIds: selectedIdSet,
        getRowLabel: (row) => row.name,
        onChange: setTorrentSelection,
      }}
      contextMenu={{
        label: "Torrent actions",
        render: (row, targetIds) => (
          <TorrentContextMenu
            row={row}
            tableId="transfers"
            targetIds={targetIds}
          />
        ),
      }}
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

function transferRate(row: TorrentRow, dataUnits: DataUnits): string {
  if (row.downloadRate > 0) return formatRate(row.downloadRate, dataUnits);
  if ((row.uploadRate ?? 0) > 0)
    return `${formatRate(row.uploadRate, dataUnits)} up`;
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
