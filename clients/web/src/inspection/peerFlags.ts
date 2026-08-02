import type { PeerFlag } from "./model";

export type PeerFlagGroup =
  | "Connection"
  | "Transfer"
  | "Protocol"
  | "Scheduler / integrity";

export interface PeerFlagDefinition {
  readonly glyph: string;
  readonly label: string;
  readonly description: string;
  readonly group: PeerFlagGroup;
}

export const PEER_FLAG_ORDER = [
  "incoming",
  "encrypted",
  "download_allowed",
  "download_choked",
  "upload_allowed",
  "upload_choked",
  "extension_protocol",
  "metadata_extension",
  "utp",
  "hole_punched",
  "on_parole",
  "optimistic_unchoke",
  "snubbed",
  "upload_only",
  "endgame",
  "seed",
] as const satisfies readonly PeerFlag[];

export const PEER_FLAG_DEFINITIONS: Readonly<
  Record<PeerFlag, PeerFlagDefinition>
> = {
  incoming: {
    glyph: "I",
    label: "Incoming",
    description: "The remote peer initiated this connection.",
    group: "Connection",
  },
  encrypted: {
    glyph: "E",
    label: "Encrypted",
    description: "The peer transport is encrypted.",
    group: "Connection",
  },
  download_allowed: {
    glyph: "D",
    label: "Download allowed",
    description: "We are interested and the peer is not choking us.",
    group: "Transfer",
  },
  download_choked: {
    glyph: "d",
    label: "Download choked",
    description: "We are interested but the peer is choking us.",
    group: "Transfer",
  },
  upload_allowed: {
    glyph: "U",
    label: "Upload allowed",
    description: "The peer is interested and we are not choking it.",
    group: "Transfer",
  },
  upload_choked: {
    glyph: "u",
    label: "Upload choked",
    description: "The peer is interested but we are choking it.",
    group: "Transfer",
  },
  extension_protocol: {
    glyph: "x",
    label: "Extension protocol",
    description: "The peer supports the BitTorrent extension protocol.",
    group: "Protocol",
  },
  metadata_extension: {
    glyph: "m",
    label: "Metadata extension",
    description: "The peer supports metadata exchange over ut_metadata.",
    group: "Protocol",
  },
  utp: {
    glyph: "T",
    label: "uTP",
    description: "This connection uses the uTP transport.",
    group: "Connection",
  },
  hole_punched: {
    glyph: "h",
    label: "Hole punched",
    description: "This connection was established by hole punching.",
    group: "Connection",
  },
  on_parole: {
    glyph: "p",
    label: "On parole",
    description: "Requests are restricted after data from this peer failed a hash check.",
    group: "Scheduler / integrity",
  },
  optimistic_unchoke: {
    glyph: "O",
    label: "Optimistic unchoke",
    description: "This peer currently has the optimistic unchoke slot.",
    group: "Scheduler / integrity",
  },
  snubbed: {
    glyph: "S",
    label: "Snubbed",
    description: "The peer has not delivered requested data within the snub timeout.",
    group: "Scheduler / integrity",
  },
  upload_only: {
    glyph: "L",
    label: "Upload only",
    description: "The peer reports that it is upload-only.",
    group: "Transfer",
  },
  endgame: {
    glyph: "e",
    label: "Endgame",
    description: "This connection has outstanding requests in endgame mode.",
    group: "Scheduler / integrity",
  },
  seed: {
    glyph: "s",
    label: "Seed",
    description: "The peer has every piece in the torrent.",
    group: "Transfer",
  },
};

export function normalizedPeerFlags(flags: readonly PeerFlag[]): PeerFlag[] {
  const present = new Set(flags);
  return PEER_FLAG_ORDER.filter((flag) => present.has(flag));
}

export function formatPeerFlags(flags: readonly PeerFlag[]): string {
  return normalizedPeerFlags(flags)
    .map((flag) => PEER_FLAG_DEFINITIONS[flag].glyph)
    .join("");
}

export function describePeerFlags(flags: readonly PeerFlag[]): string {
  const labels = normalizedPeerFlags(flags).map(
    (flag) => PEER_FLAG_DEFINITIONS[flag].label,
  );
  return labels.length === 0
    ? "No active peer flags"
    : `Peer flags: ${labels.join(", ")}`;
}
