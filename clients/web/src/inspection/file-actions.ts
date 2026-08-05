export type FileActionId = "normal" | "skip";

export interface FileActionDefinition {
  readonly id: FileActionId;
  readonly label: string;
  readonly group: "priority";
  readonly priority: "normal" | "skip";
}

export interface ResolvedFileAction extends FileActionDefinition {
  readonly disabled: boolean;
  readonly disabledReason?: string;
}

export const FILE_ACTIONS: readonly FileActionDefinition[] = [
  { id: "normal", label: "Normal", group: "priority", priority: "normal" },
  { id: "skip", label: "Skip", group: "priority", priority: "skip" },
];

export function resolveFileActions(
  targetCount: number,
  pending: boolean,
  unavailableReason?: string,
): readonly ResolvedFileAction[] {
  const disabledReason = pending
    ? "Another file action is still in progress."
    : targetCount === 0
      ? "Select a file to use these actions."
      : unavailableReason;
  return FILE_ACTIONS.map((action) => ({
    ...action,
    disabled: disabledReason !== undefined,
    ...(disabledReason === undefined ? {} : { disabledReason }),
  }));
}
