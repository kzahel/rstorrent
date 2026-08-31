import { connectAndroidCompanion } from "./android-companion-client";
import { startCompanionInspection } from "./inspection/companion-bootstrap";
import { localizeDocumentShell, message } from "./localization/runtime";

localizeDocumentShell();

const abort = new AbortController();
const status = requiredElement("companion-status");
const cancel = requiredButton("companion-cancel");
let cancelAction = () => {
  abort.abort(new Error("Connection canceled"));
  cancel.disabled = true;
  status.textContent = message("shell.companion.connection-canceled");
};

cancel.addEventListener("click", () => {
  cancelAction();
});

void connectAndroidCompanion(
  (message) => {
    status.textContent = message;
  },
  abort.signal,
)
  .then(async ({ client, hello, endpoint, disconnected }) => {
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
    const closeInspection = await startCompanionInspection(client);
    void disconnected.then(async () => {
      await closeInspection().catch(() => {});
      requiredElement("app").hidden = true;
      identity.hidden = true;
      requiredElement("companion-bootstrap").hidden = false;
      status.setAttribute("role", "alert");
      status.textContent = message("shell.companion.disconnected");
      cancel.textContent = message("common.action.retry");
      cancel.disabled = false;
      cancelAction = () => window.location.reload();
    });
  })
  .catch((error: unknown) => {
    if (abort.signal.aborted) return;
    status.setAttribute("role", "alert");
    status.textContent = error instanceof Error ? error.message : String(error);
    cancel.textContent = message("common.action.close");
    cancel.disabled = false;
    cancelAction = () => window.close();
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
