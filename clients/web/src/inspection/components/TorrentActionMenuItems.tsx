import { TORRENT_ACTION_GROUPS, type TorrentActionGroup, type TorrentActionId } from "../torrent-actions";
import { Icon } from "./Icon";
import type { ResolvedTorrentAction } from "./TorrentActionContext";
import {
  ActionMenuItem,
  ActionMenuSection,
  ActionMenuSeparator,
} from "./overlays/AnchoredOverlay";

const GROUP_LABELS: Readonly<Record<TorrentActionGroup, string>> = {
  transfer: "Transfer",
  sharing: "Sharing",
  organization: "Organization",
  destructive: "Destructive",
};

export function TorrentActionMenuItems({
  actions,
  onAction,
}: {
  readonly actions: readonly ResolvedTorrentAction[];
  readonly onAction: (actionId: TorrentActionId) => void;
}) {
  const groups = TORRENT_ACTION_GROUPS.map((group) => ({
    group,
    actions: actions.filter((action) => action.group === group),
  })).filter(({ actions: groupActions }) => groupActions.length > 0);

  return groups.map(({ group, actions: groupActions }, index) => (
    <ActionMenuSection key={group} label={GROUP_LABELS[group]}>
      {index === 0 ? null : <ActionMenuSeparator />}
      {groupActions.map((action) => (
        <ActionMenuItem
          key={action.id}
          isDisabled={action.disabled}
          aria-description={action.disabledReason}
          onAction={() => onAction(action.id)}
        >
          <Icon name={action.icon} />
          <span>{action.resolvedLabel}</span>
        </ActionMenuItem>
      ))}
    </ActionMenuSection>
  ));
}
