import { useEffect, useMemo, useState } from "react";

import { useInspectionCommand, useInspectionStore } from "../context";
import {
  resolveFileActions,
  type FileActionId,
} from "../file-actions";
import { formatDecimalBytes, formatDecimalProgress } from "../format";
import type { FileRow, ViewMaterialization } from "../model";
import { FileActionsMenu } from "./FileActionsMenu";
import { FileActionMenuItems } from "./FileActionMenuItems";
import { VirtualTable, type VirtualColumn } from "./VirtualTable";
import { ActionMenuPopover } from "./overlays/AnchoredOverlay";
import styles from "./FileTable.module.css";

const COLUMNS: readonly VirtualColumn<FileRow>[] = [
  {
    id: "name",
    label: "Name",
    width: 270,
    minimumWidth: 130,
    maximumWidth: 620,
    sortValue: (row) => row.name,
    render: (row) => <span className={styles.name} title={row.name}>{row.name}</span>,
  },
  {
    id: "folder",
    label: "Folder",
    width: 210,
    minimumWidth: 100,
    maximumWidth: 520,
    minimumViewport: 520,
    sortValue: (row) => row.folder,
    render: (row) => <span title={row.folder}>{row.folder || "—"}</span>,
  },
  {
    id: "priority",
    label: "Priority",
    width: 76,
    sortValue: (row) => row.selection,
    sortOrder: ["wanted", "skipped"],
    render: (row) => (
      <span className={styles.priority} data-selection={row.selection}>
        {row.selection === "skipped" ? "Skip" : "Normal"}
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
    render: (row) => formatDecimalBytes(row.lengthBytes),
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
          <span style={{ width: formatDecimalProgress(row.doneBytes, row.lengthBytes) }} />
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
    render: (row) => formatDecimalBytes(row.doneBytes),
  },
  {
    id: "verified",
    label: "Verified",
    width: 96,
    minimumViewport: 650,
    align: "right",
    sortValue: (row) => row.verifiedBytes,
    sortKind: "decimal",
    render: (row) => formatDecimalBytes(row.verifiedBytes),
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
    render: (row) => formatDecimalBytes(row.torrentOffsetBytes),
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
    render: (row) => <span title={row.storagePath ?? undefined}>{row.storagePath ?? "—"}</span>,
  },
];

export function FileTable({ torrentId }: { readonly torrentId: string }) {
  const [currentFileId, setCurrentFileId] = useState<string | null>(null);
  const [selectedFileIds, setSelectedFileIds] = useState<ReadonlySet<string>>(
    new Set(),
  );
  const [priorityPending, setPriorityPending] = useState(false);
  const [priorityStatus, setPriorityStatus] = useState("");
  const execute = useInspectionCommand();
  const demo = useInspectionStore((state) => state.demo);
  const fileSet = useInspectionStore((state) => state.filesByTorrent[torrentId]);
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
    setPriorityPending(false);
    setPriorityStatus("");
  }, [torrentId]);

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
      : "File priority changes are unavailable in demo scenarios.";
  const toolbarActions = resolveFileActions(
    selectedFileIds.size,
    priorityPending,
    unavailableReason,
  );

  const setPriority = async (
    actionId: FileActionId,
    requestedIds: readonly string[] = [...selectedFileIds],
  ) => {
    const requested = new Set(requestedIds);
    const targetRows = rows
      .filter((row) => requested.has(row.id))
      .sort((left, right) => left.index - right.index);
    if (targetRows.length !== requested.size) {
      setPriorityStatus("A selected file is no longer available.");
      return;
    }
    const action = resolveFileActions(
      targetRows.length,
      priorityPending,
      unavailableReason,
    ).find((candidate) => candidate.id === actionId);
    if (action === undefined || action.disabled) {
      if (action?.disabledReason !== undefined) {
        setPriorityStatus(action.disabledReason);
      }
      return;
    }
    setPriorityPending(true);
    setPriorityStatus("");
    try {
      const result = await execute({
        type: "set_file_priority",
        torrentId,
        fileIndices: targetRows.map((row) => row.index),
        priority: action.priority,
      });
      setPriorityStatus(result.message);
    } catch (error) {
      setPriorityStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setPriorityPending(false);
    }
  };

  return (
    <div className={styles.filePanel}>
      <div className={styles.summary}>
        <span>{rows.length.toLocaleString()} files</span>
        {paddingCount > 0 ? <span>{paddingCount.toLocaleString()} padding hidden</span> : null}
        <span
          className={styles.storagePath}
          title={fileSet?.filesystemContentBase ?? undefined}
        >
          {fileSet?.filesystemContentBase ?? "Platform-managed storage"}
        </span>
        <output className={styles.commandStatus} aria-live="polite">
          {priorityStatus}
        </output>
        <FileActionsMenu
          pending={priorityPending}
          actions={toolbarActions}
          onAction={(actionId) => void setPriority(actionId)}
        />
      </div>
      <VirtualTable
        tableId="files"
        label="Torrent files"
        rows={rows}
        getRowId={(row) => row.id}
        columns={COLUMNS}
        interfaceSize={interfaceSize}
        currentRowId={currentFileId}
        selection={{
          selectedIds: selectedFileIds,
          getRowLabel: (row) => row.name,
          onChange: changeSelection,
        }}
        contextMenu={{
          render: (_row, targetIds) => {
            const actions = resolveFileActions(
              targetIds.length,
              priorityPending,
              unavailableReason,
            );
            return (
              <ActionMenuPopover label="File actions">
                <FileActionMenuItems
                  actions={actions}
                  onAction={(actionId) =>
                    void setPriority(actionId, targetIds)
                  }
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
  catalogState: "metadata_pending" | "available" | "torrent_missing" | undefined,
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
      if (catalogState === "metadata_pending") return "Files are available after metadata is verified.";
      if (catalogState === "torrent_missing") return "The torrent is no longer present.";
      return "This torrent has no ordinary files to display.";
  }
}
