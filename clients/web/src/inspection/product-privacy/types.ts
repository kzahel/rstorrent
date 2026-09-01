export const CURRENT_PRODUCT_DISCLOSURE_VERSION = 1;

export interface ProductSummary {
  readonly installationId: string;
  readonly createdAtMillis: string;
  readonly firstVersion: string;
  readonly currentVersion: string;
  readonly disclosureVersion: number;
  readonly statisticsEnabled: boolean;
  readonly torrentsAdded: string;
  readonly downloadsCompleted: string;
  readonly foregroundSessions: string;
  readonly resetGeneration: string;
  readonly lastStartMillis: string;
  readonly lastCleanShutdownMillis: string | null;
  readonly daysSinceFirstUse: number;
  readonly transmissionAllowed: boolean;
}

export interface ProductFeedbackField {
  readonly name: string;
  readonly value: string;
  readonly pseudonymous: boolean;
}

export interface ProductFeedbackPreview {
  readonly destination: string;
  readonly url: string;
  readonly fields: readonly ProductFeedbackField[];
  readonly statisticsAvailable: boolean;
  readonly statisticsIncluded: boolean;
  readonly hostedContextReady: boolean;
}

export interface ProductPrivacySnapshot {
  readonly summary: ProductSummary;
  readonly busy: boolean;
  readonly error?: string;
}

export interface ProductPrivacyController {
  readonly getSnapshot: () => ProductPrivacySnapshot;
  readonly subscribe: (listener: () => void) => () => void;
  readonly acknowledgeDisclosure: (enabled: boolean) => Promise<void>;
  readonly setStatisticsEnabled: (enabled: boolean) => Promise<void>;
  readonly resetStatistics: () => Promise<void>;
  readonly feedbackPreview: (
    includeStatistics: boolean,
  ) => Promise<ProductFeedbackPreview>;
  readonly openFeedback: (
    includeStatistics: boolean,
    expectedUrl: string,
  ) => Promise<void>;
  readonly openPrivacy: () => Promise<void>;
}
