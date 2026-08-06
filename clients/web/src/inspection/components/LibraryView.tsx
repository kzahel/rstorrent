import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type UIEvent,
} from "react";

import type { DataUnits, InterfaceSize } from "../appearance";
import { useInspectionStore } from "../context";
import { formatBytes, formatProgress } from "../format";
import type {
  LibraryCategory,
  TorrentRow,
  ViewMaterialization,
} from "../model";
import { torrentMatchesLibraryCategory } from "../state";
import { Icon } from "./Icon";
import styles from "./LibraryView.module.css";

const GRID_METRICS: Readonly<
  Record<
    InterfaceSize,
    {
      readonly minimumCardWidth: number;
      readonly rowHeight: number;
      readonly gap: number;
    }
  >
> = {
  compact: { minimumCardWidth: 176, rowHeight: 224, gap: 10 },
  standard: { minimumCardWidth: 206, rowHeight: 258, gap: 14 },
  spacious: { minimumCardWidth: 232, rowHeight: 294, gap: 16 },
};

const OVERSCAN_ROWS = 2;

export function LibraryView() {
  const dataUnits = useInspectionStore((state) => state.presentation.dataUnits);
  const order = useInspectionStore((state) => state.torrentOrder);
  const torrents = useInspectionStore((state) => state.torrents);
  const category = useInspectionStore(
    (state) => state.presentation.libraryCategory,
  );
  const currentTorrentId = useInspectionStore(
    (state) => state.presentation.currentTorrentId,
  );
  const interfaceSize = useInspectionStore(
    (state) => state.presentation.interfaceSize,
  );
  const selectOnlyTorrent = useInspectionStore(
    (state) => state.selectOnlyTorrent,
  );
  const openTorrentInWorkbench = useInspectionStore(
    (state) => state.openTorrentInWorkbench,
  );
  const materialization = useInspectionStore(
    (state) => state.viewStatus.library,
  );
  const allRows = useMemo(
    () =>
      order
        .map((id) => torrents[id])
        .filter((row): row is TorrentRow => row !== undefined),
    [order, torrents],
  );
  const newestAddedAtMs = useMemo(
    () =>
      allRows.reduce<number | null>(
        (latest, row) =>
          row.addedAtMs !== null && (latest === null || row.addedAtMs > latest)
            ? row.addedAtMs
            : latest,
        null,
      ),
    [allRows],
  );
  const rows = useMemo(
    () =>
      allRows.filter((row) =>
        torrentMatchesLibraryCategory(row, category, newestAddedAtMs),
      ),
    [allRows, category, newestAddedAtMs],
  );
  const current = rows.find((row) => row.id === currentTorrentId);

  return (
    <section className={styles.library} aria-labelledby="library-heading">
      <div className={styles.heading}>
        <div>
          <p className={styles.eyebrow}>Library</p>
          <h1 id="library-heading">{categoryLabel(category)}</h1>
          <p>
            {rows.length.toLocaleString()} torrent-backed content{" "}
            {rows.length === 1 ? "source" : "sources"}
            <span aria-hidden="true"> · </span>
            <span>media details are not connected yet</span>
          </p>
        </div>
        <button
          type="button"
          disabled={current === undefined}
          onClick={() =>
            current === undefined
              ? undefined
              : openTorrentInWorkbench(current.id)
          }
        >
          <Icon name="workbench" /> Open in Workbench
        </button>
      </div>
      {materialization.status !== "ready" ? (
        <LibraryEmpty message={materializationMessage(materialization)} />
      ) : rows.length === 0 ? (
        <LibraryEmpty message={emptyMessage(category)} />
      ) : (
        <VirtualLibraryGrid
          rows={rows}
          currentId={currentTorrentId}
          interfaceSize={interfaceSize}
          dataUnits={dataUnits}
          onActivate={selectOnlyTorrent}
        />
      )}
    </section>
  );
}

