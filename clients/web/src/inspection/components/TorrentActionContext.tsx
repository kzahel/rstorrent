import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import {
  useInspectionCommand,
  useInspectionDispatch,
  useInspectionStore,
} from "../context";
import type { InspectionCommand, TorrentRow } from "../model";
import {
  TORRENT_ACTIONS,
  orderedSelectedTorrentRows,
  torrentActionAvailability,
  type TorrentActionDefinition,
  type TorrentActionId,
} from "../torrent-actions";
import { RemoveTorrentDialog } from "./RemoveTorrentDialog";

const MAX_FAILURE_DETAILS = 5;

export interface ResolvedTorrentAction extends TorrentActionDefinition {
  readonly resolvedLabel: string;
  readonly disabled: boolean;
  readonly disabledReason?: string;
}

export type TorrentActionOrigin =
  | { readonly type: "toolbar"; readonly element: HTMLElement | null }
  | {
      readonly type: "row";
      readonly tableId: string;
      readonly rowId: string;
    };

interface RemoveRequest {
  readonly targets: readonly TorrentRow[];
  readonly origin: TorrentActionOrigin;
}

interface TorrentActionContextValue {
  readonly status: string;
  readonly pendingAction: TorrentActionId | null;
  readonly selectedTargetIds: readonly string[];
  readonly actionsFor: (targetIds?: readonly string[]) => readonly ResolvedTorrentAction[];
  readonly setStatus: (status: string) => void;
  readonly runAction: (
    actionId: TorrentActionId,
    targetIds?: readonly string[],
    origin?: TorrentActionOrigin,
  ) => Promise<void>;
}

const TorrentActionContext = createContext<TorrentActionContextValue | null>(
  null,
);

