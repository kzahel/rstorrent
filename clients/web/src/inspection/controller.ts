import type { InspectionApplication } from "./application";
import type { InspectionCommand } from "./model";
import { createInspectionStore, type InspectionStoreApi } from "./state";

export class InspectionController {
  readonly store: InspectionStoreApi;
  readonly application: InspectionApplication;

  private unsubscribe: (() => void) | null = null;
  private closed = false;

  constructor(application: InspectionApplication) {
    this.application = application;
    this.store = createInspectionStore();
  }

  start(): void {
    if (this.closed) throw new Error("inspection controller is closed");
    if (this.unsubscribe !== null) return;
    this.unsubscribe = this.application.subscribe((update) => {
      this.store.getState().applyUpdate(update);
    });
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
    this.unsubscribe?.();
    this.unsubscribe = null;
    await this.application.close();
  }
}
