import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type UIEvent,
} from "react";

import { useInspectionStore } from "../context";
import { formatDecimalProgress, formatExactBytes } from "../format";
import { episodeLabel, sortFileRows, sortMediaRows } from "../library-media";
import type { FileRow, MediaRow, TorrentRow, ViewMaterialization } from "../model";
import { Icon } from "./Icon";
import styles from "./LibraryDetailView.module.css";

const OVERSCAN = 5;

export function LibraryDetailView({
  torrent,
  onBack,
}: {
  readonly torrent: TorrentRow;
  readonly onBack: () => void;
}) {
  const dataUnits = useInspectionStore((state) => state.presentation.dataUnits);
  const layout = useInspectionStore((state) => state.presentation.layout);
  const mode = useInspectionStore(
    (state) => state.presentation.libraryDetailMode,
  );
  const selectMode = useInspectionStore(
    (state) => state.selectLibraryDetailMode,
  );
  const openInWorkbench = useInspectionStore(
    (state) => state.openTorrentInWorkbench,
  );
  const media = useInspectionStore(
    (state) => state.mediaByTorrent[torrent.id],
  );
  const files = useInspectionStore(
    (state) => state.filesByTorrent[torrent.id],
  );
  const mediaStatus = useInspectionStore((state) => state.viewStatus.media);
  const filesStatus = useInspectionStore((state) => state.viewStatus.files);
  const autoFallback = useRef(torrent.id);

  useEffect(() => {
    autoFallback.current = torrent.id;
  }, [torrent.id]);

  useEffect(() => {
    if (
      mode === "media" &&
      autoFallback.current === torrent.id &&
      media?.state === "available" &&
      media.order.length === 0
    ) {
      autoFallback.current = "";
      selectMode("files");
    }
  }, [media, mode, selectMode, torrent.id]);

  const chooseMode = (next: "media" | "files") => {
    autoFallback.current = "";
    selectMode(next);
  };
  const mediaCount = media?.order.length;

  return (
    <section className={styles.detail} aria-labelledby="library-detail-heading">
      <header className={styles.header}>
        <button type="button" className={styles.back} onClick={onBack}>
          <span aria-hidden="true">←</span>
          <span>Library</span>
        </button>
        <div>
          <p>Content details</p>
          <h1 id="library-detail-heading">{torrent.name}</h1>
        </div>
        <button
          type="button"
          className={styles.workbench}
          onClick={() => openInWorkbench(torrent.id)}
        >
          <Icon name="workbench" />
          <span>Open in Workbench</span>
        </button>
      </header>
      <div className={styles.body}>
        <aside className={styles.summary} aria-label="Torrent content summary">
          <div className={styles.placeholder} aria-hidden="true">
            <span>{initials(torrent.name)}</span>
          </div>
          <div className={styles.summaryText}>
            <strong>{torrent.name}</strong>
            <span>{torrentSummary(torrent)}</span>
            <span>
              {torrent.sizeBytes === null
                ? "Size pending"
                : formatExactBytes(String(torrent.sizeBytes), dataUnits)}
            </span>
            {mediaCount === undefined ? null : (
              <span>
                {mediaCount.toLocaleString()} recognized video
                {mediaCount === 1 ? "" : "s"}
              </span>
            )}
          </div>
        </aside>
        <div className={styles.catalog}>
          <div className={styles.tabs} role="tablist" aria-label="Content view">
            <button
              type="button"
              role="tab"
              aria-selected={mode === "media"}
              onClick={() => chooseMode("media")}
            >
              Media
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={mode === "files"}
              onClick={() => chooseMode("files")}
            >
              All files
            </button>
          </div>
          {mode === "media" ? (
            <MediaCatalog
              rows={media === undefined ? [] : media.order
                .map((id) => media.rows[id])
                .filter((row): row is MediaRow => row !== undefined)}
              state={media?.state}
              materialization={mediaStatus}
              layout={layout}
              dataUnits={dataUnits}
            />
          ) : (
            <FileCatalog
              rows={files === undefined ? [] : files.order
                .map((id) => files.rows[id])
                .filter((row): row is FileRow => row !== undefined && !row.padding)}
              state={files?.state}
              total={files?.page.total ?? 0}
              materialization={filesStatus}
              layout={layout}
              dataUnits={dataUnits}
            />
          )}
        </div>
      </div>
    </section>
  );
}

function MediaCatalog({
  rows,
  state,
  materialization,
  layout,
  dataUnits,
}: {
  readonly rows: readonly MediaRow[];
  readonly state: "metadata_pending" | "available" | "torrent_missing" | undefined;
  readonly materialization: ViewMaterialization;
  readonly layout: "wide" | "compact" | "phone";
  readonly dataUnits: "decimal" | "binary";
}) {
  if (materialization.status !== "ready") {
    return <CatalogMessage message={materializationMessage(materialization)} />;
  }
  if (state === "metadata_pending") {
    return <CatalogMessage message="Waiting for torrent metadata…" />;
  }
  if (state === "torrent_missing") {
    return <CatalogMessage message="This torrent is no longer in the Library." />;
  }
  const sorted = sortMediaRows(rows);
  if (sorted.length === 0) {
    return <CatalogMessage message="No recognized video files" />;
  }
  return (
    <VirtualCatalog
      rows={sorted.map((row) => ({ kind: "media" as const, row }))}
      layout={layout}
      dataUnits={dataUnits}
      label="Recognized video files"
    />
  );
}

