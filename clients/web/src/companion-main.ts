import { connectAndroidCompanion } from "./android-companion-client";
import { startCompanionInspection } from "./inspection/companion-bootstrap";

const abort = new AbortController();
const status = requiredElement("companion-status");
const cancel = requiredButton("companion-cancel");

cancel.addEventListener("click", () => {
  abort.abort(new Error("Connection canceled"));
  cancel.disabled = true;
  status.textContent = "Connection canceled. Use the extension menu to try again.";
});

void connectAndroidCompanion(
  (message) => {
    status.textContent = message;
  },
  abort.signal,
)
  .then(async ({ client, hello, endpoint }) => {
    const backend = hello.backend;
    if (backend === undefined || backend === null) {
      throw new Error("Android backend identity is unavailable");
    }
    requiredElement("companion-bootstrap").hidden = true;
    const identity = requiredElement("companion-identity");
    identity.hidden = false;
    identity.textContent =
      `Android · profile ${backend.profile_id} · instance ${backend.instance_id} · ` +
      `RSTorrent ${backend.product_version} · protocol ${hello.api.minimum}–${hello.api.current} · ` +
      endpoint;
    await startCompanionInspection(client);
  })
  .catch((error: unknown) => {
    if (abort.signal.aborted) return;
    status.setAttribute("role", "alert");
    status.textContent = error instanceof Error ? error.message : String(error);
    cancel.textContent = "Close";
    cancel.disabled = false;
    cancel.onclick = () => window.close();
  });

function requiredElement(id: string): HTMLElement {
  const element = document.getElementById(id);
  if (element === null) throw new Error(`missing ${id}`);
  return element;
}

function requiredButton(id: string): HTMLButtonElement {
  const element = document.getElementById(id);
  if (!(element instanceof HTMLButtonElement)) throw new Error(`missing ${id}`);
  return element;
}
