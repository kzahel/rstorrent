import type { MediaFileAvailability } from "../api";

export type FileActionId = "open" | "download_now" | "normal" | "skip";

interface OpenFileActionDefinition {
  readonly id: "open";
  readonly label: string;
  readonly group: "open";
}

interface DownloadFileActionDefinition {
  readonly id: "download_now";
  readonly label: string;
  readonly group: "download";
}

interface PriorityFileActionDefinition {
  readonly id: "normal" | "skip";
  readonly label: string;
  readonly group: "priority";
  readonly priority: "normal" | "skip";
}

export type FileActionDefinition =
  | OpenFileActionDefinition
  | DownloadFileActionDefinition
  | PriorityFileActionDefinition;

export type ResolvedFileAction = FileActionDefinition & {
  readonly disabled: boolean;
  readonly disabledReason?: string;
};

export const FILE_ACTIONS: readonly FileActionDefinition[] = [
  { id: "open", label: "Open", group: "open" },
  { id: "download_now", label: "Download now", group: "download" },
  { id: "normal", label: "Normal", group: "priority", priority: "normal" },
  { id: "skip", label: "Skip", group: "priority", priority: "skip" },
];

export function resolveFileActions(
  targetCount: number,
  skippedTargetCount: number,
  pending: boolean,
  unavailableReason?: string,
  openAvailability?: MediaFileAvailability,
): readonly ResolvedFileAction[] {
  const disabledReason = pending
    ? "Another file action is still in progress."
    : targetCount === 0
      ? "Select a file to use these actions."
      : unavailableReason;
  return FILE_ACTIONS.filter(
    (action) =>
      (action.id !== "open" ||
        (targetCount === 1 && openAvailability === "available")) &&
      (action.id !== "download_now" || skippedTargetCount > 0),
  ).map((action) => ({
    ...action,
    disabled: disabledReason !== undefined,
    ...(disabledReason === undefined ? {} : { disabledReason }),
  }));
}
