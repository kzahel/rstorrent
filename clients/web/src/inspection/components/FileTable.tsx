import { useEffect, useMemo, useState } from "react";

import { useInspectionCommand, useInspectionStore } from "../context";
import type { DataUnits } from "../appearance";
import { resolveFileActions, type FileActionId } from "../file-actions";
import { formatExactBytes, formatDecimalProgress } from "../format";
import type { FileRow, ViewMaterialization } from "../model";
import type { DesktopRemoteAccess } from "../remote-access/types";
import { FileActionsMenu } from "./FileActionsMenu";
import { FileActionMenuItems } from "./FileActionMenuItems";
import { VirtualTable, type VirtualColumn } from "./VirtualTable";
import { ActionMenuPopover } from "./overlays/AnchoredOverlay";
import styles from "./FileTable.module.css";

const columns = (dataUnits: DataUnits): readonly VirtualColumn<FileRow>[] => [
  {
    id: "name",
    label: "Name",
    width: 270,
    minimumWidth: 130,
    maximumWidth: 620,
    sortValue: (row) => row.name,
    render: (row) => (
      <span className={styles.name} title={row.name}>
        {row.name}
      </span>
    ),
  },
  {
    id: "folder",
    label: "Folder",
    width: 210,
    minimumWidth: 100,
    maximumWidth: 520,
    sortValue: (row) => row.folder,
    render: (row) => <span title={row.folder}>{row.folder || "—"}</span>,
  },
  {
    id: "priority",
    label: "Priority",
    width: 76,
    sortValue: (row) => row.selection,
    sortOrder: ["high", "normal", "skipped"],
    render: (row) => (
      <span className={styles.priority} data-selection={row.selection}>
        {row.selection === "skipped"
          ? "Skip"
          : row.selection === "high"
            ? "High"
            : "Normal"}
      </span>
    ),
  },
  {
    id: "size",
    label: "Size",
    width: 92,
    align: "right",
    sortValue: (row) => row.lengthBytes,
    sortKind: "decimal",
    render: (row) => formatExactBytes(row.lengthBytes, dataUnits),
  },
  {
    id: "progress",
    label: "Progress",
    width: 108,
    minimumWidth: 88,
    align: "right",
    sortValue: (row) => progressBasisPoints(row),
    sortKind: "number",
    render: (row) => (
      <span className={styles.progress}>
        <span aria-hidden="true">
          <span
            style={{
              width: formatDecimalProgress(row.doneBytes, row.lengthBytes),
            }}
          />
        </span>
        {formatDecimalProgress(row.doneBytes, row.lengthBytes)}
      </span>
    ),
  },
  {
    id: "done",
    label: "Done",
    width: 96,
    align: "right",
    sortValue: (row) => row.doneBytes,
    sortKind: "decimal",
    render: (row) => formatExactBytes(row.doneBytes, dataUnits),
  },
  {
    id: "verified",
    label: "Verified",
    width: 96,
    align: "right",
    sortValue: (row) => row.verifiedBytes,
    sortKind: "decimal",
    render: (row) => formatExactBytes(row.verifiedBytes, dataUnits),
  },
  {
    id: "extension",
    label: "Type",
    width: 72,
    defaultVisible: false,
    sortValue: (row) => row.extension,
    render: (row) => row.extension || "—",
  },
  {
    id: "index",
    label: "Index",
    width: 68,
    align: "right",
    defaultVisible: false,
    sortValue: (row) => row.index,
    sortKind: "number",
    render: (row) => row.index.toLocaleString(),
  },
  {
    id: "offset",
    label: "Torrent offset",
    width: 118,
    align: "right",
    defaultVisible: false,
    sortValue: (row) => row.torrentOffsetBytes,
    sortKind: "decimal",
    render: (row) => formatExactBytes(row.torrentOffsetBytes, dataUnits),
  },
  {
    id: "pieces",
    label: "Pieces",
    width: 108,
    align: "right",
    defaultVisible: false,
    sortValue: (row) => row.firstPiece,
    sortKind: "number",
    render: (row) =>
      row.firstPiece === null
        ? "—"
        : row.firstPiece === row.lastPiece
          ? row.firstPiece.toLocaleString()
          : `${row.firstPiece.toLocaleString()}–${row.lastPiece?.toLocaleString()}`,
  },
  {
    id: "storagePath",
    label: "Storage Path",
    width: 520,
    minimumWidth: 180,
    maximumWidth: 900,
    defaultVisible: false,
    sortValue: (row) => row.storagePath,
    render: (row) => (
      <span title={row.storagePath ?? undefined}>{row.storagePath ?? "—"}</span>
    ),
  },
];

