import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
  type RefObject,
} from "react";

import type { DownloadRoot } from "../model";
import styles from "./AddTorrentDialog.module.css";

export interface AddTorrentDialogProps {
  readonly roots: readonly DownloadRoot[];
  readonly defaultRoot: string | null;
  readonly returnFocus: RefObject<HTMLInputElement | null>;
  readonly externalKind?: "magnet" | "torrent_file" | undefined;
  readonly onChooseFolder: (repairRoot?: string) => Promise<DownloadRoot | null>;
  readonly onCancel: () => void;
  readonly onConfirm: (
    rootId: string,
    dontShowAgain: boolean,
    startContent: boolean,
  ) => Promise<void>;
}

export function AddTorrentDialog({
  roots,
  defaultRoot,
  returnFocus,
  externalKind,
  onChooseFolder,
  onCancel,
  onConfirm,
}: AddTorrentDialogProps) {
  const availableRoots = useMemo(
    () => roots.filter((root) => root.availability === "available"),
    [roots],
  );
  const preferred = availableRoots.some((root) => root.id === defaultRoot)
    ? defaultRoot
    : availableRoots[0]?.id ?? null;
  const [selectedRoot, setSelectedRoot] = useState<string | null>(preferred);
  const [dontShowAgain, setDontShowAgain] = useState(false);
  const [startContent, setStartContent] = useState(true);
  const [choosingRoot, setChoosingRoot] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const dialogRef = useRef<HTMLFormElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    cancelRef.current?.focus();
    return () => returnFocus.current?.focus();
  }, [returnFocus]);

  useEffect(() => {
    if (!availableRoots.some((root) => root.id === selectedRoot)) {
      setSelectedRoot(preferred);
    }
  }, [availableRoots, preferred, selectedRoot]);

  const chooseFolder = async (repairRoot?: string) => {
    setChoosingRoot(repairRoot ?? "new");
    setError("");
    try {
      const root = await onChooseFolder(repairRoot);
      if (root !== null) setSelectedRoot(root.id);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setChoosingRoot(null);
    }
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (selectedRoot === null) {
      setError("Choose an available download folder first.");
      return;
    }
    setPending(true);
    setError("");
    try {
      await onConfirm(selectedRoot, dontShowAgain, startContent);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setPending(false);
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLFormElement>) => {
    if (event.key === "Escape" && !pending && choosingRoot === null) {
      event.preventDefault();
      onCancel();
      return;
    }
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

  const busy = pending || choosingRoot !== null;

  return (
    <div className={styles.backdrop}>
      <form
        ref={dialogRef}
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-torrent-title"
        aria-describedby="add-torrent-description"
        onKeyDown={handleKeyDown}
        onSubmit={(event) => void submit(event)}
      >
        <header>
          <p>Add torrent</p>
          <h2 id="add-torrent-title">Choose download options</h2>
        </header>
        <p id="add-torrent-description" className={styles.description}>
          {externalKind === "magnet"
            ? "An external magnet link requested this add. "
            : externalKind === "torrent_file"
              ? "An external .torrent file requested this add. "
              : null}
          Choose where this torrent will download. This location applies only to
          this torrent unless you change the default in Settings.
        </p>

        <fieldset className={styles.locations} disabled={busy}>
          <legend>Download location</legend>
          {roots.length === 0 ? (
            <p className={styles.empty}>
              A download folder is required before RSTorrent can add this torrent.
            </p>
          ) : (
            roots.map((root) => (
              <div
                key={root.id}
                className={styles.location}
                data-availability={root.availability}
              >
                <label>
                  <input
                    type="radio"
                    name="download-root"
                    value={root.id}
                    checked={selectedRoot === root.id}
                    disabled={root.availability !== "available"}
                    onChange={() => setSelectedRoot(root.id)}
                  />
                  <span>
                    <strong>{root.label}</strong>
                    <small>{root.path ?? "Location is not available"}</small>
                  </span>
                </label>
                {root.availability === "unavailable" ? (
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void chooseFolder(root.id)}
                  >
                    {choosingRoot === root.id ? "Repairing…" : "Repair…"}
                  </button>
                ) : root.id === defaultRoot ? (
                  <span className={styles.defaultBadge}>Default</span>
                ) : null}
              </div>
            ))
          )}
          <button
            className={styles.choose}
            type="button"
            disabled={busy}
            onClick={() => void chooseFolder()}
          >
            {choosingRoot === "new" ? "Choosing…" : "Choose folder…"}
          </button>
        </fieldset>

        <section className={styles.files}>
          <label>
            <input
              type="checkbox"
              checked={startContent}
              disabled={busy}
              onChange={(event) => setStartContent(event.currentTarget.checked)}
            />
            <span>
              <strong>Start downloading files when metadata is available</strong>
              <small>
                Turn this off to fetch metadata first, then choose files in the Files tab.
              </small>
            </span>
          </label>
        </section>

        <label className={styles.preference}>
          <input
            type="checkbox"
            checked={dontShowAgain}
            disabled={busy}
            onChange={(event) => setDontShowAgain(event.currentTarget.checked)}
          />
          Don’t show these options again when a usable default is available
        </label>

        {error === "" ? null : (
          <p className={styles.error} role="alert">{error}</p>
        )}
        <div className={styles.actions}>
          <button
            ref={cancelRef}
            type="button"
            disabled={busy}
            onClick={onCancel}
          >
            Cancel
          </button>
          <button
            className={styles.confirm}
            type="submit"
            disabled={busy || selectedRoot === null}
          >
            {pending ? "Adding…" : "Add torrent"}
          </button>
        </div>
      </form>
    </div>
  );
}
