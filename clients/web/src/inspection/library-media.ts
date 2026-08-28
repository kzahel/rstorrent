import type { FileRow, MediaRow } from "./model";

export function sortMediaRows(rows: readonly MediaRow[]): MediaRow[] {
  return [...rows].sort((left, right) => {
    if (left.role.type === "episode" && right.role.type !== "episode") return -1;
    if (left.role.type !== "episode" && right.role.type === "episode") return 1;
    if (left.role.type === "episode" && right.role.type === "episode") {
      return (
        compareNatural(left.role.seriesTitleHint, right.role.seriesTitleHint) ||
        left.role.seasonNumber - right.role.seasonNumber ||
        left.role.episodeNumber - right.role.episodeNumber ||
        (left.role.endingEpisodeNumber ?? left.role.episodeNumber) -
          (right.role.endingEpisodeNumber ?? right.role.episodeNumber) ||
        compareNatural(left.path.join("/"), right.path.join("/")) ||
        left.fileIndex - right.fileIndex
      );
    }
    return (
      compareNatural(left.path.join("/"), right.path.join("/")) ||
      left.fileIndex - right.fileIndex
    );
  });
}

export function sortFileRows(rows: readonly FileRow[]): FileRow[] {
  return [...rows].sort(
    (left, right) =>
      compareNatural(left.path.join("/"), right.path.join("/")) ||
      left.index - right.index,
  );
}

export function episodeLabel(row: MediaRow): string | null {
  if (row.role.type !== "episode") return null;
  const season = String(row.role.seasonNumber).padStart(2, "0");
  const episode = String(row.role.episodeNumber).padStart(2, "0");
  const ending = row.role.endingEpisodeNumber;
  return ending === null
    ? `S${season} · E${episode}`
    : `S${season} · E${episode}–${String(ending).padStart(2, "0")}`;
}

function compareNatural(left: string, right: string): number {
  const leftTokens = tokens(left);
  const rightTokens = tokens(right);
  const count = Math.max(leftTokens.length, rightTokens.length);
  for (let index = 0; index < count; index += 1) {
    const leftToken = leftTokens[index];
    const rightToken = rightTokens[index];
    if (leftToken === undefined) return -1;
    if (rightToken === undefined) return 1;
    if (leftToken === rightToken) continue;
    const leftNumber = /^\d+$/.test(leftToken) ? BigInt(leftToken) : null;
    const rightNumber = /^\d+$/.test(rightToken) ? BigInt(rightToken) : null;
    if (leftNumber !== null && rightNumber !== null) {
      if (leftNumber !== rightNumber) return leftNumber < rightNumber ? -1 : 1;
      continue;
    }
    const leftFolded = leftToken.toLowerCase();
    const rightFolded = rightToken.toLowerCase();
    if (leftFolded !== rightFolded) return leftFolded < rightFolded ? -1 : 1;
    continue;
  }
  return 0;
}

function tokens(value: string): string[] {
  return value.match(/\d+|\D+/g) ?? [];
}
