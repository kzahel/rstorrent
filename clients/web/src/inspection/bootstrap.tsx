import { createRoot } from "react-dom/client";

import { App } from "./components/App";
import { InspectionProvider } from "./context";
import { InspectionController } from "./controller";
import { DemoApplication } from "./demo/DemoApplication";
import { isDemoScenarioId } from "./demo/catalog";
import { LiveApplication } from "./live/LiveApplication";
import type { DemoScenarioId } from "./model";
import { HttpApplicationClient } from "../api/client";
import { TauriApplicationViewClient } from "../tauri-view-client";
import { WebSocketApplicationViewClient } from "../websocket-view-client";
import "./global.css";

export function startDemoInspection(parameters: URLSearchParams): void {
  const requested = parameters.get("demo");
  const scenarioId: DemoScenarioId = isDemoScenarioId(requested)
    ? requested
    : "healthy-download";
  const elapsedMs = parseElapsed(parameters.get("at"));
  const running = parameters.get("autoplay") !== "0";
  const application = new DemoApplication({ scenarioId, elapsedMs, running });
  const controller = new InspectionController(application);
  renderInspection(controller);
}

export async function startLiveInspection(
  parameters: URLSearchParams,
): Promise<void> {
  const gateway = parameters.get("live");
  if (gateway === null) throw new Error("live gateway URL is required");
  const baseUrl = new URL(gateway);
  if (baseUrl.protocol !== "http:" || !isLoopbackHost(baseUrl.hostname)) {
    throw new Error("live gateway must use an HTTP loopback address");
  }
  const token = parameters.get("token");
  const transport = parameters.get("transport");
  if (transport !== null && transport !== "http") {
    throw new Error("live transport must be omitted or set to http");
  }
  if (transport !== "http" && parameters.has("poll_ms")) {
    throw new Error("poll_ms requires the diagnostic transport=http mode");
  }
  const waitMillis =
    transport === "http"
      ? parsePollMillis(parameters.get("poll_ms"))
      : undefined;
  const client =
    transport === "http"
      ? new HttpApplicationClient(
          baseUrl.href,
          token,
          window.location.origin,
        )
      : new WebSocketApplicationViewClient(baseUrl.href, token);
  const application = await LiveApplication.open(client, {
    ...(waitMillis === undefined ? {} : { waitMillis }),
  });
  application.installBrowserWakeHints(window, document);
  renderInspection(new InspectionController(application));
}

export async function startTauriInspection(): Promise<void> {
  const application = await LiveApplication.open(
    new TauriApplicationViewClient(),
  );
  application.installBrowserWakeHints(window, document);
  renderInspection(new InspectionController(application));
}

export function renderBootstrapError(error: unknown): void {
  const rootElement = applicationRoot();
  const message = error instanceof Error ? error.message : String(error);
  rootElement.replaceChildren();
  const alert = document.createElement("div");
  alert.setAttribute("role", "alert");
  alert.textContent = `Unable to start live inspection: ${message}`;
  rootElement.append(alert);
}

function renderInspection(controller: InspectionController): void {
  controller.start();
  createRoot(applicationRoot()).render(
    <InspectionProvider controller={controller}>
      <App />
    </InspectionProvider>,
  );
}

function applicationRoot(): HTMLElement {
  const rootElement = document.querySelector<HTMLElement>("#app");
  if (rootElement === null) throw new Error("missing application root");
  return rootElement;
}

function isLoopbackHost(host: string): boolean {
  return host === "127.0.0.1" || host === "[::1]" || host === "::1";
}

function parseElapsed(value: string | null): number {
  if (value === null || value.trim() === "") return 0;
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return 0;
  return Math.max(0, Math.min(86_400_000, Math.trunc(numeric)));
}

function parsePollMillis(value: string | null): number | undefined {
  if (value === null || value.trim() === "") return undefined;
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return undefined;
  return Math.max(50, Math.min(20_000, Math.trunc(numeric)));
}
