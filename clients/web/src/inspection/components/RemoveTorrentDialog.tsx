import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type RefObject,
} from "react";

import styles from "./RemoveTorrentDialog.module.css";

export interface RemoveTorrentDialogProps {
  readonly torrentName: string;
  readonly deleteDataSupported: boolean;
  readonly returnFocus: RefObject<HTMLButtonElement | null>;
  readonly onCancel: () => void;
  readonly onConfirm: (deleteData: boolean) => Promise<void>;
}

export function RemoveTorrentDialog({
  torrentName,
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

  useEffect(() => {
    cancelRef.current?.focus();
    return () => returnFocus.current?.focus();
  }, [returnFocus]);

  useEffect(() => {
    if (pending) dialogRef.current?.focus();
  }, [pending]);

  useEffect(() => {
    if (!pending && error !== "") confirmRef.current?.focus();
  }, [error, pending]);

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
        <h2 id="remove-torrent-title">Remove torrent?</h2>
        <p id="remove-torrent-description">
          Remove <strong>{torrentName}</strong> from RSTorrent. Downloaded data is kept by default.
        </p>
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
          <p className={styles.note}>Managed data deletion is unavailable for this storage.</p>
        ) : null}
        {deleteData ? (
          <p className={styles.warning} role="alert">
            This permanently deletes RSTorrent-managed payload, staging, and part data. It cannot be undone.
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
            {pending ? "Removing…" : deleteData ? "Remove and delete data" : "Remove"}
          </button>
        </div>
      </form>
    </div>
  );
}
