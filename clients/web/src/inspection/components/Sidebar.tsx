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
  { id: "all", label: "All content", symbol: "▦" },
  { id: "recent", label: "Recently added", symbol: "◷" },
  { id: "available", label: "Available offline", symbol: "✓" },
  { id: "downloading", label: "Downloading", symbol: "↓" },
];

const TORRENT_CATEGORIES: readonly {
  readonly id: TorrentCategory;
  readonly label: string;
  readonly symbol: string;
}[] = [
  { id: "all", label: "All torrents", symbol: "≡" },
  { id: "active", label: "Active", symbol: "↯" },
  { id: "downloading", label: "Downloading", symbol: "↓" },
  { id: "completed", label: "Completed", symbol: "✓" },
  { id: "paused", label: "Paused", symbol: "Ⅱ" },
  { id: "errors", label: "Needs attention", symbol: "!" },
  { id: "archived", label: "Archived", symbol: "□" },
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
      <nav className={styles.sidebar} aria-label="Library filters">
        <p className={styles.heading}>Library</p>
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
        <p className={styles.note}>
          Media details and playback are not connected yet.
        </p>
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
          ? "Transfer filters"
          : "Workbench torrent filters"
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
      return "Library";
    case "transfers":
      return "Transfers";
    case "workbench":
      return "Torrents";
  }
}
