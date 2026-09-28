import { ApplicationViewError } from "./api/client";
import { message } from "./localization/runtime";
import { connectDesktopCompanion, desktopRuntime } from "./desktop-companion-client";
import { startCompanionInspection } from "./inspection/companion-bootstrap";

export async function startDesktopCompanion(): Promise<void> {
  document.title = "RSTorrent";
  const bootstrap = document.getElementById("companion-bootstrap")!;
  bootstrap.querySelector("h1")!.textContent = message("desktop.companion.title");
  const status = document.getElementById("companion-status")!;
  const start = document.getElementById("companion-cancel") as HTMLButtonElement;
  const identity = document.getElementById("companion-identity")!;
  const app = document.getElementById("app")!;
  const abort = new AbortController();
  let closeInspection: (() => Promise<void>) | undefined;
  let attempt = 0;
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const openNative = document.createElement("button");
  openNative.textContent = message("desktop.companion.open-window");
  openNative.onclick = () => { void desktopRuntime().sendMessage({ type: "nativeBootstrap", op: "launch" }); };
  identity.replaceChildren(openNative);
  identity.hidden = false;
  start.textContent = message("desktop.companion.start");
  start.onclick = async () => {
    start.disabled = true;
    if (timer) clearTimeout(timer);
    const response = await desktopRuntime().sendMessage({ type: "nativeBootstrap", op: "start_control" });
    start.disabled = false;
    if (!response.ok) { status.textContent = response.error?.message ?? message("desktop.companion.start-failed"); return; }
    attempt = 0;
    await connect();
  };
  async function connect(): Promise<void> {
    const current = ++generation;
    status.textContent = message("desktop.companion.connecting");
    try {
      const connection = await connectDesktopCompanion(abort.signal);
      if (current !== generation || abort.signal.aborted) { await connection.client.close(); return; }
      bootstrap.hidden = true;
      app.hidden = false;
      closeInspection = await startCompanionInspection(connection.client, false);
      attempt = 0;
      await connection.disconnected;
      await closeInspection();
      closeInspection = undefined;
      app.hidden = true;
      bootstrap.hidden = false;
      status.textContent = message("desktop.companion.disconnected");
    } catch (error) {
      if (abort.signal.aborted) return;
      status.textContent = error instanceof Error ? error.message : message("desktop.companion.unavailable");
      if ((error instanceof ApplicationViewError && ["authentication_failed", "invalid_version"].includes(error.code)) || /incompatible|identity/i.test(status.textContent)) return;
    }
    if (!abort.signal.aborted && current === generation) {
      timer = setTimeout(() => { void connect(); }, Math.min(30_000, 1000 * 2 ** Math.min(attempt++, 5)));
    }
  }
  window.addEventListener("pagehide", () => {
    abort.abort();
    if (timer) clearTimeout(timer);
    void closeInspection?.();
  }, { once: true });
  await connect();
}
