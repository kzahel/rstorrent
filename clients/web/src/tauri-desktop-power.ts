import { invoke } from "@tauri-apps/api/core";

import type {
  DesktopPower,
  DesktopPowerSettings,
} from "./inspection/desktop-power/types";

export async function createTauriDesktopPower(): Promise<DesktopPower> {
  let snapshot = await invoke<DesktopPowerSettings>("desktop_power_settings");
  return {
    getSnapshot: () => snapshot,
    async save(settings) {
      snapshot = await invoke<DesktopPowerSettings>(
        "desktop_set_power_settings",
        { settings },
      );
      return snapshot;
    },
  };
}
