import {
  useId,
  useMemo,
  useRef,
  useState,
  type FormEvent,
} from "react";

import {
  useInspectionCommand,
  useInspectionDispatch,
  useInspectionStore,
} from "../context";
import type { DownloadRoot, InspectionCommand, TorrentRow } from "../model";
import type { TestTorrentShortcut } from "../testTorrents";
import { validateTorrentInput } from "../torrentInput";
import { Icon } from "./Icon";
import { AddTorrentDialog } from "./AddTorrentDialog";
import { MoreActionsMenu } from "./MoreActionsMenu";
import { RemoveTorrentDialog } from "./RemoveTorrentDialog";
import styles from "./TorrentActions.module.css";

type BatchCommand = "pause" | "resume" | "archive" | "unarchive";

interface PendingAdd {
  readonly magnet: string;
  readonly clearInputOnSuccess: boolean;
}

export function TorrentActions() {
  const torrents = useInspectionStore((state) => state.torrents);
  const selectedTorrentIds = useInspectionStore(
    (state) => state.presentation.selectedTorrentIds,
  );
  const demo = useInspectionStore((state) => state.demo);
  const storage = useInspectionStore((state) => state.storage);
  const dispatch = useInspectionDispatch();
  const execute = useInspectionCommand();
  const [status, setStatus] = useState("");
  const [torrentInput, setTorrentInput] = useState("");
  const [inputInvalid, setInputInvalid] = useState(false);
  const [adding, setAdding] = useState(false);
  const [pendingAdd, setPendingAdd] = useState<PendingAdd | null>(null);
  const [removeTarget, setRemoveTarget] = useState<TorrentRow | undefined>();
  const addingRef = useRef(false);
  const addInputRef = useRef<HTMLInputElement>(null);
  const removeButtonRef = useRef<HTMLButtonElement>(null);
  const statusId = useId();
  const targetRows = useMemo(
    () => {
      return selectedTorrentIds
        .map((id) => torrents[id])
        .filter((row): row is TorrentRow => row !== undefined);
    },
    [selectedTorrentIds, torrents],
  );
  const canStart =
    targetRows.length > 0 &&
    targetRows.every(
      (row) =>
        row.removalState === null &&
        row.status !== "downloading" &&
        row.status !== "metadata",
    );
  const canPause =
    targetRows.length > 0 &&
    targetRows.every(
      (row) =>
        row.removalState === null &&
        row.status !== "paused" &&
        row.status !== "complete",
    );
  const archiveState =
    targetRows.length > 0 &&
    targetRows.every(
      (row) => row.archived !== null && row.removalState === null,
    ) &&
    targetRows.every((row) => row.archived === targetRows[0]?.archived)
      ? targetRows[0]?.archived
      : undefined;
  const removeCandidate = targetRows.length === 1 ? targetRows[0] : undefined;
  const copyMagnetCandidate =
    targetRows.length === 1 ? targetRows[0] : undefined;
  const canRemove =
    removeCandidate !== undefined &&
    (removeCandidate.removalState === null ||
      removeCandidate.removalState === "failed");

  const send = async (command: InspectionCommand): Promise<boolean> => {
    try {
      setStatus(await dispatch(command));
      return true;
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
      return false;
    }
  };

  const sendBatch = async (type: BatchCommand, rows: readonly TorrentRow[]) => {
    let completed = 0;
    const failures: string[] = [];
    for (const row of rows) {
      try {
        await dispatch({ type, torrentId: row.id });
        completed += 1;
      } catch (error) {
        failures.push(
          `${row.name}: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
    }
    const action = batchActionLabel(type);
    if (failures.length === 0) {
      setStatus(
        rows.length === 1
          ? `${action} ${rows[0]?.name ?? "torrent"}`
          : `${action} ${completed.toLocaleString()} torrents`,
      );
    } else {
      setStatus(
        `${action} ${completed.toLocaleString()} of ${rows.length.toLocaleString()}; ${failures[0]}`,
      );
    }
  };

  const beginAdd = async (source: string, clearInputOnSuccess: boolean) => {
    if (addingRef.current) return false;
    const validated = validateTorrentInput(source);
    if (!validated.accepted) {
      setInputInvalid(true);
      setStatus(validated.message);
      return false;
    }
    setInputInvalid(false);
    const defaultRoot = storage.roots.find(
      (root) =>
        root.id === storage.defaultRoot && root.availability === "available",
    );
    if (storage.showAddOptions || defaultRoot === undefined) {
      setPendingAdd({ magnet: validated.magnet, clearInputOnSuccess });
      return true;
    }
    return addToRoot(validated.magnet, defaultRoot.id, clearInputOnSuccess);
  };

  const addToRoot = async (
    magnet: string,
    storageRoot: string,
    clearInputOnSuccess: boolean,
    startContent = true,
  ) => {
    addingRef.current = true;
    setAdding(true);
    try {
      const accepted = await send({
        type: "add_magnet",
        magnet,
        storageRoot,
        startContent,
      });
      if (accepted && clearInputOnSuccess) setTorrentInput("");
      return accepted;
    } finally {
      addingRef.current = false;
      setAdding(false);
    }
  };

  const addTorrent = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    await beginAdd(torrentInput, true);
  };

  const addTestTorrent = async (torrent: TestTorrentShortcut) => {
    await beginAdd(torrent.magnet, false);
  };

  const copyMagnetLink = async () => {
    if (copyMagnetCandidate === undefined) return;
    try {
      const clipboard = navigator.clipboard;
      if (clipboard === undefined) {
        throw new Error("Clipboard access is unavailable");
      }
      await clipboard.writeText(
        `magnet:?xt=urn:btih:${copyMagnetCandidate.infoHash}`,
      );
      setStatus("Magnet link copied");
    } catch (error) {
      setStatus(
        `Could not copy magnet link: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  };

  const chooseFolder = async (
    repairRoot?: string,
  ): Promise<DownloadRoot | null> => {
    const result = await execute({
      type: "choose_download_root",
      ...(repairRoot === undefined ? {} : { repairRoot }),
    });
    setStatus(result.message);
    return result.storageRoot ?? null;
  };

  const confirmAdd = async (
    rootId: string,
    dontShowAgain: boolean,
    startContent: boolean,
  ) => {
    if (pendingAdd === null || addingRef.current) return;
    addingRef.current = true;
    setAdding(true);
    try {
      const result = await execute({
        type: "add_magnet",
        magnet: pendingAdd.magnet,
        storageRoot: rootId,
        startContent,
      });
      let message = result.message;
      if (dontShowAgain) {
        try {
          const preference = await execute({
            type: "set_show_add_options",
            show: false,
          });
          message = `${result.message}. ${preference.message}.`;
        } catch (error) {
          message = `${result.message}; ${
            error instanceof Error ? error.message : String(error)
          }`;
        }
      }
      if (pendingAdd.clearInputOnSuccess) setTorrentInput("");
      setPendingAdd(null);
      setStatus(message);
    } finally {
      addingRef.current = false;
      setAdding(false);
    }
  };

  const removeTorrent = async (deleteData: boolean) => {
    if (removeTarget === undefined) return;
    try {
      const message = await dispatch({
        type: "remove",
        torrentId: removeTarget.id,
        deleteData,
      });
      setStatus(message);
      setRemoveTarget(undefined);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
      throw error;
    }
  };

  return (
    <>
      <div className={styles.toolbar}>
        {demo === null ? (
          <form
            className={styles.addForm}
            aria-label="Add torrent"
            onSubmit={(event) => void addTorrent(event)}
          >
            <input
              ref={addInputRef}
              className={styles.addInput}
              type="text"
              value={torrentInput}
              aria-label="Magnet link or torrent URL"
              aria-describedby={statusId}
              aria-invalid={inputInvalid}
              autoComplete="off"
              spellCheck={false}
              placeholder="Magnet link or URL"
              onChange={(event) => {
                setTorrentInput(event.currentTarget.value);
                setInputInvalid(false);
              }}
            />
            <button
              className={styles.addButton}
              type="submit"
              disabled={adding}
            >
              {adding ? "Adding…" : "Add"}
            </button>
          </form>
        ) : (
          <>
            <button
              type="button"
              className={styles.primaryAction}
              onClick={() => void send({ type: "add_demo_torrent" })}
            >
              <Icon name="plus" /> Add demo
            </button>
            <span className={styles.divider} aria-hidden="true" />
          </>
        )}
        <button
          type="button"
          disabled={!canStart}
          onClick={() => void sendBatch("resume", targetRows)}
        >
          <Icon name="play" /> Start
        </button>
        <button
          type="button"
          disabled={!canPause}
          onClick={() => void sendBatch("pause", targetRows)}
        >
          <Icon name="pause" /> Pause
        </button>
        {demo === null || targetRows.length > 0 ? (
          <MoreActionsMenu
            disabled={adding}
            copyMagnetDisabled={copyMagnetCandidate === undefined}
            showTestTorrents={demo === null}
            onCopyMagnet={copyMagnetLink}
            onAddTestTorrent={addTestTorrent}
          />
        ) : null}
        <button
          type="button"
          disabled={archiveState === undefined}
          title={
            archiveState === undefined && targetRows.length > 1
              ? "Select torrents with the same archive state"
              : undefined
          }
          onClick={() =>
            archiveState === undefined
              ? undefined
              : void sendBatch(
                  archiveState ? "unarchive" : "archive",
                  targetRows,
                )
          }
        >
          <Icon name={archiveState ? "restore" : "archive"} />
          {archiveState ? "Restore" : "Archive"}
        </button>
        <button
          ref={removeButtonRef}
          type="button"
          disabled={!canRemove}
          title={
            targetRows.length > 1
              ? "Remove one torrent at a time"
              : undefined
          }
          onClick={() => setRemoveTarget(removeCandidate)}
        >
          <Icon name="remove" /> Remove
        </button>
        <output id={statusId} className={styles.commandStatus} aria-live="polite">
          {status}
        </output>
      </div>
      {removeTarget === undefined ? null : (
        <RemoveTorrentDialog
          torrentName={removeTarget.name}
          deleteDataSupported={removeTarget.deleteManagedDataSupported}
          returnFocus={removeButtonRef}
          onCancel={() => setRemoveTarget(undefined)}
          onConfirm={removeTorrent}
        />
      )}
      {pendingAdd === null ? null : (
        <AddTorrentDialog
          roots={storage.roots}
          defaultRoot={storage.defaultRoot}
          returnFocus={addInputRef}
          onChooseFolder={chooseFolder}
          onCancel={() => setPendingAdd(null)}
          onConfirm={confirmAdd}
        />
      )}
    </>
  );
}

function batchActionLabel(type: BatchCommand): string {
  switch (type) {
    case "pause":
      return "Paused";
    case "resume":
      return "Started";
    case "archive":
      return "Archived";
    case "unarchive":
      return "Restored";
  }
}
