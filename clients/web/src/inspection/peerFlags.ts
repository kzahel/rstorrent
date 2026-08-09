import type { PeerFlag } from "./model";

export type PeerFlagGroup =
  | "Connection"
  | "Transfer"
  | "Protocol"
  | "Scheduler / integrity";

export interface PeerFlagDefinition {
  readonly glyph: string;
  readonly label: string;
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
    group: "Connection",
  },
  encrypted: {
    glyph: "E",
    label: "Encrypted or obfuscated",
    group: "Connection",
  },
  download_allowed: {
    glyph: "D",
    label: "Download allowed",
    group: "Transfer",
  },
  download_choked: {
    glyph: "d",
    label: "Download choked",
    group: "Transfer",
  },
  upload_allowed: {
    glyph: "U",
    label: "Upload allowed",
    group: "Transfer",
  },
  upload_choked: {
    glyph: "u",
    label: "Upload choked",
    group: "Transfer",
  },
  extension_protocol: {
    glyph: "x",
    label: "Extension protocol",
    group: "Protocol",
  },
  metadata_extension: {
    glyph: "m",
    label: "Metadata extension",
    group: "Protocol",
  },
  utp: {
    glyph: "T",
    label: "uTP",
    group: "Connection",
  },
  hole_punched: {
    glyph: "h",
    label: "Hole punched",
    group: "Connection",
  },
  on_parole: {
    glyph: "p",
    label: "On parole",
    group: "Scheduler / integrity",
  },
  optimistic_unchoke: {
    glyph: "O",
    label: "Optimistic unchoke",
    group: "Scheduler / integrity",
  },
  snubbed: {
    glyph: "S",
    label: "Snubbed",
    group: "Scheduler / integrity",
  },
  upload_only: {
    glyph: "L",
    label: "Upload only",
    group: "Transfer",
  },
  endgame: {
    glyph: "e",
    label: "Endgame",
    group: "Scheduler / integrity",
  },
  seed: {
    glyph: "s",
    label: "Seed",
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
