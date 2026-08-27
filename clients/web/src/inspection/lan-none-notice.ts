export const LAN_NONE_NOTICE_STORAGE_KEY =
  "rstorrent.notice.lan-none.v1.dismissed";

const DISMISSED_VALUE = "true";

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;

export function loadLanNoneNoticeDismissed(
  storage: ReadableStorage | null = browserStorage(),
): boolean {
  if (storage === null) return false;
  try {
    return storage.getItem(LAN_NONE_NOTICE_STORAGE_KEY) === DISMISSED_VALUE;
  } catch {
    return false;
  }
}

export function saveLanNoneNoticeDismissed(
  storage: WritableStorage | null = browserStorage(),
): void {
  if (storage === null) return;
  try {
    storage.setItem(LAN_NONE_NOTICE_STORAGE_KEY, DISMISSED_VALUE);
  } catch {
    // Dismissal remains in component state for this tab when storage is unavailable.
  }
}

function browserStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}
