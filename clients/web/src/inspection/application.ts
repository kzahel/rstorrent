import type {
  CommandResult,
  DemoScenarioSummary,
  InspectionCommand,
  InspectionUpdate,
  DesiredInspectionViews,
} from "./model";

export interface InspectionApplication {
  readonly kind: "demo" | "live";
  readonly scenarios: readonly DemoScenarioSummary[];

  subscribe(listener: (update: InspectionUpdate) => void): () => void;
  setViews(views: DesiredInspectionViews): Promise<void>;
  dispatch(command: InspectionCommand): Promise<CommandResult>;
  close(): Promise<void>;
}
