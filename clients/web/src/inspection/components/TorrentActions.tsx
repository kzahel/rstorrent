import {
  useId,
  useRef,
  useState,
  type FormEvent,
} from "react";

import {
  useInspectionCommand,
  useInspectionDispatch,
  useInspectionStore,
} from "../context";
import type { DownloadRoot } from "../model";
import type { TestTorrentShortcut } from "../testTorrents";
import {
  readTorrentFile,
  torrentFileSizeError,
} from "../torrentFile";
import { validateTorrentInput } from "../torrentInput";
import { Icon } from "./Icon";
import { AddTorrentDialog } from "./AddTorrentDialog";
import { MoreActionsMenu } from "./MoreActionsMenu";
import { useTorrentActions } from "./TorrentActionContext";
import styles from "./TorrentActions.module.css";

interface PendingMagnetAdd {
  readonly type: "magnet";
  readonly magnet: string;
  readonly clearInputOnSuccess: boolean;
}

interface PendingTorrentFileAdd {
  readonly type: "torrent_file";
  readonly file: File;
}

type PendingAdd = PendingMagnetAdd | PendingTorrentFileAdd;

export function TorrentActions() {
  const demo = useInspectionStore((state) => state.demo);
  const storage = useInspectionStore((state) => state.storage);
  const dispatch = useInspectionDispatch();
  const execute = useInspectionCommand();
  const {
    status,
    pendingAction,
    selectedTargetIds,
    actionsFor,
    setStatus,
    runAction,
  } = useTorrentActions();
  const [torrentInput, setTorrentInput] = useState("");
  const [inputInvalid, setInputInvalid] = useState(false);
  const [adding, setAdding] = useState(false);
  const [pendingAdd, setPendingAdd] = useState<PendingAdd | null>(null);
  const addingRef = useRef(false);
  const addInputRef = useRef<HTMLInputElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const removeButtonRef = useRef<HTMLButtonElement>(null);
  const statusId = useId();
  const resolvedActions = actionsFor();
  const directActions = resolvedActions.filter(
    (action) => action.placement === "direct",
  );
  const overflowActions = resolvedActions.filter(
    (action) => action.placement === "overflow",
  );

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
      setPendingAdd({
        type: "magnet",
        magnet: validated.magnet,
        clearInputOnSuccess,
      });
      return true;
    }
    return addToRoot(
      {
        type: "magnet",
        magnet: validated.magnet,
        clearInputOnSuccess,
      },
      defaultRoot.id,
    );
  };

  const addToRoot = async (
    source: PendingAdd,
    storageRoot: string,
    startContent = true,
  ) => {
    if (addingRef.current) return false;
    addingRef.current = true;
    setAdding(true);
    try {
      const result = await executePendingAdd(source, storageRoot, startContent);
      setStatus(result.message);
      if (source.type === "magnet" && source.clearInputOnSuccess) {
        setTorrentInput("");
      }
      return true;
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
      return false;
    } finally {
      addingRef.current = false;
      setAdding(false);
    }
  };

  const executePendingAdd = async (
    source: PendingAdd,
    storageRoot: string,
    startContent: boolean,
  ) => {
    if (source.type === "magnet") {
      return execute({
        type: "add_magnet",
        magnet: source.magnet,
        storageRoot,
        startContent,
      });
    }
    const bytes = await readTorrentFile(source.file);
    return execute({
      type: "add_torrent_bytes",
      source: bytes,
      storageRoot,
      startContent,
    });
  };

  const addTorrent = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (addingRef.current) return;
    if (torrentInput.trim().length === 0) {
      fileInputRef.current?.click();
      return;
    }
    void beginAdd(torrentInput, true);
  };

  const selectTorrentFile = (file: File) => {
    if (addingRef.current) return;
    const sizeError = torrentFileSizeError(file.size);
    if (sizeError !== null) {
      setStatus(sizeError);
      return;
    }
    setInputInvalid(false);
    const source: PendingTorrentFileAdd = { type: "torrent_file", file };
    const defaultRoot = storage.roots.find(
      (root) =>
        root.id === storage.defaultRoot && root.availability === "available",
    );
    if (storage.showAddOptions || defaultRoot === undefined) {
      setPendingAdd(source);
      return;
    }
    void addToRoot(source, defaultRoot.id);
  };

  const addTestTorrent = async (torrent: TestTorrentShortcut) => {
    await beginAdd(torrent.magnet, false);
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
      const result = await executePendingAdd(pendingAdd, rootId, startContent);
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
      if (
        pendingAdd.type === "magnet" &&
        pendingAdd.clearInputOnSuccess
      ) {
        setTorrentInput("");
      }
      setPendingAdd(null);
      setStatus(message);
    } finally {
      addingRef.current = false;
      setAdding(false);
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
            <input
              ref={fileInputRef}
              hidden
              type="file"
              accept=".torrent,application/x-bittorrent"
              onChange={(event) => {
                const file = event.currentTarget.files?.[0];
                event.currentTarget.value = "";
                if (file !== undefined) selectTorrentFile(file);
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
              onClick={() =>
                void dispatch({ type: "add_demo_torrent" })
                  .then(setStatus)
                  .catch((error: unknown) =>
                    setStatus(error instanceof Error ? error.message : String(error)),
                  )
              }
            >
              <Icon name="plus" /> Add demo
            </button>
            <span className={styles.divider} aria-hidden="true" />
          </>
        )}
        {directActions
          .filter((action) => action.id !== "remove")
          .map((action) => (
            <button
              key={action.id}
              type="button"
              disabled={adding || action.disabled}
              title={action.disabledReason}
              onClick={() => void runAction(action.id)}
            >
              <Icon name={action.icon} /> {action.resolvedLabel}
            </button>
          ))}
        {demo === null || selectedTargetIds.length > 0 ? (
          <MoreActionsMenu
            disabled={adding || pendingAction !== null}
            actions={overflowActions}
            showTestTorrents={demo === null}
            onAction={(actionId) => void runAction(actionId)}
            onAddTestTorrent={addTestTorrent}
          />
        ) : null}
        {directActions
          .filter((action) => action.id === "remove")
          .map((action) => (
            <button
              key={action.id}
              ref={removeButtonRef}
              type="button"
              disabled={adding || action.disabled}
              title={action.disabledReason}
              onClick={() =>
                void runAction(action.id, undefined, {
                  type: "toolbar",
                  element: removeButtonRef.current,
                })
              }
            >
              <Icon name={action.icon} /> {action.resolvedLabel}
            </button>
          ))}
        <output id={statusId} className={styles.commandStatus} aria-live="polite">
          {status}
        </output>
      </div>
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
