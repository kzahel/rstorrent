import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";

import type { TorrentRow } from "../model";

import styles from "./RemoveTorrentDialog.module.css";

export interface RemoveTorrentDialogProps {
  readonly targets: readonly TorrentRow[];
  readonly deleteDataSupported: boolean;
  readonly returnFocus: () => void;
  readonly onCancel: () => void;
  readonly onConfirm: (deleteData: boolean) => Promise<void>;
}

export function RemoveTorrentDialog({
  targets,
  deleteDataSupported,
  returnFocus,
  onCancel,
  onConfirm,
}: RemoveTorrentDialogProps) {
  const [deleteData, setDeleteData] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const deleteDataRef = useRef<HTMLInputElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLFormElement>(null);
  const unsupportedDeleteCount = targets.filter(
    (target) => !target.deleteDataSupported,
  ).length;

  useEffect(() => {
    cancelRef.current?.focus();
    return returnFocus;
  }, [returnFocus]);

  useEffect(() => {
    if (pending) dialogRef.current?.focus();
  }, [pending]);

  useEffect(() => {
    if (!pending && error !== "") confirmRef.current?.focus();
  }, [error, pending]);

  useEffect(() => {
    if (!deleteDataSupported) setDeleteData(false);
  }, [deleteDataSupported]);

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLFormElement>) => {
    if (event.key === "Escape" && !pending) {
      event.preventDefault();
      onCancel();
      return;
    }
    if (event.key !== "Tab") return;
    const first = pending
      ? dialogRef.current
      : deleteDataSupported
        ? deleteDataRef.current
        : cancelRef.current;
    const last = pending ? dialogRef.current : confirmRef.current;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setPending(true);
    setError("");
    try {
      await onConfirm(deleteData);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setPending(false);
    }
  };

  return (
    <div className={styles.backdrop}>
      <form
        ref={dialogRef}
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="remove-torrent-title"
        aria-describedby="remove-torrent-description"
        tabIndex={-1}
        onKeyDown={handleKeyDown}
        onSubmit={(event) => void submit(event)}
      >
        <h2 id="remove-torrent-title">
          {targets.length === 1
            ? "Remove torrent?"
            : `Remove ${targets.length.toLocaleString()} torrents?`}
        </h2>
        <p id="remove-torrent-description">
          {targets.length === 1 ? (
            <>
              Remove <strong>{targets[0]?.name}</strong> from RSTorrent.
            </>
          ) : (
            <>Remove the selected torrents from RSTorrent.</>
          )}{" "}
          Downloaded data is kept by default.
        </p>
        {targets.length <= 1 ? null : (
          <ul className={styles.targetList} aria-label="Torrents to remove">
            {targets.slice(0, 5).map((target) => (
              <li key={target.id}>{target.name}</li>
            ))}
            {targets.length <= 5 ? null : (
              <li>and {(targets.length - 5).toLocaleString()} more</li>
            )}
          </ul>
        )}
        <label className={styles.option}>
          <input
            ref={deleteDataRef}
            type="checkbox"
            checked={deleteData}
            disabled={!deleteDataSupported || pending}
            onChange={(event) => setDeleteData(event.currentTarget.checked)}
          />
          Also delete downloaded data
        </label>
        {!deleteDataSupported ? (
          <p className={styles.note}>
            {unsupportedDeleteCount.toLocaleString()} selected{" "}
            {unsupportedDeleteCount === 1 ? "torrent does" : "torrents do"}
            {" "}not support downloaded-data deletion. Keep downloaded data to
            remove the complete selection.
          </p>
        ) : null}
        {deleteData ? (
          <p className={styles.warning} role="alert">
            This permanently deletes this torrent's downloaded files and part data. It cannot be undone.
          </p>
        ) : null}
        {error === "" ? null : (
          <p className={styles.error} role="alert">{error}</p>
        )}
        <div className={styles.actions}>
          <button ref={cancelRef} type="button" disabled={pending} onClick={onCancel}>
            Cancel
          </button>
          <button
            ref={confirmRef}
            className={styles.remove}
            type="submit"
            disabled={pending}
          >
          {pending
            ? `Removing ${targets.length.toLocaleString()}…`
            : error === ""
              ? deleteData
                ? "Remove and delete data"
                : "Remove"
              : "Retry failed"}
          </button>
        </div>
      </form>
    </div>
  );
}
