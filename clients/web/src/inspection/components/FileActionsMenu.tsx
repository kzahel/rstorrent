import { message as localizedMessage } from "../../localization/runtime";
import { Icon } from "./Icon";
import type { FileActionId, ResolvedFileAction } from "../file-actions";
import {
  FileActionMenuItems,
  type DirectSaveMenuAction,
} from "./FileActionMenuItems";
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
  directSave,
  onDirectSave,
}: {
  readonly pending: boolean;
  readonly actions: readonly ResolvedFileAction[];
  readonly onAction: (actionId: FileActionId) => void;
  readonly directSave?: DirectSaveMenuAction | undefined;
  readonly onDirectSave?: (() => void) | undefined;
}) {
  const description =
    directSave?.disabledReason ??
    actions.find((action) => action.disabledReason)?.disabledReason;

  return (
    <ActionMenuTrigger isDisabled={pending}>
      <OverlayButton
        className={styles.trigger!}
        aria-label={localizedMessage("inspection.components.file.actions.menu.more.file.actions")}
        isDisabled={pending}
      >{localizedMessage("inspection.components.file.actions.menu.more")}{" "}<Icon name="chevronDown" />
      </OverlayButton>
      <ActionMenuPopover description={description}>
        <FileActionMenuItems
          actions={actions}
          onAction={onAction}
          directSave={directSave}
          onDirectSave={onDirectSave}
        />
      </ActionMenuPopover>
    </ActionMenuTrigger>
  );
}
