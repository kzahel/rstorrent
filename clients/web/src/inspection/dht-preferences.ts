import type { AppearanceStorage } from "./appearance";

export type DhtVisualizationMode = "normalized" | "literal";

const STORAGE_KEY = "rstorrent.presentation.dht";
const VERSION = 1;

export function loadDhtVisualizationMode(
  storage: AppearanceStorage | null = browserStorage(),
): DhtVisualizationMode {
  if (storage === null) return "normalized";
  try {
    const parsed = JSON.parse(storage.getItem(STORAGE_KEY) ?? "null") as {
      readonly version?: unknown;
      readonly mode?: unknown;
    } | null;
    return parsed?.version === VERSION && parsed.mode === "literal"
      ? "literal"
      : "normalized";
  } catch {
    return "normalized";
  }
}

export function saveDhtVisualizationMode(
  mode: DhtVisualizationMode,
  storage: AppearanceStorage | null = browserStorage(),
): void {
  if (storage === null) return;
  try {
    storage.setItem(STORAGE_KEY, JSON.stringify({ version: VERSION, mode }));
  } catch {
    // Browser-local visualization preferences are optional.
  }
}

function browserStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}
