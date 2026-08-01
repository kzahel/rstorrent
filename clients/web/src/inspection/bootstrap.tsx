import { createRoot } from "react-dom/client";

import { App } from "./components/App";
import { InspectionProvider } from "./context";
import { InspectionController } from "./controller";
import { DemoApplication } from "./demo/DemoApplication";
import { isDemoScenarioId } from "./demo/catalog";
import type { DemoScenarioId } from "./model";
import "./global.css";

export function startDemoInspection(parameters: URLSearchParams): void {
  const rootElement = document.querySelector<HTMLElement>("#app");
  if (rootElement === null) throw new Error("missing application root");
  const requested = parameters.get("demo");
  const scenarioId: DemoScenarioId = isDemoScenarioId(requested)
    ? requested
    : "healthy-download";
  const elapsedMs = parseElapsed(parameters.get("at"));
  const running = parameters.get("autoplay") !== "0";
  const application = new DemoApplication({ scenarioId, elapsedMs, running });
  const controller = new InspectionController(application);
  controller.start();
  createRoot(rootElement).render(
    <InspectionProvider controller={controller}>
      <App />
    </InspectionProvider>,
  );
}

function parseElapsed(value: string | null): number {
  if (value === null || value.trim() === "") return 0;
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return 0;
  return Math.max(0, Math.min(86_400_000, Math.trunc(numeric)));
}
