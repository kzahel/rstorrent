import { applyStoredColorTheme } from "./inspection/appearance";

const parameters = new URLSearchParams(window.location.search);
const appearance = applyStoredColorTheme();
document
  .querySelector<HTMLMetaElement>('meta[name="color-scheme"]')
  ?.setAttribute(
    "content",
    appearance.colorTheme === "auto"
      ? "light dark"
      : appearance.colorTheme,
  );

if (parameters.has("demo")) {
  void import("./inspection/bootstrap").then(({ startDemoInspection }) => {
    startDemoInspection(parameters);
  });
} else if (parameters.has("live")) {
  void import("./inspection/bootstrap").then(
    ({ startLiveInspection, renderBootstrapError }) => {
      void startLiveInspection(parameters).catch(renderBootstrapError);
    },
  );
} else if ("__TAURI_INTERNALS__" in window) {
  void import("./inspection/bootstrap").then(
    ({ startTauriInspection, renderBootstrapError }) => {
      void startTauriInspection().catch(renderBootstrapError);
    },
  );
} else {
  void import("./inspection/bootstrap").then(({ startDemoInspection }) => {
    startDemoInspection(parameters);
  });
}