function FileCatalog({
  rows,
  state,
  total,
  materialization,
  layout,
  dataUnits,
}: {
  readonly rows: readonly FileRow[];
  readonly state: "metadata_pending" | "available" | "torrent_missing" | undefined;
  readonly total: number;
  readonly materialization: ViewMaterialization;
  readonly layout: "wide" | "compact" | "phone";
  readonly dataUnits: "decimal" | "binary";
}) {
  if (materialization.status !== "ready") {
    return <CatalogMessage message={materializationMessage(materialization)} />;
  }
  if (state === "metadata_pending") {
    return <CatalogMessage message="Waiting for torrent metadata…" />;
  }
  if (state === "torrent_missing") {
    return <CatalogMessage message="This torrent is no longer in the Library." />;
  }
  const sorted = sortFileRows(rows);
  if (sorted.length === 0) return <CatalogMessage message="No files" />;
  return (
    <>
      {total > sorted.length ? (
        <p className={styles.pageNotice}>
          Showing the first {sorted.length.toLocaleString()} of {total.toLocaleString()} files
        </p>
      ) : null}
      <VirtualCatalog
        rows={sorted.map((row) => ({ kind: "file" as const, row }))}
        layout={layout}
        dataUnits={dataUnits}
        label="All torrent files"
      />
    </>
  );
}

type CatalogRow =
  | { readonly kind: "media"; readonly row: MediaRow }
  | { readonly kind: "file"; readonly row: FileRow };

function VirtualCatalog({
  rows,
  layout,
  dataUnits,
  label,
}: {
  readonly rows: readonly CatalogRow[];
  readonly layout: "wide" | "compact" | "phone";
  readonly dataUnits: "decimal" | "binary";
  readonly label: string;
}) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(640);
  const rowHeight = layout === "phone" ? 104 : 82;

  useEffect(() => {
    const element = viewportRef.current;
    if (element === null) return;
    const measure = () => setHeight(element.clientHeight || 640);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN);
  const count = Math.ceil(height / rowHeight) + OVERSCAN * 2;
  const visible = rows.slice(first, Math.min(rows.length, first + count));
  return (
    <div
      ref={viewportRef}
      className={styles.viewport}
      role="list"
      aria-label={label}
      onScroll={(event: UIEvent<HTMLDivElement>) =>
        setScrollTop(event.currentTarget.scrollTop)
      }
    >
      <div className={styles.canvas} style={{ height: rows.length * rowHeight }}>
        {visible.map((entry, offset) => (
          <CatalogRowView
            key={`${entry.kind}:${entry.row.id}`}
            entry={entry}
            dataUnits={dataUnits}
            style={{ transform: `translateY(${(first + offset) * rowHeight}px)` }}
          />
        ))}
      </div>
    </div>
  );
}

function CatalogRowView({
  entry,
  dataUnits,
  style,
}: {
  readonly entry: CatalogRow;
  readonly dataUnits: "decimal" | "binary";
  readonly style: CSSProperties;
}) {
  const row = entry.row;
  const label = entry.kind === "media" ? episodeLabel(entry.row) : null;
  const downloaded = row.verifiedBytes === row.lengthBytes;
  const progressState = downloaded
    ? "Downloaded"
    : row.selection === "skipped"
      ? "Not selected"
      : row.doneBytes === "0"
        ? "Not downloaded"
        : "Partially downloaded";
  return (
    <article className={styles.row} style={style} role="listitem">
      <span className={styles.type} aria-hidden="true">
        {entry.kind === "media" ? "▶" : row.extension.slice(0, 3).toUpperCase() || "FILE"}
      </span>
      <span className={styles.identity}>
        <span className={styles.rowHeading}>
          {label === null ? null : <b>{label}</b>}
          <strong title={row.name}>{row.name}</strong>
        </span>
        <span title={row.path.join("/")}>{row.folder || "Torrent root"}</span>
        <span className={styles.progress} aria-label={`${row.name} download progress`}>
          <span aria-hidden="true">
            <span style={{ width: formatDecimalProgress(row.doneBytes, row.lengthBytes) }} />
          </span>
          {formatDecimalProgress(row.doneBytes, row.lengthBytes)} done · {formatDecimalProgress(row.verifiedBytes, row.lengthBytes)} verified
        </span>
      </span>
      <span className={styles.facts}>
        <strong data-downloaded={downloaded || undefined}>
          {progressState}
        </strong>
        <span>{formatExactBytes(row.lengthBytes, dataUnits)}</span>
        <span>{selectionLabel(row.selection)}</span>
      </span>
    </article>
  );
}

function CatalogMessage({ message }: { readonly message: string }) {
  return (
    <div className={styles.message} role="status">
      <span aria-hidden="true">◇</span>
      <strong>{message}</strong>
    </div>
  );
}

function materializationMessage(materialization: ViewMaterialization): string {
  switch (materialization.status) {
    case "not_requested":
      return "Content details are not requested";
    case "loading":
      return "Loading content details…";
    case "unavailable":
    case "unsupported":
    case "stale":
      return materialization.reason;
    case "ready":
      return "No content";
  }
}

function torrentSummary(torrent: TorrentRow): string {
  switch (torrent.status) {
    case "metadata": return "Finding content details";
    case "downloading": return "Downloading";
    case "complete": return "Available offline";
    case "paused": return "Paused";
    case "checking": return "Checking downloaded data";
    case "error": return "Needs attention";
  }
}

function selectionLabel(selection: "normal" | "high" | "skipped" | null): string {
  if (selection === "high") return "High priority";
  if (selection === "skipped") return "Skipped";
  return "Normal priority";
}

function initials(name: string): string {
  const value = name
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part.slice(0, 1).toUpperCase())
    .join("");
  return value || "RS";
}
