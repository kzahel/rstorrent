import { useEffect, useSyncExternalStore } from "react";

import type { DesktopUpdater, DesktopUpdaterSnapshot } from "./types";

const NO_UPDATER: DesktopUpdaterSnapshot | undefined = undefined;
const NOOP_SUBSCRIBE = () => () => undefined;
const NO_UPDATER_SNAPSHOT = () => NO_UPDATER;

export function useDesktopUpdater(
  updater: DesktopUpdater | undefined,
): DesktopUpdaterSnapshot | undefined {
  const snapshot = useSyncExternalStore(
    updater?.subscribe ?? NOOP_SUBSCRIBE,
    updater?.getSnapshot ?? NO_UPDATER_SNAPSHOT,
    updater?.getSnapshot ?? NO_UPDATER_SNAPSHOT,
  );
  useEffect(() => () => updater?.close(), [updater]);
  return snapshot;
}
