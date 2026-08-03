import type { InspectionApplication } from "./application";
import type { AppearanceStorage } from "./appearance";
import type {
  CommandResult,
  DesiredInspectionViews,
  InspectionCommand,
} from "./model";
import {
  createInspectionStore,
  type InspectionStore,
  type InspectionStoreApi,
} from "./state";
import { desiredDetailForTab } from "./tabs";

export class InspectionController {
  readonly store: InspectionStoreApi;
  readonly application: InspectionApplication;

  private unsubscribe: (() => void) | null = null;
  private unsubscribeStore: (() => void) | null = null;
  private closed = false;
  private desiredViews: DesiredInspectionViews = {
    library: true,
    torrentId: null,
    detail: null,
    logCapture: null,
    speed: null,
  };
  private desiredVersion = 0;
  private syncedVersion = -1;
  private syncing: Promise<void> | null = null;
  private retryMillis = 100;
  private readonly closeSignal = new AbortController();

  constructor(
    application: InspectionApplication,
    appearanceStorage?: AppearanceStorage | null,
  ) {
    this.application = application;
    this.store = createInspectionStore(appearanceStorage);
  }

  start(): void {
    if (this.closed) throw new Error("inspection controller is closed");
    if (this.unsubscribe !== null) return;
    this.unsubscribeStore = this.store.subscribe((state) => {
      this.queueViews(desiredViewsFor(state));
    });
    this.unsubscribe = this.application.subscribe((update) => {
      this.store.getState().applyUpdate(update);
    });
    this.queueViews(desiredViewsFor(this.store.getState()));
  }

  async dispatch(command: InspectionCommand): Promise<string> {
    return (await this.execute(command)).message;
  }

  async execute(command: InspectionCommand): Promise<CommandResult> {
    if (this.closed) throw new Error("inspection controller is closed");
    const result = await this.application.dispatch(command);
    if (!result.accepted) throw new Error(result.message);
    return result;
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.closeSignal.abort("inspection controller closed");
    this.unsubscribe?.();
    this.unsubscribe = null;
    this.unsubscribeStore?.();
    this.unsubscribeStore = null;
    await this.application.close();
    await this.syncing;
  }

  private queueViews(views: DesiredInspectionViews): void {
    if (!sameViews(this.desiredViews, views)) {
      this.desiredViews = views;
      this.desiredVersion += 1;
    } else if (this.syncedVersion === this.desiredVersion) {
      return;
    }
    if (this.syncing === null) {
      this.syncing = this.syncViews()
        .then(() => {
          this.retryMillis = 100;
        })
        .catch(async () => {
          if (this.closed) return;
          const retryMillis = this.retryMillis;
          this.retryMillis = Math.min(retryMillis * 2, 2_000);
          await delay(retryMillis, this.closeSignal.signal);
        })
        .finally(() => {
          this.syncing = null;
          if (!this.closed && this.syncedVersion !== this.desiredVersion) {
            this.queueViews(this.desiredViews);
          }
        });
    }
  }

  private async syncViews(): Promise<void> {
    while (!this.closed && this.syncedVersion !== this.desiredVersion) {
      const version = this.desiredVersion;
      const views = this.desiredViews;
      await this.application.setViews(views);
      this.syncedVersion = version;
    }
  }
}

function delay(millis: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    const aborted = () => {
      globalThis.clearTimeout(timer);
      resolve();
    };
    const timer = globalThis.setTimeout(() => {
      signal.removeEventListener("abort", aborted);
      resolve();
    }, millis);
    signal.addEventListener("abort", aborted, { once: true });
  });
}

function desiredViewsFor(state: InspectionStore): DesiredInspectionViews {
  const presentation = state.presentation;
  if (presentation.destination !== "workbench") {
    return { library: true, torrentId: null, detail: null, logCapture: null, speed: null };
  }
  const currentTorrentId = presentation.currentTorrentId;
  if (presentation.layout === "phone" && !presentation.detailOpen) {
    return { library: true, torrentId: null, detail: null, logCapture: null, speed: null };
  }
  const torrentId = currentTorrentId;
  const detail = desiredDetailForTab(presentation.activeTab, torrentId);
  return {
    library: presentation.layout !== "phone",
    torrentId,
    detail,
    logCapture:
      detail === "logs"
        ? {
            profile: presentation.logCaptureProfile,
            torrentId: presentation.logCaptureTorrentId,
          }
        : null,
    speed:
      detail === "speed"
        ? {
            range: presentation.speedRange,
            metrics: presentation.speedMetrics,
          }
        : null,
  };
}

function sameViews(
  left: DesiredInspectionViews,
  right: DesiredInspectionViews,
): boolean {
  return (
    left.library === right.library &&
    left.torrentId === right.torrentId &&
    left.detail === right.detail &&
    left.logCapture?.profile === right.logCapture?.profile &&
    left.logCapture?.torrentId === right.logCapture?.torrentId
    && left.speed?.range === right.speed?.range
    && sameSeries(left.speed?.metrics, right.speed?.metrics)
  );
}

function sameSeries(
  left: readonly string[] | undefined,
  right: readonly string[] | undefined,
): boolean {
  return left === right ||
    (left !== undefined && right !== undefined &&
      left.length === right.length && left.every((value, index) => value === right[index]));
}
