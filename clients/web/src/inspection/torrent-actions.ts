import type { IconName } from "./components/Icon";
import type { TorrentRow } from "./model";

export type TorrentActionId =
  | "start"
  | "pause"
  | "force_recheck"
  | "copy_magnet"
  | "archive"
  | "restore"
  | "remove";

export type TorrentActionGroup =
  | "transfer"
  | "sharing"
  | "organization"
  | "destructive";

export type TorrentActionPlacement = "direct" | "overflow";

export interface TorrentActionDefinition {
  readonly id: TorrentActionId;
  readonly label: (targetCount: number) => string;
  readonly pendingLabel: string;
  readonly icon: IconName;
  readonly group: TorrentActionGroup;
  readonly placement: TorrentActionPlacement;
  readonly destructive: boolean;
}

export interface TorrentActionAvailability {
  readonly disabled: boolean;
  readonly reason?: string;
}

const label = (value: string) => () => value;

export const TORRENT_ACTIONS: readonly TorrentActionDefinition[] = [
  {
    id: "start",
    label: label("Start"),
    pendingLabel: "Starting",
    icon: "play",
    group: "transfer",
    placement: "direct",
    destructive: false,
  },
  {
    id: "pause",
    label: label("Pause"),
    pendingLabel: "Pausing",
    icon: "pause",
    group: "transfer",
    placement: "direct",
    destructive: false,
  },
  {
    id: "force_recheck",
    label: label("Force recheck"),
    pendingLabel: "Starting recheck",
    icon: "recheck",
    group: "transfer",
    placement: "overflow",
    destructive: false,
  },
  {
    id: "copy_magnet",
    label: (targetCount) =>
      targetCount === 1 ? "Copy magnet link" : "Copy magnet links",
    pendingLabel: "Copying magnet links",
    icon: "copy",
    group: "sharing",
    placement: "overflow",
    destructive: false,
  },
  {
    id: "archive",
    label: label("Archive"),
    pendingLabel: "Archiving",
    icon: "archive",
    group: "organization",
    placement: "overflow",
    destructive: false,
  },
  {
    id: "restore",
    label: label("Restore"),
    pendingLabel: "Restoring",
    icon: "restore",
    group: "organization",
    placement: "overflow",
    destructive: false,
  },
  {
    id: "remove",
    label: label("Remove"),
    pendingLabel: "Removing",
    icon: "remove",
    group: "destructive",
    placement: "direct",
    destructive: true,
  },
];

export const TORRENT_ACTION_GROUPS: readonly TorrentActionGroup[] = [
  "transfer",
  "sharing",
  "organization",
  "destructive",
];

export function torrentActionsForPlacement(
  placement: TorrentActionPlacement,
): readonly TorrentActionDefinition[] {
  return TORRENT_ACTIONS.filter((action) => action.placement === placement);
}

export function orderedSelectedTorrentRows(
  order: readonly string[],
  torrents: Readonly<Record<string, TorrentRow>>,
  selectedIds: ReadonlySet<string>,
): TorrentRow[] {
  return order
    .filter((id) => selectedIds.has(id))
    .map((id) => torrents[id])
    .filter((row): row is TorrentRow => row !== undefined);
}

export function torrentActionAvailability(
  actionId: TorrentActionId,
  targets: readonly TorrentRow[],
): TorrentActionAvailability {
  if (targets.length === 0) {
    return { disabled: true, reason: "Select a torrent to use this action." };
  }

  if (actionId === "copy_magnet") return { disabled: false };

  const activeRemovalCount = targets.filter(
    (row) => row.removalState === "pending" || row.removalState === "awaiting_platform",
  ).length;
  if (activeRemovalCount > 0) {
    return {
      disabled: true,
      reason: `${activeRemovalCount.toLocaleString()} selected ${plural(
        activeRemovalCount,
        "torrent is",
        "torrents are",
      )} already being removed.`,
    };
  }

  if (actionId !== "remove" && targets.some((row) => row.removalState !== null)) {
    return {
      disabled: true,
      reason: "A selected torrent has a failed removal that must be retried or completed.",
    };
  }

  switch (actionId) {
    case "start":
    case "pause":
    case "remove":
      return { disabled: false };
    case "force_recheck": {
      const unavailable = targets.filter((row) => !row.forceRecheckAvailable).length;
      return unavailable === 0
        ? { disabled: false }
        : {
            disabled: true,
            reason: `${unavailable.toLocaleString()} selected ${plural(
              unavailable,
              "torrent does",
              "torrents do",
            )} not have managed content available to recheck.`,
          };
    }
    case "archive":
    case "restore": {
      if (targets.some((row) => row.archived === null)) {
        return {
          disabled: true,
          reason: "Archive state is unavailable for a selected torrent.",
        };
      }
      const alreadySatisfied = targets.every((row) =>
        actionId === "archive" ? row.archived === true : row.archived === false,
      );
      return alreadySatisfied
        ? {
            disabled: true,
            reason:
              actionId === "archive"
                ? "All selected torrents are already archived."
                : "All selected torrents are already restored.",
          }
        : { disabled: false };
    }
  }
}

function plural(count: number, singular: string, pluralValue: string): string {
  return count === 1 ? singular : pluralValue;
}
