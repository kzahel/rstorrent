import type { InspectionApplication } from "./application";
import type {
  DesiredInspectionViews,
  InspectionCommand,
} from "./model";
import {
  createInspectionStore,
  type InspectionStore,
  type InspectionStoreApi,
} from "./state";

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
  };
  private desiredVersion = 0;
  private syncedVersion = -1;
  private syncing: Promise<void> | null = null;
  private retryMillis = 100;
  private readonly closeSignal = new AbortController();

  constructor(application: InspectionApplication) {
    this.application = application;
    this.store = createInspectionStore();
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
    if (this.closed) throw new Error("inspection controller is closed");
    const result = await this.application.dispatch(command);
    if (!result.accepted) throw new Error(result.message);
    return result.message;
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
  const selected = presentation.selectedTorrentId;
  if (presentation.layout === "phone" && !presentation.detailOpen) {
    return { library: true, torrentId: null, detail: null };
  }
  const torrentId = selected;
  const detail =
    presentation.activeTab === "disk"
      ? "disk"
      : torrentId === null
      ? null
      : presentation.activeTab === "peers" ||
          presentation.activeTab === "trackers" ||
          presentation.activeTab === "files" ||
          presentation.activeTab === "pieces" ||
          presentation.activeTab === "logs" ||
          presentation.activeTab === "general"
        ? presentation.activeTab
        : null;
  return {
    library: presentation.layout !== "phone",
    torrentId,
    detail,
  };
}

function sameViews(
  left: DesiredInspectionViews,
  right: DesiredInspectionViews,
): boolean {
  return (
    left.library === right.library &&
    left.torrentId === right.torrentId &&
    left.detail === right.detail
  );
}
