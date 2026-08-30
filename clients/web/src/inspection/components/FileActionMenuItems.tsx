import type { FileActionId, ResolvedFileAction } from "../file-actions";
import { ActionMenuItem, ActionMenuSection } from "./overlays/AnchoredOverlay";

export function FileActionMenuItems({
  actions,
  onAction,
  directSave,
  onDirectSave,
}: {
  readonly actions: readonly ResolvedFileAction[];
  readonly onAction: (actionId: FileActionId) => void;
  readonly directSave?: DirectSaveMenuAction | undefined;
  readonly onDirectSave?: (() => void) | undefined;
}) {
  const open = actions.filter((action) => action.group === "open");
  const download = actions.filter((action) => action.group === "download");
  const priority = actions.filter((action) => action.group === "priority");
  return (
    <>
      {open.length > 0 ? (
        <ActionMenuSection label="Open">
          {open.map((action) => (
            <ActionMenuItem
              key={action.id}
              isDisabled={action.disabled}
              aria-description={action.disabledReason}
              onAction={() => onAction(action.id)}
            >
              {action.label}
            </ActionMenuItem>
          ))}
          {directSave === undefined || onDirectSave === undefined ? null : (
            <ActionMenuItem
              isDisabled={directSave.disabled}
              aria-description={directSave.disabledReason}
              onAction={onDirectSave}
            >
              Save file…
            </ActionMenuItem>
          )}
        </ActionMenuSection>
      ) : null}
      {download.length > 0 ? (
        <ActionMenuSection label="Download">
          {download.map((action) => (
            <ActionMenuItem
              key={action.id}
              isDisabled={action.disabled}
              aria-description={action.disabledReason}
              onAction={() => onAction(action.id)}
            >
              {action.label}
            </ActionMenuItem>
          ))}
        </ActionMenuSection>
      ) : null}
      <ActionMenuSection label="Priority">
        {priority.map((action) => (
          <ActionMenuItem
            key={action.id}
            isDisabled={action.disabled}
            aria-description={action.disabledReason}
            onAction={() => onAction(action.id)}
          >
            {action.label}
          </ActionMenuItem>
        ))}
      </ActionMenuSection>
    </>
  );
}

export interface DirectSaveMenuAction {
  readonly disabled: boolean;
  readonly disabledReason?: string | undefined;
}
