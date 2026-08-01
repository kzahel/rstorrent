import { useMemo } from "react";

import { useInspectionStore } from "../context";
import type { LibraryCategory, TorrentRow } from "../model";
import { torrentMatchesCategory } from "../state";
import styles from "./Sidebar.module.css";

const CATEGORIES: readonly {
  readonly id: LibraryCategory;
  readonly label: string;
  readonly symbol: string;
}[] = [
  { id: "all", label: "All torrents", symbol: "≡" },
  { id: "active", label: "Active", symbol: "↯" },
  { id: "downloading", label: "Downloading", symbol: "↓" },
  { id: "completed", label: "Completed", symbol: "✓" },
  { id: "paused", label: "Paused", symbol: "Ⅱ" },
  { id: "errors", label: "Needs attention", symbol: "!" },
  { id: "archived", label: "Archive", symbol: "□" },
];

export function Sidebar() {
  const order = useInspectionStore((state) => state.torrentOrder);
  const torrents = useInspectionStore((state) => state.torrents);
  const activeCategory = useInspectionStore(
    (state) => state.presentation.category,
  );
  const selectCategory = useInspectionStore((state) => state.selectCategory);
  const rows = useMemo(
    () =>
      order
        .map((id) => torrents[id])
        .filter((row): row is TorrentRow => row !== undefined),
    [order, torrents],
  );

  return (
    <nav className={styles.sidebar} aria-label="Torrent library">
      <p className={styles.heading}>Library</p>
      <ul>
        {CATEGORIES.map((category) => {
          const count = rows.filter((row) =>
            torrentMatchesCategory(row, category.id),
          ).length;
          return (
            <li key={category.id}>
              <button
                type="button"
                aria-current={activeCategory === category.id ? "page" : undefined}
                onClick={() => selectCategory(category.id)}
              >
                <span className={styles.symbol} aria-hidden="true">
                  {category.symbol}
                </span>
                <span>{category.label}</span>
                <span className={styles.count}>{count.toLocaleString()}</span>
              </button>
            </li>
          );
        })}
      </ul>
      <div className={styles.footer}>
        <span className={styles.demoMark} aria-hidden="true">D</span>
        <span>
          <strong>Demo workspace</strong>
          <small>No network traffic</small>
        </span>
      </div>
    </nav>
  );
}
