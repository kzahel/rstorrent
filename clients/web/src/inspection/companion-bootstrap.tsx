import { createRoot } from "react-dom/client";

import type { ApplicationViewClient } from "../api/client";
import { App } from "./components/App";
import { InspectionProvider } from "./context";
import { InspectionController } from "./controller";
import { LiveApplication } from "./live/LiveApplication";
import "./global.css";

export async function startCompanionInspection(
  client: ApplicationViewClient,
): Promise<void> {
  const application = await LiveApplication.open(client, {
    storagePolicy: "one_current_root",
  });
  application.installBrowserWakeHints(window, document);
  const controller = new InspectionController(application);
  controller.start();
  const rootElement = document.querySelector<HTMLElement>("#app");
  if (rootElement === null) throw new Error("missing application root");
  createRoot(rootElement).render(
    <InspectionProvider controller={controller}>
      <App oneCurrentRoot />
    </InspectionProvider>,
  );
}
