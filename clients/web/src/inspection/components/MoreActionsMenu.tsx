import { useEffect, useState } from "react";

import {
  WEBTORRENT_TEST_TORRENTS,
  type TestTorrentShortcut,
} from "../testTorrents";
import type { TorrentActionId } from "../torrent-actions";
import { Icon } from "./Icon";
import type { ResolvedTorrentAction } from "./TorrentActionContext";
import { TorrentActionMenuItems } from "./TorrentActionMenuItems";
import {
  ActionMenuPopover,
  ActionMenuSeparator,
  ActionMenuTrigger,
  ActionMenuItem,
  ActionSubmenu,
  OverlayButton,
} from "./overlays/AnchoredOverlay";
import styles from "./MoreActionsMenu.module.css";

export interface MoreActionsMenuProps {
  readonly disabled: boolean;
  readonly actions: readonly ResolvedTorrentAction[];
  readonly showTestTorrents: boolean;
  readonly addTestDisabled: boolean;
  readonly onAction: (actionId: TorrentActionId) => void;
  readonly onAddTestTorrent: (torrent: TestTorrentShortcut) => Promise<void>;
}

export function MoreActionsMenu({
  disabled,
  actions,
  showTestTorrents,
  addTestDisabled,
  onAction,
  onAddTestTorrent,
}: MoreActionsMenuProps) {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  return (
    <ActionMenuTrigger
      isDisabled={disabled}
      isOpen={open}
      onOpenChange={setOpen}
    >
      <OverlayButton className={styles.trigger!} isDisabled={disabled}>
        More <Icon name="chevronDown" />
      </OverlayButton>
      <ActionMenuPopover>
        <TorrentActionMenuItems actions={actions} onAction={onAction} />
        {showTestTorrents ? (
          <>
            {actions.length === 0 ? null : <ActionMenuSeparator />}
            <ActionSubmenu
              isDisabled={addTestDisabled}
              trigger={
                <>
                  <Icon name="plus" />
                  <span>Add test torrent</span>
                </>
              }
            >
              {WEBTORRENT_TEST_TORRENTS.map((torrent) => (
                <ActionMenuItem
                  key={torrent.id}
                  onAction={() => void onAddTestTorrent(torrent)}
                >
                  {torrent.menuLabel}
                </ActionMenuItem>
              ))}
            </ActionSubmenu>
          </>
        ) : null}
      </ActionMenuPopover>
    </ActionMenuTrigger>
  );
}