export function FileTable({
  torrentId,
  remoteAccess,
}: {
  readonly torrentId: string;
  readonly remoteAccess?: DesktopRemoteAccess | undefined;
}) {
  const dataUnits = useInspectionStore((state) => state.presentation.dataUnits);
  const displayColumns = useMemo(() => columns(dataUnits), [dataUnits]);
  const [currentFileId, setCurrentFileId] = useState<string | null>(null);
  const [selectedFileIds, setSelectedFileIds] = useState<ReadonlySet<string>>(
    new Set(),
  );
  const [fileActionPending, setFileActionPending] = useState(false);
  const [fileActionStatus, setFileActionStatus] = useState("");
  const [directFileEnabled, setDirectFileEnabled] = useState<boolean | null>(null);
  const [directSaveAbort, setDirectSaveAbort] = useState<AbortController | null>(null);
  const execute = useInspectionCommand();
  const demo = useInspectionStore((state) => state.demo);
  const fileSet = useInspectionStore(
    (state) => state.filesByTorrent[torrentId],
  );
  const materialization = useInspectionStore((state) => state.viewStatus.files);
  const interfaceSize = useInspectionStore(
    (state) => state.presentation.interfaceSize,
  );
  const rows = useMemo(
    () =>
      (fileSet?.order ?? [])
        .map((id) => fileSet?.rows[id])
        .filter((row): row is FileRow => row !== undefined && !row.padding),
    [fileSet],
  );
  const paddingCount = useMemo(
    () =>
      (fileSet?.order ?? []).filter((id) => fileSet?.rows[id]?.padding === true)
        .length,
    [fileSet],
  );
  const availableIds = useMemo(
    () => new Set(rows.map((row) => row.id)),
    [rows],
  );

  useEffect(() => {
    setCurrentFileId(null);
    setSelectedFileIds(new Set());
    setFileActionPending(false);
    setFileActionStatus("");
    setDirectSaveAbort((current) => {
      current?.abort();
      return null;
    });
  }, [torrentId]);

  useEffect(() => {
    let active = true;
    if (
      remoteAccess?.scope !== "remote" ||
      remoteAccess.directFileSupported?.() !== true
    ) {
      setDirectFileEnabled(false);
      return () => {
        active = false;
      };
    }
    setDirectFileEnabled(null);
    void remoteAccess
      .state()
      .then((state) => {
        if (active) setDirectFileEnabled(state.security?.direct_file.enabled === true);
      })
      .catch(() => {
        if (active) setDirectFileEnabled(false);
      });
    return () => {
      active = false;
    };
  }, [remoteAccess, torrentId]);

  useEffect(() => {
    setSelectedFileIds((current) => {
      const next = new Set([...current].filter((id) => availableIds.has(id)));
      return setsEqual(current, next) ? current : next;
    });
  }, [availableIds]);

  useEffect(() => {
    setCurrentFileId((current) =>
      current !== null && selectedFileIds.has(current)
        ? current
        : (selectedFileIds.values().next().value ?? null),
    );
  }, [selectedFileIds]);

  const changeSelection = (
    selectedIds: readonly string[],
    requestedCurrentId: string | null,
  ) => {
    const next = new Set(selectedIds.filter((id) => availableIds.has(id)));
    const nextCurrentId =
      requestedCurrentId !== null && next.has(requestedCurrentId)
        ? requestedCurrentId
        : (next.values().next().value ?? null);
    setSelectedFileIds(next);
    setCurrentFileId(nextCurrentId);
  };

  const unavailableReason =
    demo === null
      ? undefined
      : "File actions are unavailable in demo scenarios.";
  const selectedSkippedCount = rows.filter(
    (row) => selectedFileIds.has(row.id) && row.selection === "skipped",
  ).length;
  const selectedOpenAvailability =
    selectedFileIds.size === 1
      ? rows.find((row) => selectedFileIds.has(row.id))?.mediaAvailability
      : undefined;
  const toolbarActions = resolveFileActions(
    selectedFileIds.size,
    selectedSkippedCount,
    fileActionPending,
    unavailableReason,
    selectedOpenAvailability,
  );
  const selectedRows = rows.filter((row) => selectedFileIds.has(row.id));
  const toolbarDirectSave = resolveDirectSave(
    remoteAccess,
    selectedRows,
    fileActionPending,
    directFileEnabled,
    unavailableReason,
  );

  const runFileAction = async (
    actionId: FileActionId,
    requestedIds: readonly string[] = [...selectedFileIds],
  ) => {
    const requested = new Set(requestedIds);
    const targetRows = rows
      .filter((row) => requested.has(row.id))
      .sort((left, right) => left.index - right.index);
    if (targetRows.length !== requested.size) {
      setFileActionStatus("A selected file is no longer available.");
      return;
    }
    const action = resolveFileActions(
      targetRows.length,
      targetRows.filter((row) => row.selection === "skipped").length,
      fileActionPending,
      unavailableReason,
      targetRows.length === 1 ? targetRows[0]?.mediaAvailability : undefined,
    ).find((candidate) => candidate.id === actionId);
    if (action === undefined || action.disabled) {
      if (action?.disabledReason !== undefined) {
        setFileActionStatus(action.disabledReason);
      }
      return;
    }
    setFileActionPending(true);
    setFileActionStatus("");
    try {
      const fileIndices = targetRows.map((row) => row.index);
      const result = await execute(
        action.id === "open"
          ? {
              type: "open_file",
              torrentId,
              fileIndex: targetRows[0]!.index,
            }
          : action.id === "download_now"
          ? {
              type: "download_files",
              torrentId,
              fileIndices,
            }
          : {
              type: "set_file_priority",
              torrentId,
              fileIndices,
              priority: action.priority,
            },
      );
      setFileActionStatus(result.message);
    } catch (error) {
      setFileActionStatus(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setFileActionPending(false);
    }
  };

  const runDirectSave = async (
    requestedIds: readonly string[] = [...selectedFileIds],
  ) => {
    const requested = new Set(requestedIds);
    const targetRows = rows.filter((row) => requested.has(row.id));
    const action = resolveDirectSave(
      remoteAccess,
      targetRows,
      fileActionPending,
      directFileEnabled,
      unavailableReason,
    );
    if (action === undefined || action.disabled || targetRows.length !== 1) {
      if (action?.disabledReason !== undefined) setFileActionStatus(action.disabledReason);
      return;
    }
    const save = remoteAccess?.saveCompletedFile;
    if (save === undefined) return;
    const row = targetRows[0]!;
    const cancellation = new AbortController();
    setDirectSaveAbort(cancellation);
    setFileActionPending(true);
    setFileActionStatus("Choose where to save this remote file.");
    try {
      await save({
        torrentId,
        fileIndex: row.index,
        fileName: row.name,
        lengthBytes: row.lengthBytes,
        signal: cancellation.signal,
        onProgress: (progress) => {
          if (progress.state === "connecting") {
            setFileActionStatus("Connecting directly to the remote device…");
          } else if (progress.state === "transferring") {
            setFileActionStatus(
              `Saving directly… ${directProgress(progress.bytesWritten, progress.fileLength)}`,
            );
          } else if (progress.state === "complete") {
            setFileActionStatus("Remote file saved.");
          }
        },
      });
    } catch (error) {
      setFileActionStatus(
        error instanceof DOMException && error.name === "AbortError"
          ? "Remote file save cancelled."
          : error instanceof Error
            ? error.message
            : String(error),
      );
    } finally {
      setDirectSaveAbort(null);
      setFileActionPending(false);
    }
  };

  return (
    <div className={styles.filePanel}>
      <div className={styles.summary}>
        <span>{rows.length.toLocaleString()} files</span>
        {paddingCount > 0 ? (
          <span>{paddingCount.toLocaleString()} padding hidden</span>
        ) : null}
        <span
          className={styles.storagePath}
          title={fileSet?.filesystemContentBase ?? undefined}
        >
          {fileSet?.filesystemContentBase ?? "Platform-managed storage"}
        </span>
        <output className={styles.commandStatus} aria-live="polite">
          {fileActionStatus}
        </output>
        {directSaveAbort === null ? null : (
          <button
            type="button"
            className={styles.cancelSave}
            onClick={() => directSaveAbort.abort()}
          >
            Cancel save
          </button>
        )}
        <FileActionsMenu
          pending={fileActionPending}
          actions={toolbarActions}
          onAction={(actionId) => void runFileAction(actionId)}
          directSave={toolbarDirectSave}
          onDirectSave={() => void runDirectSave()}
        />
      </div>
      <VirtualTable
        tableId="files"
        label="Torrent files"
        rows={rows}
        getRowId={(row) => row.id}
        columns={displayColumns}
        interfaceSize={interfaceSize}
        currentRowId={currentFileId}
        selection={{
          selectedIds: selectedFileIds,
          getRowLabel: (row) => row.name,
          onChange: changeSelection,
        }}
        contextMenu={{
          label: "File actions",
          render: (_row, targetIds) => {
            const targetIdSet = new Set(targetIds);
            const actions = resolveFileActions(
              targetIds.length,
              rows.filter(
                (row) =>
                  targetIdSet.has(row.id) && row.selection === "skipped",
              ).length,
              fileActionPending,
              unavailableReason,
              targetIds.length === 1
                ? rows.find((row) => row.id === targetIds[0])?.mediaAvailability
                : undefined,
            );
            const directSave = resolveDirectSave(
              remoteAccess,
              rows.filter((row) => targetIdSet.has(row.id)),
              fileActionPending,
              directFileEnabled,
              unavailableReason,
            );
            return (
              <ActionMenuPopover label="File actions">
                <FileActionMenuItems
                  actions={actions}
                  onAction={(actionId) =>
                    void runFileAction(actionId, targetIds)
                  }
                  directSave={directSave}
                  onDirectSave={() => void runDirectSave(targetIds)}
                />
              </ActionMenuPopover>
            );
          },
        }}
        emptyMessage={fileEmptyMessage(materialization, fileSet?.state)}
        initialSort={{ columnId: "index", direction: "asc" }}
      />
    </div>
  );
}

function resolveDirectSave(
  remoteAccess: DesktopRemoteAccess | undefined,
  rows: readonly FileRow[],
  pending: boolean,
  enabled: boolean | null,
  unavailableReason: string | undefined,
) {
  if (remoteAccess?.scope !== "remote") return undefined;
  if (unavailableReason !== undefined) return { disabled: true, disabledReason: unavailableReason };
  if (rows.length !== 1) {
    return { disabled: true, disabledReason: "Select one completed file to save." };
  }
  if (rows[0]?.mediaAvailability !== "available") {
    return { disabled: true, disabledReason: "Only completed, verified files can be saved remotely." };
  }
  if (remoteAccess.directFileSupported?.() !== true || remoteAccess.saveCompletedFile === undefined) {
    return { disabled: true, disabledReason: "This remote host does not support direct file saves." };
  }
  if (enabled === null) {
    return { disabled: true, disabledReason: "Checking direct file transfer settings…" };
  }
  if (!enabled) {
    return { disabled: true, disabledReason: "Direct file transfers are disabled on the host." };
  }
  if (pending) return { disabled: true, disabledReason: "Another file action is in progress." };
  return { disabled: false };
}

function directProgress(written: bigint, length: bigint): string {
  if (length === 0n) return "100%";
  const basisPoints = (written * 10_000n) / length;
  return `${Number(basisPoints) / 100}%`;
}

function setsEqual(left: ReadonlySet<string>, right: ReadonlySet<string>) {
  if (left.size !== right.size) return false;
  for (const value of left) {
    if (!right.has(value)) return false;
  }
  return true;
}

function progressBasisPoints(row: FileRow): number {
  try {
    const length = BigInt(row.lengthBytes);
    if (length === 0n) return 10_000;
    return Number((BigInt(row.doneBytes) * 10_000n) / length);
  } catch {
    return 0;
  }
}

function fileEmptyMessage(
  materialization: ViewMaterialization,
  catalogState:
    "metadata_pending" | "available" | "torrent_missing" | undefined,
): string {
  switch (materialization.status) {
    case "not_requested":
      return "File inspection is not requested.";
    case "loading":
      return "Loading file catalog…";
    case "unavailable":
    case "unsupported":
    case "stale":
      return materialization.reason;
    case "ready":
      if (catalogState === "metadata_pending")
        return "Files are available after metadata is verified.";
      if (catalogState === "torrent_missing")
        return "The torrent is no longer present.";
      return "This torrent has no ordinary files to display.";
  }
}