export function TorrentActionProvider({ children }: { readonly children: ReactNode }) {
  const order = useInspectionStore((state) => state.torrentOrder);
  const torrents = useInspectionStore((state) => state.torrents);
  const selectedTargetIds = useInspectionStore(
    (state) => state.presentation.selectedTorrentIds,
  );
  const dispatch = useInspectionDispatch();
  const execute = useInspectionCommand();
  const [status, setStatusState] = useState("");
  const [pendingAction, setPendingAction] = useState<TorrentActionId | null>(
    null,
  );
  const [removeRequest, setRemoveRequest] = useState<RemoveRequest | null>(null);
  const removeOriginRef = useRef<TorrentActionOrigin>({
    type: "toolbar",
    element: null,
  });
  const mountedRef = useRef(true);

  useEffect(
    () => () => {
      mountedRef.current = false;
    },
    [],
  );

  const setStatus = useCallback((nextStatus: string) => {
    if (mountedRef.current) setStatusState(nextStatus);
  }, []);

  const targetRows = useCallback(
    (targetIds: readonly string[]) =>
      orderedSelectedTorrentRows(order, torrents, new Set(targetIds)),
    [order, torrents],
  );

  const actionsFor = useCallback(
    (requestedIds: readonly string[] = selectedTargetIds) => {
      const targets = targetRows(requestedIds);
      return TORRENT_ACTIONS.map((action): ResolvedTorrentAction => {
        const availability = torrentActionAvailability(action.id, targets);
        const busy = pendingAction !== null;
        return {
          ...action,
          resolvedLabel: action.label(targets.length),
          disabled: busy || availability.disabled,
          ...(busy
            ? { disabledReason: "Another torrent action is still in progress." }
            : availability.reason === undefined
              ? {}
              : { disabledReason: availability.reason }),
        };
      });
    },
    [pendingAction, selectedTargetIds, targetRows],
  );

  const runSequential = useCallback(
    async (
      actionId: Exclude<TorrentActionId, "copy_magnet" | "remove">,
      targets: readonly TorrentRow[],
    ) => {
      setPendingAction(actionId);
      setStatusState("");
      let completed = 0;
      let failureCount = 0;
      const failures: string[] = [];
      const action = TORRENT_ACTIONS.find((candidate) => candidate.id === actionId)!;
      try {
        for (const target of targets) {
          if (!mountedRef.current) break;
          setStatusState(
            `${action.pendingLabel} ${completed + failureCount + 1} of ${targets.length}…`,
          );
          try {
            await dispatch(commandFor(actionId, target.id));
            completed += 1;
          } catch (error) {
            failureCount += 1;
            if (failures.length < MAX_FAILURE_DETAILS) {
              failures.push(`${target.name}: ${errorText(error)}`);
            }
          }
        }
        if (!mountedRef.current) return;
        setStatusState(
          resultMessage(actionId, targets, completed, failureCount, failures),
        );
      } finally {
        if (mountedRef.current) setPendingAction(null);
      }
    },
    [dispatch],
  );

  const copyMagnets = useCallback(
    async (targets: readonly TorrentRow[]) => {
      setPendingAction("copy_magnet");
      setStatusState("");
      try {
        const clipboard = navigator.clipboard;
        if (clipboard === undefined) {
          throw new Error("Clipboard access is unavailable");
        }
        const exported = (async () => {
          const magnets: string[] = [];
          let omittedTrackerCount = 0;
          for (const target of targets) {
            const result = await execute({
              type: "export_magnet",
              torrentId: target.id,
            });
            if (result.magnetExport === undefined) {
              throw new Error("Magnet export did not return a link");
            }
            magnets.push(result.magnetExport.magnet);
            omittedTrackerCount += result.magnetExport.omittedTrackerCount;
          }
          return { text: magnets.join("\n"), omittedTrackerCount };
        })();
        let exportResult: Awaited<typeof exported>;
        if (
          typeof clipboard.write === "function" &&
          typeof ClipboardItem !== "undefined"
        ) {
          await clipboard.write([
            new ClipboardItem({
              "text/plain": exported.then(
                ({ text }) => new Blob([text], { type: "text/plain" }),
              ),
            }),
          ]);
          exportResult = await exported;
        } else {
          exportResult = await exported;
          await clipboard.writeText(exportResult.text);
        }
        if (mountedRef.current) {
          const copied =
            targets.length === 1
              ? "Magnet link copied"
              : `${targets.length.toLocaleString()} magnet links copied`;
          const omitted =
            exportResult.omittedTrackerCount === 0
              ? ""
              : `; ${exportResult.omittedTrackerCount.toLocaleString()} ${
                  exportResult.omittedTrackerCount === 1 ? "tracker" : "trackers"
                } omitted to keep ${targets.length === 1 ? "it" : "them"} usable`;
          setStatusState(`${copied}${omitted}`);
        }
      } catch (error) {
        if (mountedRef.current) {
          setStatusState(`Could not copy magnet links: ${errorText(error)}`);
        }
      } finally {
        if (mountedRef.current) setPendingAction(null);
      }
    },
    [execute],
  );

  const runAction = useCallback(
    async (
      actionId: TorrentActionId,
      requestedIds: readonly string[] = selectedTargetIds,
      origin: TorrentActionOrigin = { type: "toolbar", element: null },
    ) => {
      const uniqueTargetIds = new Set(requestedIds);
      const targets = targetRows(requestedIds);
      if (targets.length !== uniqueTargetIds.size) {
        setStatus("A selected torrent is no longer available.");
        return;
      }
      const availability = torrentActionAvailability(actionId, targets);
      if (pendingAction !== null) {
        setStatus("Another torrent action is still in progress.");
        return;
      }
      if (availability.disabled) {
        setStatus(availability.reason ?? "This torrent action is unavailable.");
        return;
      }
      if (actionId === "copy_magnet") {
        await copyMagnets(targets);
      } else if (actionId === "remove") {
        removeOriginRef.current = origin;
        setRemoveRequest({ targets, origin });
      } else {
        await runSequential(actionId, targets);
      }
    },
    [copyMagnets, pendingAction, runSequential, selectedTargetIds, setStatus, targetRows],
  );

  const removeTargets = useCallback(
    async (deleteData: boolean) => {
      const request = removeRequest;
      if (request === null) return;
      setPendingAction("remove");
      setStatusState("");
      let completed = 0;
      let failureCount = 0;
      const failures: { readonly target: TorrentRow; readonly message: string }[] = [];
      try {
        for (const target of request.targets) {
          if (!mountedRef.current) break;
          setStatusState(
            `Removing ${completed + failureCount + 1} of ${request.targets.length}…`,
          );
          try {
            await dispatch({
              type: "remove",
              torrentId: target.id,
              deleteData,
            });
            completed += 1;
          } catch (error) {
            failureCount += 1;
            failures.push({ target, message: errorText(error) });
          }
        }
        if (!mountedRef.current) return;
        if (failures.length === 0) {
          setStatusState(
            request.targets.length === 1
              ? `Removed ${request.targets[0]?.name ?? "torrent"}`
              : `Removed ${completed.toLocaleString()} torrents`,
          );
          setRemoveRequest(null);
          return;
        }
        const details = failures
          .slice(0, MAX_FAILURE_DETAILS)
          .map((failure) => `${failure.target.name}: ${failure.message}`);
        const remaining = Math.max(0, failures.length - details.length);
        const message = `Removed ${completed.toLocaleString()} of ${request.targets.length.toLocaleString()}; ${failureCount.toLocaleString()} failed: ${details.join("; ")}${
          remaining === 0 ? "" : `; ${remaining.toLocaleString()} more failed`
        }`;
        setStatusState(message);
        setRemoveRequest({
          targets: failures.map((failure) => failure.target),
          origin: request.origin,
        });
        throw new Error(message);
      } finally {
        if (mountedRef.current) setPendingAction(null);
      }
    },
    [dispatch, removeRequest],
  );

  const value = useMemo<TorrentActionContextValue>(
    () => ({
      status,
      pendingAction,
      selectedTargetIds,
      actionsFor,
      setStatus,
      runAction,
    }),
    [actionsFor, pendingAction, runAction, selectedTargetIds, setStatus, status],
  );

  const returnRemoveFocus = useCallback(
    () => focusOrigin(removeOriginRef.current),
    [],
  );

  return (
    <TorrentActionContext.Provider value={value}>
      {children}
      {removeRequest === null ? null : (
        <RemoveTorrentDialog
          targets={removeRequest.targets}
          deleteDataSupported={removeRequest.targets.every(
            (target) => target.deleteManagedDataSupported,
          )}
          returnFocus={returnRemoveFocus}
          onCancel={() => setRemoveRequest(null)}
          onConfirm={removeTargets}
        />
      )}
    </TorrentActionContext.Provider>
  );
}

