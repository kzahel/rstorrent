import type { TorrentRow } from "../model";
import { useTorrentActions } from "./TorrentActionContext";
import { TorrentActionMenuItems } from "./TorrentActionMenuItems";
import { ActionMenuPopover } from "./overlays/AnchoredOverlay";

export function TorrentContextMenu({
  row,
  tableId,
  targetIds,
}: {
  readonly row: TorrentRow;
  readonly tableId: string;
  readonly targetIds: readonly string[];
}) {
  const { actionsFor, runAction } = useTorrentActions();

  return (
    <ActionMenuPopover label="Torrent actions">
      <TorrentActionMenuItems
        actions={actionsFor(targetIds)}
        onAction={(actionId) =>
          void runAction(actionId, targetIds, {
            type: "row",
            tableId,
            rowId: row.id,
          })
        }
      />
    </ActionMenuPopover>
  );
}
