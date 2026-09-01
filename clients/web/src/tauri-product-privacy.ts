import { invoke } from "@tauri-apps/api/core";

import type {
  ProductFeedbackPreview,
  ProductPrivacyController,
  ProductPrivacySnapshot,
  ProductSummary,
} from "./inspection/product-privacy/types";

export async function createTauriProductPrivacy(): Promise<ProductPrivacyController> {
  const summary = await invoke<ProductSummary>("desktop_product_summary");
  return new TauriProductPrivacy(summary);
}

class TauriProductPrivacy implements ProductPrivacyController {
  private snapshot: ProductPrivacySnapshot;
  private readonly listeners = new Set<() => void>();

  constructor(summary: ProductSummary) {
    this.snapshot = { summary, busy: false };
  }

  readonly getSnapshot = (): ProductPrivacySnapshot => this.snapshot;

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  readonly acknowledgeDisclosure = async (enabled: boolean): Promise<void> => {
    await this.update("desktop_product_acknowledge_disclosure", {
      statisticsEnabled: enabled,
    });
  };

  readonly setStatisticsEnabled = async (enabled: boolean): Promise<void> => {
    await this.update("desktop_product_set_statistics_enabled", {
      statisticsEnabled: enabled,
    });
  };

  readonly resetStatistics = async (): Promise<void> => {
    await this.update("desktop_product_reset_statistics");
  };

  readonly feedbackPreview = (includeStatistics: boolean) =>
    invoke<ProductFeedbackPreview>("desktop_product_feedback_preview", {
      includeStatistics,
    });

  readonly openFeedback = (includeStatistics: boolean, expectedUrl: string) =>
    invoke<void>("desktop_product_open_feedback", {
      includeStatistics,
      expectedUrl,
    });

  readonly openPrivacy = () => invoke<void>("desktop_product_open_privacy");

  private async update(
    command: string,
    arguments_: Record<string, unknown> = {},
  ): Promise<void> {
    this.setSnapshot({ summary: this.snapshot.summary, busy: true });
    try {
      const summary = await invoke<ProductSummary>(command, arguments_);
      this.setSnapshot({ summary, busy: false });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      this.setSnapshot({ ...this.snapshot, busy: false, error: message });
      throw error;
    }
  }

  private setSnapshot(snapshot: ProductPrivacySnapshot): void {
    this.snapshot = snapshot;
    for (const listener of this.listeners) listener();
  }
}