function VirtualLibraryGrid({
  rows,
  currentId,
  interfaceSize,
  dataUnits,
  onActivate,
}: {
  readonly rows: readonly TorrentRow[];
  readonly currentId: string | null;
  readonly interfaceSize: InterfaceSize;
  readonly dataUnits: DataUnits;
  readonly onActivate: (torrentId: string) => void;
}) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportSize, setViewportSize] = useState({ width: 960, height: 640 });
  const metrics = GRID_METRICS[interfaceSize];

  useEffect(() => {
    const element = viewportRef.current;
    if (element === null) return;
    const measure = () => {
      setViewportSize({
        width: element.clientWidth || 960,
        height: element.clientHeight || 640,
      });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const columnCount = Math.max(
    1,
    Math.floor(
      (viewportSize.width + metrics.gap) /
        (metrics.minimumCardWidth + metrics.gap),
    ),
  );
  const logicalRowCount = Math.ceil(rows.length / columnCount);
  const firstRow = Math.max(
    0,
    Math.floor(scrollTop / metrics.rowHeight) - OVERSCAN_ROWS,
  );
  const visibleRowCount =
    Math.ceil(viewportSize.height / metrics.rowHeight) + OVERSCAN_ROWS * 2;
  const lastRow = Math.min(logicalRowCount, firstRow + visibleRowCount);
  const rowStyle = {
    "--library-columns": columnCount,
    "--library-grid-gap": `${metrics.gap}px`,
    "--library-row-height": `${metrics.rowHeight}px`,
  } as CSSProperties;

  const onScroll = (event: UIEvent<HTMLDivElement>) => {
    setScrollTop(event.currentTarget.scrollTop);
  };

  return (
    <div
      ref={viewportRef}
      className={styles.viewport}
      role="list"
      aria-label="Torrent-backed content"
      onScroll={onScroll}
    >
      <div
        className={styles.canvas}
        style={{ height: `${logicalRowCount * metrics.rowHeight}px` }}
      >
        {Array.from({ length: lastRow - firstRow }, (_, offset) => {
          const logicalRow = firstRow + offset;
          const firstItem = logicalRow * columnCount;
          return (
            <div
              key={logicalRow}
              className={styles.gridRow}
              style={{
                ...rowStyle,
                transform: `translateY(${logicalRow * metrics.rowHeight}px)`,
              }}
            >
              {rows
                .slice(firstItem, firstItem + columnCount)
                .map((row, column) => {
                  const position = firstItem + column;
                  return (
                    <article
                      key={row.id}
                      className={styles.card}
                      role="listitem"
                      aria-posinset={position + 1}
                      aria-setsize={rows.length}
                      data-current={row.id === currentId}
                    >
                      <button
                        type="button"
                        aria-label={`Activate ${row.name} in Library`}
                        aria-pressed={row.id === currentId}
                        onClick={() => onActivate(row.id)}
                      >
                        <span
                          className={styles.art}
                          data-tone={toneFor(row.infoHash)}
                          aria-hidden="true"
                        >
                          <span>{initials(row.name)}</span>
                        </span>
                        <span className={styles.cardBody}>
                          <strong title={row.name}>{row.name}</strong>
                          <span>{availabilityLabel(row)}</span>
                          <span>{formatBytes(row.sizeBytes, dataUnits)}</span>
                        </span>
                        {row.progress === null ? null : (
                          <span
                            className={styles.progress}
                            role="progressbar"
                            aria-label={`${row.name} download progress`}
                            aria-valuemin={0}
                            aria-valuemax={100}
                            aria-valuenow={Math.round(row.progress * 100)}
                          >
                            <span
                              style={{
                                width: `${Math.round(row.progress * 100)}%`,
                              }}
                            />
                          </span>
                        )}
                      </button>
                    </article>
                  );
                })}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function LibraryEmpty({ message }: { readonly message: string }) {
  return (
    <div className={styles.empty}>
      <span aria-hidden="true">◇</span>
      <strong>{message}</strong>
      <p>
        Library will gain media-aware files, metadata, artwork, and playback
        only when those application owners are connected.
      </p>
    </div>
  );
}

function categoryLabel(category: LibraryCategory): string {
  switch (category) {
    case "all":
      return "All content";
    case "recent":
      return "Recently added";
    case "available":
      return "Available offline";
    case "downloading":
      return "Downloading";
  }
}

function availabilityLabel(row: TorrentRow): string {
  switch (row.status) {
    case "metadata":
      return "Finding content details";
    case "downloading":
      return `${formatProgress(row.progress)} downloaded`;
    case "complete":
      return "Available offline";
    case "paused":
      return `${formatProgress(row.progress)} downloaded · Paused`;
    case "checking":
      return "Checking downloaded content";
    case "error":
      return "Content needs attention";
  }
}

function emptyMessage(category: LibraryCategory): string {
  switch (category) {
    case "all":
      return "No content sources yet";
    case "recent":
      return "No recently added content";
    case "available":
      return "No content is available offline";
    case "downloading":
      return "No content is downloading";
  }
}

function materializationMessage(materialization: ViewMaterialization): string {
  switch (materialization.status) {
    case "not_requested":
      return "Library collection is not requested in this layout";
    case "loading":
      return "Loading Library content…";
    case "unavailable":
    case "unsupported":
    case "stale":
      return materialization.reason;
    case "ready":
      return "No content sources yet";
  }
}

function initials(name: string): string {
  const value = name
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part.slice(0, 1).toUpperCase())
    .join("");
  return value === "" ? "RS" : value;
}

function toneFor(infoHash: string): string {
  let sum = 0;
  for (const character of infoHash.slice(0, 12)) sum += character.charCodeAt(0);
  return String(sum % 6);
}
