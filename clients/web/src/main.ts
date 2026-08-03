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

void startInspection().catch(renderBootstrapError);

async function startInspection(): Promise<void> {
  const { startDemoInspection, startLiveInspection, startTauriInspection } =
    await import("./inspection/bootstrap");
  if (parameters.has("demo")) {
    startDemoInspection(parameters);
  } else if (parameters.has("live")) {
    await startLiveInspection(parameters);
  } else if ("__TAURI_INTERNALS__" in window) {
    await startTauriInspection();
  } else {
    startDemoInspection(parameters);
  }
}

function renderBootstrapError(error: unknown): void {
  const rootElement = document.querySelector<HTMLElement>("#app");
  if (rootElement === null) {
    console.error("Unable to start RSTorrent", error);
    return;
  }
  const message = error instanceof Error ? error.message : String(error);
  rootElement.replaceChildren();
  const alert = document.createElement("div");
  alert.setAttribute("role", "alert");
  alert.textContent = `Unable to start RSTorrent: ${message}`;
  rootElement.append(alert);
}
