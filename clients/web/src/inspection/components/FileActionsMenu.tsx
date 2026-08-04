import { Icon } from "./Icon";
import {
  ActionMenuItem,
  ActionMenuPopover,
  ActionMenuTrigger,
  OverlayButton,
} from "./overlays/AnchoredOverlay";
import styles from "./FileActionsMenu.module.css";

export function FileActionsMenu({
  targetCount,
  pending,
  unavailableReason,
  onPriority,
}: {
  readonly targetCount: number;
  readonly pending: boolean;
  readonly unavailableReason?: string;
  readonly onPriority: (priority: "normal" | "skip") => Promise<void>;
}) {
  const reason =
    targetCount === 0
      ? "Select a file to use these actions."
      : unavailableReason;
  const disabled = pending || reason !== undefined;

  return (
    <ActionMenuTrigger isDisabled={pending}>
      <OverlayButton
        className={styles.trigger!}
        aria-label="More file actions"
        isDisabled={pending}
      >
        More <Icon name="chevronDown" />
      </OverlayButton>
      <ActionMenuPopover description={reason}>
        <ActionMenuItem
          isDisabled={disabled}
          onAction={() => void onPriority("normal")}
        >
          Normal
        </ActionMenuItem>
        <ActionMenuItem
          isDisabled={disabled}
          onAction={() => void onPriority("skip")}
        >
          Skip
        </ActionMenuItem>
      </ActionMenuPopover>
    </ActionMenuTrigger>
  );
}
