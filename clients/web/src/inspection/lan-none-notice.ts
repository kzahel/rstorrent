export const LAN_NONE_NOTICE_STORAGE_KEY =
  "rstorrent.notice.lan-none.v1.dismissed";
export const NETWORK_NONE_NOTICE_STORAGE_KEY =
  "rstorrent.notice.network-none.v1.dismissed";

export type CredentialFreeAccessMode = "lan_none" | "network_none";

const DISMISSED_VALUE = "true";

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;

export function loadCredentialFreeNoticeDismissed(
  accessMode: CredentialFreeAccessMode,
  storage: ReadableStorage | null = browserStorage(),
): boolean {
  if (storage === null) return false;
  try {
    return storage.getItem(storageKey(accessMode)) === DISMISSED_VALUE;
  } catch {
    return false;
  }
}

export function saveCredentialFreeNoticeDismissed(
  accessMode: CredentialFreeAccessMode,
  storage: WritableStorage | null = browserStorage(),
): void {
  if (storage === null) return;
  try {
    storage.setItem(storageKey(accessMode), DISMISSED_VALUE);
  } catch {
    // Dismissal remains in component state for this tab when storage is unavailable.
  }
}

function storageKey(accessMode: CredentialFreeAccessMode): string {
  return accessMode === "network_none"
    ? NETWORK_NONE_NOTICE_STORAGE_KEY
    : LAN_NONE_NOTICE_STORAGE_KEY;
}

function browserStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}
