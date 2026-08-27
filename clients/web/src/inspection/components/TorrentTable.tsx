import { useMemo } from "react";

import { useInspectionStore } from "../context";
import type { DataUnits } from "../appearance";
import {
  etaAccessibleLabel,
  etaSortValue,
  formatBytes,
  formatEta,
  formatRate,
} from "../format";
import type { TorrentRow, ViewMaterialization } from "../model";
import { torrentMatchesCategory } from "../state";
import { TorrentStatus } from "./TorrentStatus";
import { TorrentContextMenu } from "./TorrentContextMenu";
import {
  TorrentProgress,
  torrentProgressSortValue,
} from "./TorrentProgress";
import { VirtualTable, type VirtualColumn } from "./VirtualTable";
import styles from "./TorrentTable.module.css";

const columns = (
  dataUnits: DataUnits,
): readonly VirtualColumn<TorrentRow>[] => [
  {
    id: "name",
    label: "Name",
    width: 330,
    sortable: true,
    sortValue: (row) => row.name,
    render: (row) => (
      <span className={styles.name} title={row.name}>
        <span
          className={styles.stateDot}
          data-status={row.status}
          aria-hidden="true"
        />
        <span>{row.name}</span>
      </span>
    ),
  },
  {
    id: "size",
    label: "Size",
    width: 92,
    align: "right",
    sortable: true,
    sortValue: (row) => row.sizeBytes,
    render: (row) => formatBytes(row.sizeBytes, dataUnits),
  },
  {
    id: "progress",
    label: "Done",
    width: 190,
    align: "right",
    sortable: true,
    sortValue: torrentProgressSortValue,
    render: (row) => <TorrentProgress row={row} />,
  },
  {
    id: "status",
    label: "Status",
    width: 112,
    sortable: true,
    sortValue: (row) => row.status,
    render: (row) => <TorrentStatus row={row} />,
  },
  {
    id: "down",
    label: "Down",
    width: 100,
    align: "right",
    sortable: true,
    sortValue: (row) => row.downloadRate,
    render: (row) => formatRate(row.downloadRate, dataUnits),
  },
  {
    id: "up",
    label: "Up",
    width: 92,
    align: "right",
    sortable: true,
    sortValue: (row) => row.uploadRate,
    render: (row) => formatRate(row.uploadRate, dataUnits),
  },
  {
    id: "peers",
    label: "Peers",
    width: 72,
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
];

export function TorrentTable() {
  const order = useInspectionStore((state) => state.torrentOrder);
  const torrents = useInspectionStore((state) => state.torrents);
  const category = useInspectionStore(
    (state) => state.presentation.workbenchCategory,
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
  const openTorrentDetail = useInspectionStore(
    (state) => state.openTorrentDetail,
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
      tableId="torrents"
      label="Torrent library"
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
            tableId="torrents"
            targetIds={targetIds}
          />
        ),
      }}
      onActivate={(row) => openTorrentDetail(row.id)}
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
