import { Fragment } from "react";

import { TORRENT_ACTION_GROUPS, type TorrentActionId } from "../torrent-actions";
import { Icon } from "./Icon";
import type { ResolvedTorrentAction } from "./TorrentActionContext";
import {
  ActionMenuItem,
  ActionMenuSeparator,
} from "./overlays/AnchoredOverlay";

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
    <Fragment key={group}>
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
    </Fragment>
  ));
}
