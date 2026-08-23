import { getBundleType } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  check as checkForTauriUpdate,
  type DownloadEvent,
  type Update,
} from "@tauri-apps/plugin-updater";

import { DesktopUpdaterController } from "./inspection/updater/controller";
import type {
  CheckReason,
  DesktopBundleType,
  DesktopReleaseInfo,
  DesktopUpdateBackend,
  DesktopUpdater,
  UpdateCandidate,
  UpdateDownloadEvent,
} from "./inspection/updater/types";

interface NativeDesktopReleaseInfo {
  readonly version: string;
  readonly buildId: string;
  readonly target: string;
  readonly arch: string;
}

export async function createTauriDesktopUpdater(): Promise<DesktopUpdater> {
  const [nativeInfo, bundleType] = await Promise.all([
    invoke<NativeDesktopReleaseInfo>("desktop_release_info"),
    getBundleType().catch(() => null),
  ]);
  const info: DesktopReleaseInfo = {
    ...nativeInfo,
    bundleType: normalizeBundleType(bundleType),
  };
  const backend: DesktopUpdateBackend = {
    async check(reason, timeoutMs) {
      const update = await checkForTauriUpdate({
        headers: { "X-Check-Reason": reason },
        timeout: timeoutMs,
      });
      return update === null ? null : new TauriUpdateCandidate(update);
    },
    relaunch,
  };
  return new DesktopUpdaterController(backend, info);
}

class TauriUpdateCandidate implements UpdateCandidate {
  readonly version: string;
  readonly notes?: string;

  constructor(private readonly update: Update) {
    this.version = update.version;
    if (update.body !== undefined) this.notes = update.body;
  }

  async downloadAndInstall(
    onEvent: (event: UpdateDownloadEvent) => void,
  ): Promise<void> {
    await this.update.downloadAndInstall((event) => onEvent(mapEvent(event)));
  }

  async close(): Promise<void> {
    await this.update.close();
  }
}

function mapEvent(event: DownloadEvent): UpdateDownloadEvent {
  switch (event.event) {
    case "Started":
      return {
        type: "started",
        ...(event.data.contentLength === undefined
          ? {}
          : { contentLength: event.data.contentLength }),
      };
    case "Progress":
      return { type: "progress", chunkLength: event.data.chunkLength };
    case "Finished":
      return { type: "finished" };
  }
}

function normalizeBundleType(value: string | null): DesktopBundleType {
  switch (value) {
    case "app":
    case "nsis":
    case "appimage":
    case "msi":
    case "deb":
    case "rpm":
      return value;
    default:
      return "unknown";
  }
}

export type { CheckReason };
