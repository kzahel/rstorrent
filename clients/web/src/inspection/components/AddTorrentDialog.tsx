import { message as localizedMessage } from "../../localization/runtime";
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
import {
  CrostiniStorageHelp,
  describeCrostiniStoragePath,
} from "./CrostiniStorageHelp";
import styles from "./AddTorrentDialog.module.css";

export interface AddTorrentDialogProps {
  readonly roots: readonly DownloadRoot[];
  readonly defaultRoot: string | null;
  readonly oneCurrentRoot?: boolean;
  readonly returnFocus: RefObject<HTMLInputElement | null>;
  readonly externalKind?: "magnet" | "torrent_file" | undefined;
  readonly showCrostiniStorageHelp: boolean;
  readonly fileSelectionEnabled: boolean;
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
  oneCurrentRoot = false,
  returnFocus,
  externalKind,
  showCrostiniStorageHelp,
  fileSelectionEnabled,
  onChooseFolder,
  onCancel,
  onConfirm,
}: AddTorrentDialogProps) {
  const availableRoots = useMemo(
    () =>
      roots.filter(
        (root) =>
          root.availability === "available" &&
          (!oneCurrentRoot || root.id === defaultRoot),
      ),
    [defaultRoot, oneCurrentRoot, roots],
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
      setError(localizedMessage("inspection.components.add.torrent.dialog.choose.an.available.download.folder.first"));
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
          <p>{localizedMessage("inspection.components.add.torrent.dialog.add.torrent")}</p>
          <h2 id="add-torrent-title">{localizedMessage("inspection.components.add.torrent.dialog.choose.download.options")}</h2>
        </header>
        <p id="add-torrent-description" className={styles.description}>
          {externalKind === "magnet"
            ? localizedMessage("inspection.components.add.torrent.dialog.an.external.magnet.link.requested.this.add")
            : externalKind === "torrent_file"
              ? localizedMessage("inspection.components.add.torrent.dialog.an.external.torrent.file.requested.this.add")
              : null}
          {oneCurrentRoot
            ? localizedMessage("inspection.components.add.torrent.dialog.this.torrent.will.use.android.s.current")
            : localizedMessage("inspection.components.add.torrent.dialog.choose.where.this.torrent.will.download.this")}
        </p>

        {showCrostiniStorageHelp ? <CrostiniStorageHelp /> : null}

        <fieldset className={styles.locations} disabled={busy}>
          <legend>{localizedMessage("inspection.components.add.torrent.dialog.download.location")}</legend>
          {roots.length === 0 ? (
            <p className={styles.empty}>{localizedMessage("inspection.components.add.torrent.dialog.a.download.folder.is.required.before.rstorrent")}</p>
          ) : (
            roots
              .filter((root) => !oneCurrentRoot || root.id === defaultRoot)
              .map((root) => {
                const performance = showCrostiniStorageHelp
                  ? describeCrostiniStoragePath(root.path)
                  : null;
                return (
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
                        <small>{root.path ?? localizedMessage("inspection.components.add.torrent.dialog.location.is.not.available")}</small>
                        {performance === null ? null : (
                          <small className={styles.performance}>
                            {performance}
                          </small>
                        )}
                      </span>
                    </label>
                    {root.availability === "unavailable" ? (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void chooseFolder(root.id)}
                      >
                        {choosingRoot === root.id ? localizedMessage("inspection.components.add.torrent.dialog.repairing") : localizedMessage("inspection.components.add.torrent.dialog.repair")}
                      </button>
                    ) : root.id === defaultRoot ? (
                      <span className={styles.defaultBadge}>
                        {oneCurrentRoot ? localizedMessage("inspection.components.add.torrent.dialog.current") : localizedMessage("inspection.components.add.torrent.dialog.default")}
                      </span>
                    ) : null}
                  </div>
                );
              })
          )}
          <button
            className={styles.choose}
            type="button"
            disabled={busy}
            onClick={() => void chooseFolder()}
          >
            {choosingRoot === "new" ? localizedMessage("inspection.components.add.torrent.dialog.choosing") : localizedMessage("inspection.components.add.torrent.dialog.choose.folder")}
          </button>
        </fieldset>

        {fileSelectionEnabled ? (
          <section className={styles.files}>
            <strong>{localizedMessage("inspection.components.add.torrent.dialog.choose.files.next")}</strong>
            <small>{localizedMessage("inspection.components.add.torrent.dialog.rstorrent.will.load.metadata.without.downloading.content")}</small>
          </section>
        ) : (
        <section className={styles.files}>
          <label>
            <input
              type="checkbox"
              checked={startContent}
              disabled={busy}
              onChange={(event) => setStartContent(event.currentTarget.checked)}
            />
            <span>
              <strong>{localizedMessage("inspection.components.add.torrent.dialog.start.downloading.files.when.metadata.is.available")}</strong>
              <small>{localizedMessage("inspection.components.add.torrent.dialog.turn.this.off.to.fetch.metadata.first")}</small>
            </span>
          </label>
        </section>
        )}

        {oneCurrentRoot ? null : (
          <label className={styles.preference}>
            <input
              type="checkbox"
              checked={dontShowAgain}
              disabled={busy}
              onChange={(event) =>
                setDontShowAgain(event.currentTarget.checked)
              }
            />{localizedMessage("inspection.components.add.torrent.dialog.don.t.show.these.options.again.when")}</label>
        )}

        {error === "" ? null : (
          <p className={styles.error} role="alert">
            {error}
          </p>
        )}
        <div className={styles.actions}>
          <button
            ref={cancelRef}
            type="button"
            disabled={busy}
            onClick={onCancel}
          >{localizedMessage("inspection.components.add.torrent.dialog.cancel")}</button>
          <button
            className={styles.confirm}
            type="submit"
            disabled={busy || selectedRoot === null}
          >
            {pending
              ? localizedMessage("inspection.components.add.torrent.dialog.adding")
              : fileSelectionEnabled
                ? localizedMessage("inspection.components.add.torrent.dialog.continue")
                : localizedMessage("inspection.components.add.torrent.dialog.add.torrent")}
          </button>
        </div>
      </form>
    </div>
  );
}
