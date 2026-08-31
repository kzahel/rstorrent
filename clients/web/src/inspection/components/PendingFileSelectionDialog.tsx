import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type UIEvent,
} from "react";

import { formatExactBytes } from "../format";
import type { DataUnits } from "../appearance";
import type { FileRow, FileSet, TorrentRow } from "../model";
import styles from "./AddTorrentDialog.module.css";

const MAX_OVERRIDES = 4_096;

interface DraftOverride {
  readonly selected: boolean;
  readonly initialSelected: boolean;
  readonly lengthBytes: string;
}

export interface PendingFileSelectionDialogProps {
  readonly torrent: TorrentRow;
  readonly files: FileSet | undefined;
  readonly rootLabel: string;
  readonly queuedCount: number;
  readonly dataUnits: DataUnits;
  readonly onPage: (offset: number) => void;
  readonly onConfirm: (
    base: "current" | "all" | "none",
    overrides: readonly {
      readonly start: number;
      readonly endExclusive: number;
      readonly selected: boolean;
    }[],
    disableFuture: boolean,
  ) => Promise<void>;
  readonly onCancel: () => Promise<void>;
}

export function PendingFileSelectionDialog({
  torrent,
  files,
  rootLabel,
  queuedCount,
  dataUnits,
  onPage,
  onConfirm,
  onCancel,
}: PendingFileSelectionDialogProps) {
  const [base, setBase] = useState<"current" | "all" | "none">("current");
  const [overrides, setOverrides] = useState<ReadonlyMap<number, DraftOverride>>(
    new Map(),
  );
  const [loadedRows, setLoadedRows] = useState<ReadonlyMap<number, FileRow>>(
    new Map(),
  );
  const [disableFuture, setDisableFuture] = useState(false);
  const [pending, setPending] = useState<"confirm" | "cancel" | null>(null);
  const [error, setError] = useState("");
  const dialogRef = useRef<HTMLDivElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const lastRequestedOffset = useRef(0);

  useEffect(() => {
    cancelRef.current?.focus();
  }, []);

  useEffect(() => {
    if (files === undefined || files.state !== "available") return;
    setLoadedRows((current) => {
      const next = files.page.offset === 0 ? new Map<number, FileRow>() : new Map(current);
      for (const id of files.order) {
        const row = files.rows[id];
        if (row !== undefined && !row.padding) next.set(row.index, row);
      }
      return next;
    });
    lastRequestedOffset.current = files.page.offset;
  }, [files]);

  const rows = useMemo(
    () => [...loadedRows.values()].sort((left, right) => left.index - right.index),
    [loadedRows],
  );
  const summary = useMemo(
    () => draftSummary(torrent, base, overrides),
    [base, overrides, torrent],
  );
  const busy = pending !== null;

  const selectedFor = (row: FileRow) =>
    overrides.get(row.index)?.selected ??
    (base === "all"
      ? true
      : base === "none"
        ? false
        : row.selection !== "skipped");

  const toggle = (row: FileRow) => {
    const nextSelected = !selectedFor(row);
    const baseSelected =
      base === "all"
        ? true
        : base === "none"
          ? false
          : row.selection !== "skipped";
    setOverrides((current) => {
      const next = new Map(current);
      if (nextSelected === baseSelected) {
        next.delete(row.index);
      } else {
        if (!next.has(row.index) && next.size >= MAX_OVERRIDES) {
          setError(
            `This selection is too fragmented. Use All or None, then change at most ${MAX_OVERRIDES.toLocaleString()} files.`,
          );
          return current;
        }
        next.set(row.index, {
          selected: nextSelected,
          initialSelected: row.selection !== "skipped",
          lengthBytes: row.lengthBytes,
        });
      }
      setError("");
      return next;
    });
  };

  const chooseBase = (next: "all" | "none") => {
    setBase(next);
    setOverrides(new Map());
    setError("");
  };

  const loadNextPage = (event: UIEvent<HTMLDivElement>) => {
    const page = files?.page;
    if (page?.nextOffset === null || page?.nextOffset === undefined) return;
    const target = event.currentTarget;
    if (target.scrollHeight - target.scrollTop - target.clientHeight > 240) return;
    if (lastRequestedOffset.current === page.nextOffset) return;
    lastRequestedOffset.current = page.nextOffset;
    onPage(page.nextOffset);
  };

  const confirm = async () => {
    const catalogId = torrent.fileCatalogId;
    if (catalogId === null || catalogId === undefined) return;
    setPending("confirm");
    setError("");
    try {
      await onConfirm(base, compactOverrides(overrides), disableFuture);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setPending(null);
    }
  };

  const cancel = async () => {
    setPending("cancel");
    setError("");
    try {
      await onCancel();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setPending(null);
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") event.preventDefault();
    if (event.key !== "Tab") return;
    const focusable = dialogRef.current?.querySelectorAll<HTMLElement>(
      'button:not(:disabled), input:not(:disabled), [tabindex]:not([tabindex="-1"])',
    );
    if (focusable === undefined || focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  };

  const metadataReady = torrent.fileCatalogId != null && files?.state === "available";
  return (
    <div className={styles.backdrop}>
      <div
        ref={dialogRef}
        className={`${styles.dialog} ${styles.selectionDialog}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="pending-selection-title"
        aria-describedby="pending-selection-description"
        onKeyDown={handleKeyDown}
      >
        <header>
          <p>Choose files</p>
          <h2 id="pending-selection-title">{torrent.name}</h2>
        </header>
        <p id="pending-selection-description" className={styles.description}>
          Checked files use Normal priority. Unchecked files are skipped. High
          priority remains available later in the Files tab.
        </p>
        <div className={styles.selectionMeta}>
          <span>Download folder: <strong>{rootLabel}</strong></span>
          {queuedCount === 0 ? null : (
            <span>{queuedCount.toLocaleString()} more pending</span>
          )}
        </div>
        {!metadataReady ? (
          <div className={styles.metadataWait} role="status">
            <strong>Fetching file information…</strong>
            <span>No content files will download before you confirm.</span>
          </div>
        ) : (
          <>
            <div className={styles.selectionToolbar}>
              <button type="button" disabled={busy} onClick={() => chooseBase("all")}>All</button>
              <button type="button" disabled={busy} onClick={() => chooseBase("none")}>None</button>
              <span>
                {summary.count.toLocaleString()} of {(torrent.selectableFileCount ?? 0).toLocaleString()} selected
                {" · "}{formatExactBytes(summary.bytes.toString(), dataUnits)}
              </span>
            </div>
            <div className={styles.selectionList} onScroll={loadNextPage}>
              {rows.map((row) => (
                <label key={row.id} className={styles.selectionRow}>
                  <input
                    type="checkbox"
                    checked={selectedFor(row)}
                    disabled={busy}
                    onChange={() => toggle(row)}
                  />
                  <span>
                    <strong title={row.path.join("/")}>{row.path.join("/")}</strong>
                    <small>{formatExactBytes(row.lengthBytes, dataUnits)}</small>
                  </span>
                </label>
              ))}
              {files.page.nextOffset === null ? null : (
                <p className={styles.loadingMore}>Scroll to load more files</p>
              )}
            </div>
          </>
        )}
        <label className={styles.preference}>
          <input
            type="checkbox"
            checked={disableFuture}
            disabled={busy}
            onChange={(event) => setDisableFuture(event.currentTarget.checked)}
          />
          Don’t show file selection again
        </label>
        {error === "" ? null : <p className={styles.error} role="alert">{error}</p>}
        <div className={styles.actions}>
          <button ref={cancelRef} type="button" disabled={busy} onClick={() => void cancel()}>
            {pending === "cancel" ? "Cancelling…" : "Cancel"}
          </button>
          <button
            className={styles.confirm}
            type="button"
            disabled={busy || !metadataReady}
            onClick={() => void confirm()}
          >
            {pending === "confirm"
              ? "Saving…"
              : summary.count === 0
                ? "Add"
                : "Download"}
          </button>
        </div>
      </div>
    </div>
  );
}

function draftSummary(
  torrent: TorrentRow,
  base: "current" | "all" | "none",
  overrides: ReadonlyMap<number, DraftOverride>,
): { count: number; bytes: bigint } {
  let count =
    base === "all"
      ? (torrent.selectableFileCount ?? 0)
      : base === "none"
        ? 0
        : (torrent.selectedFileCount ?? 0);
  let bytes = BigInt(
    base === "all"
      ? (torrent.selectableFileBytes ?? "0")
      : base === "none"
        ? "0"
        : (torrent.selectedFileBytes ?? "0"),
  );
  for (const entry of overrides.values()) {
    const baseline =
      base === "all" ? true : base === "none" ? false : entry.initialSelected;
    if (entry.selected === baseline) continue;
    const length = BigInt(entry.lengthBytes);
    count += entry.selected ? 1 : -1;
    bytes += entry.selected ? length : -length;
  }
  return { count, bytes };
}

function compactOverrides(
  overrides: ReadonlyMap<number, DraftOverride>,
): readonly {
  readonly start: number;
  readonly endExclusive: number;
  readonly selected: boolean;
}[] {
  const sorted = [...overrides.entries()].sort(([left], [right]) => left - right);
  const ranges: { start: number; endExclusive: number; selected: boolean }[] = [];
  for (const [index, entry] of sorted) {
    const previous = ranges.at(-1);
    if (previous !== undefined && previous.endExclusive === index && previous.selected === entry.selected) {
      previous.endExclusive += 1;
    } else {
      ranges.push({ start: index, endExclusive: index + 1, selected: entry.selected });
    }
  }
  return ranges;
}
