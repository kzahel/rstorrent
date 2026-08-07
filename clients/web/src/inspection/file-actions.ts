export type FileActionId = "download_now" | "normal" | "skip";

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
  | DownloadFileActionDefinition
  | PriorityFileActionDefinition;

export type ResolvedFileAction = FileActionDefinition & {
  readonly disabled: boolean;
  readonly disabledReason?: string;
};

export const FILE_ACTIONS: readonly FileActionDefinition[] = [
  { id: "download_now", label: "Download now", group: "download" },
  { id: "normal", label: "Normal", group: "priority", priority: "normal" },
  { id: "skip", label: "Skip", group: "priority", priority: "skip" },
];

export function resolveFileActions(
  targetCount: number,
  skippedTargetCount: number,
  pending: boolean,
  unavailableReason?: string,
): readonly ResolvedFileAction[] {
  const disabledReason = pending
    ? "Another file action is still in progress."
    : targetCount === 0
      ? "Select a file to use these actions."
      : unavailableReason;
  return FILE_ACTIONS.filter(
    (action) => action.id !== "download_now" || skippedTargetCount > 0,
  ).map((action) => ({
    ...action,
    disabled: disabledReason !== undefined,
    ...(disabledReason === undefined ? {} : { disabledReason }),
  }));
}
