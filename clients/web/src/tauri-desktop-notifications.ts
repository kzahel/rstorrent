import { invoke } from "@tauri-apps/api/core";

import type {
  DesktopNotifications,
  DesktopNotificationSettings,
} from "./inspection/desktop-notifications/types";

export async function createTauriDesktopNotifications(): Promise<DesktopNotifications> {
  let snapshot = await invoke<DesktopNotificationSettings>(
    "desktop_notification_settings",
  );
  return {
    getSnapshot: () => snapshot,
    async save(settings) {
      snapshot = await invoke<DesktopNotificationSettings>(
        "desktop_set_notification_settings",
        { settings },
      );
      return snapshot;
    },
  };
}