export function useTorrentActions(): TorrentActionContextValue {
  const value = useContext(TorrentActionContext);
  if (value === null) throw new Error("TorrentActionProvider is missing");
  return value;
}

function commandFor(
  actionId: Exclude<TorrentActionId, "copy_magnet" | "remove">,
  torrentId: string,
): InspectionCommand {
  switch (actionId) {
    case "start":
      return { type: "resume", torrentId };
    case "pause":
      return { type: "pause", torrentId };
    case "force_recheck":
      return { type: "force_recheck", torrentId };
    case "move_to_top":
      return { type: "move_download_to_top", torrentId };
    case "move_to_bottom":
      return { type: "move_download_to_bottom", torrentId };
    case "archive":
      return { type: "archive", torrentId };
    case "restore":
      return { type: "unarchive", torrentId };
  }
}

function resultMessage(
  actionId: Exclude<TorrentActionId, "copy_magnet" | "remove">,
  targets: readonly TorrentRow[],
  completed: number,
  failureCount: number,
  failures: readonly string[],
): string {
  const verb = resultVerb(actionId);
  if (failureCount === 0) {
    return targets.length === 1
      ? `${verb} ${targets[0]?.name ?? "torrent"}`
      : `${verb} ${completed.toLocaleString()} torrents`;
  }
  const remaining = Math.max(0, failureCount - failures.length);
  return `${verb} ${completed.toLocaleString()} of ${targets.length.toLocaleString()}; ${failureCount.toLocaleString()} failed: ${failures.join("; ")}${
    remaining === 0 ? "" : `; ${remaining.toLocaleString()} more failed`
  }`;
}

function resultVerb(
  actionId: Exclude<TorrentActionId, "copy_magnet" | "remove">,
): string {
  switch (actionId) {
    case "start":
      return "Started";
    case "pause":
      return "Paused";
    case "force_recheck":
      return "Started recheck for";
    case "move_to_top":
      return "Moved to the top of the download queue";
    case "move_to_bottom":
      return "Moved to the bottom of the download queue";
    case "archive":
      return "Archived";
    case "restore":
      return "Restored";
  }
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function focusOrigin(origin: TorrentActionOrigin): void {
  if (origin.type === "toolbar") {
    if (origin.element?.isConnected) origin.element.focus();
    return;
  }
  const table = Array.from(
    document.querySelectorAll<HTMLElement>("[data-table-id]"),
  ).find((candidate) => candidate.dataset.tableId === origin.tableId);
  const row = Array.from(
    table?.querySelectorAll<HTMLElement>("[data-row-id]") ?? [],
  ).find((candidate) => candidate.dataset.rowId === origin.rowId);
  const current = table?.querySelector<HTMLElement>(`[data-current="true"]`);
  (row ?? current ?? table)?.focus();
}
