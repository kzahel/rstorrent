import { useMemo } from "react";

import { useInspectionStore } from "../context";
import type { TorrentCategory, TorrentRow } from "../model";
import { torrentMatchesCategory } from "../state";
import { TorrentActions } from "./TorrentActions";
import { TransferTable } from "./TransferTable";
import styles from "./TransfersView.module.css";

interface TransfersViewProps {
  readonly showCrostiniStorageHelp: boolean;
  readonly oneCurrentRoot?: boolean;
}

export function TransfersView({
  showCrostiniStorageHelp,
  oneCurrentRoot = false,
}: TransfersViewProps) {
  const order = useInspectionStore((state) => state.torrentOrder);
  const torrents = useInspectionStore((state) => state.torrents);
  const category = useInspectionStore(
    (state) => state.presentation.transfersCategory,
  );
  const rows = useMemo(
    () =>
      order
        .map((id) => torrents[id])
        .filter((row): row is TorrentRow => row !== undefined),
    [order, torrents],
  );
  const visibleCount = rows.filter((row) =>
    torrentMatchesCategory(row, category),
  ).length;
  const activeCount = rows.filter(
    (row) =>
      row.operationalState === "starting" ||
      row.operationalState === "downloading",
  ).length;

  return (
    <section className={styles.transfers} aria-labelledby="transfers-heading">
      <div className={styles.heading}>
        <div>
          <p>{categoryLabel(category)}</p>
          <h1 id="transfers-heading">Transfers</h1>
        </div>
        <span>
          {visibleCount.toLocaleString()} shown
          <span aria-hidden="true"> · </span>
          {activeCount.toLocaleString()} active
        </span>
      </div>
      <TorrentActions
        showCrostiniStorageHelp={showCrostiniStorageHelp}
        oneCurrentRoot={oneCurrentRoot}
      />
      <div className={styles.table}>
        <TransferTable />
      </div>
    </section>
  );
}

function categoryLabel(category: TorrentCategory): string {
  switch (category) {
    case "all":
      return "All torrents";
    case "active":
      return "Active";
    case "downloading":
      return "Downloading";
    case "completed":
      return "Completed";
    case "paused":
      return "Paused";
    case "errors":
      return "Needs attention";
    case "archived":
      return "Archived";
  }
}
