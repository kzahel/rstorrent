import { Icon } from "./Icon";
import type { FileActionId, ResolvedFileAction } from "../file-actions";
import { FileActionMenuItems } from "./FileActionMenuItems";
import {
  ActionMenuPopover,
  ActionMenuTrigger,
  OverlayButton,
} from "./overlays/AnchoredOverlay";
import styles from "./FileActionsMenu.module.css";

export function FileActionsMenu({
  pending,
  actions,
  onAction,
}: {
  readonly pending: boolean;
  readonly actions: readonly ResolvedFileAction[];
  readonly onAction: (actionId: FileActionId) => void;
}) {
  const description = actions.find((action) => action.disabledReason)?.disabledReason;

  return (
    <ActionMenuTrigger isDisabled={pending}>
      <OverlayButton
        className={styles.trigger!}
        aria-label="More file actions"
        isDisabled={pending}
      >
        More <Icon name="chevronDown" />
      </OverlayButton>
      <ActionMenuPopover description={description}>
        <FileActionMenuItems actions={actions} onAction={onAction} />
      </ActionMenuPopover>
    </ActionMenuTrigger>
  );
}
