import { createRoot } from "react-dom/client";
import "../../src/inspection/global.css";
import { AboutUpdatesSettingsSection } from "../../src/inspection/components/AboutUpdatesSettingsSection";
import type { DesktopUpdaterSnapshot } from "../../src/inspection/updater/types";

document.documentElement.dataset.interfaceSize = "standard";
document.documentElement.dataset.colorTheme = new URLSearchParams(location.search).get("theme") ?? "light";
const snapshot: DesktopUpdaterSnapshot = {
  info: { version: "0.1.3", buildId: "abcdef1234567890", target: "aarch64-apple-darwin",
    arch: "aarch64", bundleType: "app", checkPrivacy: "preference-controlled" },
  state: { phase: "idle" },
};
const updater = {
  getSnapshot: () => snapshot,
  subscribe: () => () => undefined,
  check: async () => undefined,
  install: async () => undefined,
  dismiss: () => undefined,
  close: () => undefined,
};
createRoot(document.getElementById("support-harness")!).render(
  <main style={{ maxWidth: "42rem", margin: "0 auto", padding: "1rem" }}>
    <h1>About &amp; updates</h1>
    <AboutUpdatesSettingsSection updater={updater} snapshot={snapshot} />
  </main>,
);
