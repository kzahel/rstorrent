import { message as localizedMessage } from "../../localization/runtime";
import { useMemo } from "react";

import { useInspectionStore } from "../context";
import type {
  ApplicationDestination,
  LibraryCategory,
  TorrentCategory,
  TorrentRow,
} from "../model";
import {
  torrentMatchesCategory,
  torrentMatchesLibraryCategory,
} from "../state";
import styles from "./Sidebar.module.css";

const LIBRARY_CATEGORIES: readonly {
  readonly id: LibraryCategory;
  readonly label: string;
  readonly symbol: string;
}[] = [
  { id: "all", label: localizedMessage("inspection.components.sidebar.all.content"), symbol: "▦" },
  { id: "recent", label: localizedMessage("inspection.components.sidebar.recently.added"), symbol: "◷" },
  { id: "available", label: localizedMessage("inspection.components.sidebar.available.offline"), symbol: "✓" },
  { id: "downloading", label: localizedMessage("inspection.components.sidebar.downloading"), symbol: "↓" },
  { id: "archived", label: localizedMessage("inspection.components.sidebar.archived"), symbol: "▣" },
];

const TORRENT_CATEGORIES: readonly {
  readonly id: TorrentCategory;
  readonly label: string;
  readonly symbol: string;
}[] = [
  { id: "all", label: localizedMessage("inspection.components.sidebar.all.torrents"), symbol: "≡" },
  { id: "active", label: localizedMessage("inspection.components.sidebar.active"), symbol: "↯" },
  { id: "downloading", label: localizedMessage("inspection.components.sidebar.downloading"), symbol: "↓" },
  { id: "completed", label: localizedMessage("inspection.components.sidebar.completed"), symbol: "✓" },
  { id: "paused", label: localizedMessage("inspection.components.sidebar.paused"), symbol: "Ⅱ" },
  { id: "errors", label: localizedMessage("inspection.components.sidebar.needs.attention"), symbol: "!" },
  { id: "archived", label: localizedMessage("inspection.components.sidebar.archived"), symbol: "□" },
];

export function Sidebar() {
  const order = useInspectionStore((state) => state.torrentOrder);
  const torrents = useInspectionStore((state) => state.torrents);
  const destination = useInspectionStore(
    (state) => state.presentation.destination,
  );
  const libraryCategory = useInspectionStore(
    (state) => state.presentation.libraryCategory,
  );
  const transfersCategory = useInspectionStore(
    (state) => state.presentation.transfersCategory,
  );
  const workbenchCategory = useInspectionStore(
    (state) => state.presentation.workbenchCategory,
  );
  const selectLibraryCategory = useInspectionStore(
    (state) => state.selectLibraryCategory,
  );
  const selectTorrentCategory = useInspectionStore(
    (state) => state.selectTorrentCategory,
  );
  const rows = useMemo(
    () =>
      order
        .map((id) => torrents[id])
        .filter((row): row is TorrentRow => row !== undefined),
    [order, torrents],
  );
  const newestAddedAtMs = useMemo(
    () =>
      rows.reduce<number | null>(
        (latest, row) =>
          row.addedAtMs !== null && (latest === null || row.addedAtMs > latest)
            ? row.addedAtMs
            : latest,
        null,
      ),
    [rows],
  );

  if (destination === "library") {
    return (
      <nav className={styles.sidebar} aria-label={localizedMessage("inspection.components.sidebar.library.filters")}>
        <p className={styles.heading}>{localizedMessage("inspection.components.sidebar.library")}</p>
        <ul>
          {LIBRARY_CATEGORIES.map((category) => (
            <CategoryButton
              key={category.id}
              active={libraryCategory === category.id}
              count={
                rows.filter((row) =>
                  torrentMatchesLibraryCategory(
                    row,
                    category.id,
                    newestAddedAtMs,
                  ),
                ).length
              }
              label={category.label}
              symbol={category.symbol}
              onSelect={() => selectLibraryCategory(category.id)}
            />
          ))}
        </ul>
        <p className={styles.note}>{localizedMessage("inspection.components.sidebar.playback.and.library.wide.media.grouping.are")}</p>
      </nav>
    );
  }

  const activeCategory =
    destination === "transfers" ? transfersCategory : workbenchCategory;
  return (
    <nav
      className={styles.sidebar}
      aria-label={
        destination === "transfers"
          ? localizedMessage("inspection.components.sidebar.transfer.filters")
          : localizedMessage("inspection.components.sidebar.workbench.torrent.filters")
      }
    >
      <p className={styles.heading}>
        {destinationLabel(destination)}
      </p>
      <ul>
        {TORRENT_CATEGORIES.map((category) => (
          <CategoryButton
            key={category.id}
            active={activeCategory === category.id}
            count={
              rows.filter((row) => torrentMatchesCategory(row, category.id))
                .length
            }
            label={category.label}
            symbol={category.symbol}
            onSelect={() => selectTorrentCategory(category.id)}
          />
        ))}
      </ul>
    </nav>
  );
}

function CategoryButton({
  active,
  count,
  label,
  symbol,
  onSelect,
}: {
  readonly active: boolean;
  readonly count: number;
  readonly label: string;
  readonly symbol: string;
  readonly onSelect: () => void;
}) {
  return (
    <li>
      <button
        type="button"
        aria-current={active ? "page" : undefined}
        onClick={onSelect}
      >
        <span className={styles.symbol} aria-hidden="true">
          {symbol}
        </span>
        <span>{label}</span>
        <span className={styles.count}>{count.toLocaleString()}</span>
      </button>
    </li>
  );
}

function destinationLabel(destination: ApplicationDestination): string {
  switch (destination) {
    case "library":
      return localizedMessage("inspection.components.sidebar.library");
    case "transfers":
      return localizedMessage("inspection.components.sidebar.transfers");
    case "workbench":
      return localizedMessage("inspection.components.sidebar.torrents");
  }
}
