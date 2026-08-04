import { useEffect, useState } from "react";

import {
  WEBTORRENT_TEST_TORRENTS,
  type TestTorrentShortcut,
} from "../testTorrents";
import { Icon } from "./Icon";
import {
  ActionMenuItem,
  ActionMenuPopover,
  ActionMenuTrigger,
  ActionSubmenu,
  OverlayButton,
} from "./overlays/AnchoredOverlay";
import styles from "./MoreActionsMenu.module.css";

export interface MoreActionsMenuProps {
  readonly disabled: boolean;
  readonly copyMagnetDisabled: boolean;
  readonly showTestTorrents: boolean;
  readonly onCopyMagnet: () => Promise<void>;
  readonly onAddTestTorrent: (torrent: TestTorrentShortcut) => Promise<void>;
}

export function MoreActionsMenu({
  disabled,
  copyMagnetDisabled,
  showTestTorrents,
  onCopyMagnet,
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
        <ActionMenuItem
          isDisabled={copyMagnetDisabled}
          onAction={() => void onCopyMagnet()}
        >
          <Icon name="copy" />
          <span>Copy magnet link</span>
        </ActionMenuItem>
        {showTestTorrents ? (
          <ActionSubmenu
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
        ) : null}
      </ActionMenuPopover>
    </ActionMenuTrigger>
  );
}
