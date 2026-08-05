import type { FileActionId, ResolvedFileAction } from "../file-actions";
import { ActionMenuItem, ActionMenuSection } from "./overlays/AnchoredOverlay";

export function FileActionMenuItems({
  actions,
  onAction,
}: {
  readonly actions: readonly ResolvedFileAction[];
  readonly onAction: (actionId: FileActionId) => void;
}) {
  return (
    <ActionMenuSection label="Priority">
      {actions.map((action) => (
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
  );
}
