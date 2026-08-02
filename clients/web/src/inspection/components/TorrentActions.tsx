import {
  useId,
  useMemo,
  useRef,
  useState,
  type FormEvent,
} from "react";

import { useInspectionDispatch, useInspectionStore } from "../context";
import type { InspectionCommand, TorrentRow } from "../model";
import type { TestTorrentShortcut } from "../testTorrents";
import { validateTorrentInput } from "../torrentInput";
import { Icon } from "./Icon";
import { MoreActionsMenu } from "./MoreActionsMenu";
import { RemoveTorrentDialog } from "./RemoveTorrentDialog";
import styles from "./TorrentActions.module.css";

type BatchCommand = "pause" | "resume" | "archive" | "unarchive";

export function TorrentActions() {
  const torrents = useInspectionStore((state) => state.torrents);
  const selectedIds = useInspectionStore(
    (state) => state.presentation.selectedTorrentIds,
  );
  const demo = useInspectionStore((state) => state.demo);
  const dispatch = useInspectionDispatch();
  const [status, setStatus] = useState("");
  const [torrentInput, setTorrentInput] = useState("");
  const [inputInvalid, setInputInvalid] = useState(false);
  const [adding, setAdding] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<TorrentRow | undefined>();
  const addingRef = useRef(false);
  const removeButtonRef = useRef<HTMLButtonElement>(null);
  const statusId = useId();
  const selectedRows = useMemo(
    () =>
      selectedIds
        .map((id) => torrents[id])
        .filter((row): row is TorrentRow => row !== undefined),
    [selectedIds, torrents],
  );
  const canStart =
    selectedRows.length > 0 &&
    selectedRows.every(
      (row) =>
        row.removalState === null &&
        row.status !== "downloading" &&
        row.status !== "metadata",
    );
  const canPause =
    selectedRows.length > 0 &&
    selectedRows.every(
      (row) =>
        row.removalState === null &&
        row.status !== "paused" &&
        row.status !== "complete",
    );
  const archiveState =
    selectedRows.length > 0 &&
    selectedRows.every(
      (row) => row.archived !== null && row.removalState === null,
    ) &&
    selectedRows.every((row) => row.archived === selectedRows[0]?.archived)
      ? selectedRows[0]?.archived
      : undefined;
  const removeCandidate = selectedRows.length === 1 ? selectedRows[0] : undefined;
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

  const addMagnet = async (source: string, clearInputOnSuccess: boolean) => {
    if (addingRef.current) return false;
    const validated = validateTorrentInput(source);
    if (!validated.accepted) {
      setInputInvalid(true);
      setStatus(validated.message);
      return false;
    }
    setInputInvalid(false);
    addingRef.current = true;
    setAdding(true);
    try {
      const accepted = await send({
        type: "add_magnet",
        magnet: validated.magnet,
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
    await addMagnet(torrentInput, true);
  };

  const addTestTorrent = async (torrent: TestTorrentShortcut) => {
    if (await addMagnet(torrent.magnet, false)) {
      setStatus(`${torrent.menuLabel} added`);
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
          onClick={() => void sendBatch("resume", selectedRows)}
        >
          <Icon name="play" /> Start
        </button>
        <button
          type="button"
          disabled={!canPause}
          onClick={() => void sendBatch("pause", selectedRows)}
        >
          <Icon name="pause" /> Pause
        </button>
        {demo === null ? (
          <MoreActionsMenu
            disabled={adding}
            onAddTestTorrent={addTestTorrent}
          />
        ) : null}
        <button
          type="button"
          disabled={archiveState === undefined}
          title={
            archiveState === undefined && selectedRows.length > 1
              ? "Select torrents with the same archive state"
              : undefined
          }
          onClick={() =>
            archiveState === undefined
              ? undefined
              : void sendBatch(
                  archiveState ? "unarchive" : "archive",
                  selectedRows,
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
            selectedRows.length > 1
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
