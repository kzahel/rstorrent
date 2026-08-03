import type { DetailTab, DesiredInspectionViews } from "./model";

export const DETAIL_TABS: readonly {
  readonly id: DetailTab;
  readonly label: string;
  readonly scope: "torrent" | "session";
  readonly view: DesiredInspectionViews["detail"];
}[] = [
  { id: "general", label: "General", scope: "torrent", view: "general" },
  { id: "trackers", label: "Trackers", scope: "torrent", view: "trackers" },
  { id: "peers", label: "Peers", scope: "torrent", view: "peers" },
  { id: "swarm", label: "Swarm", scope: "torrent", view: "swarm" },
  { id: "files", label: "Files", scope: "torrent", view: "files" },
  { id: "pieces", label: "Pieces", scope: "torrent", view: "pieces" },
  { id: "disk", label: "Disk", scope: "session", view: "disk" },
  { id: "logs", label: "Logs", scope: "session", view: "logs" },
  { id: "speed", label: "Speed", scope: "session", view: "speed" },
  { id: "dht", label: "DHT", scope: "session", view: null },
];

export function desiredDetailForTab(
  tab: DetailTab,
  torrentId: string | null,
): DesiredInspectionViews["detail"] {
  const definition = DETAIL_TABS.find((candidate) => candidate.id === tab);
  if (definition === undefined || definition.view === null) return null;
  return definition.scope === "session" || torrentId !== null
    ? definition.view
    : null;
}
