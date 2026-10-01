// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AndroidCompanionUnavailable, AndroidCompanionUpdateRequired } from "./android-companion-client";
const mock = vi.hoisted(() => ({ connect: vi.fn(), mount: vi.fn(), permission: vi.fn() }));
vi.mock("./android-companion-client", async (original) => ({
  ...await original<typeof import("./android-companion-client")>(), connectAndroidCompanion: mock.connect,
}));
vi.mock("./inspection/companion-bootstrap", () => ({ startCompanionInspection: mock.mount }));
vi.mock("./localization/runtime", () => ({ message: (key: string) => key }));
import { startAndroidCompanion } from "./android-companion-main";

const button = (id = "companion-cancel") => document.getElementById(id) as HTMLButtonElement;
const status = () => document.getElementById("companion-status")!.textContent;
function connected() {
  let disconnect!: () => void;
  const close = vi.fn().mockResolvedValue(undefined);
  const unmount = vi.fn().mockResolvedValue(undefined);
  mock.mount.mockResolvedValue(unmount);
  mock.connect.mockResolvedValueOnce({ client: { close }, hello: {}, disconnected: new Promise<void>(resolve => { disconnect = resolve; }) });
  return { close, unmount, disconnect: () => disconnect() };
}
describe("Android pre-connection recovery ownership", () => {
  beforeEach(() => {
    vi.useFakeTimers(); vi.resetAllMocks();
    document.body.innerHTML = '<section id="companion-bootstrap"><p id="companion-status"></p><button id="companion-cancel"></button><a id="companion-update-android" hidden></a><button id="companion-preview"></button><textarea id="companion-context" hidden></textarea><p id="companion-context-privacy" hidden></p></section><header id="companion-identity" hidden></header><main id="app" hidden></main>';
    mock.permission.mockResolvedValue(true);
    vi.stubGlobal("chrome", { runtime: { getManifest: () => ({ version: "1.1.2" }) }, permissions: { contains: mock.permission } });
  });
  afterEach(async () => {
    window.dispatchEvent(new PageTransitionEvent("pagehide"));
    await vi.advanceTimersByTimeAsync(0);
    vi.useRealTimers(); vi.unstubAllGlobals();
  });
  it("denied optional permission contacts no service and makes no app/Play claim", async () => {
    mock.permission.mockResolvedValue(false);
    await startAndroidCompanion();
    expect(mock.connect).not.toHaveBeenCalled();
    expect(status()).toBe("android.companion.permission-denied");
    button("companion-preview").click();
    const report = JSON.parse((document.getElementById("companion-context") as HTMLTextAreaElement).value);
    expect(report).toMatchObject({ category: "permission_denied", browser_permission: "denied", play_availability: "unknown", app_presence: "unknown", device_policy: "unknown" });
  });
  it("offline failure stays terminal until manual attach-only retry", async () => {
    mock.connect.mockRejectedValue(new AndroidCompanionUnavailable());
    await startAndroidCompanion();
    await vi.advanceTimersByTimeAsync(300_000);
    expect(mock.connect).toHaveBeenCalledOnce();
    button().click(); button().click();
    await vi.advanceTimersByTimeAsync(0);
    expect(mock.connect).toHaveBeenCalledTimes(2);
  });
  it("cancel joins the pending attempt before allowing retry", async () => {
    mock.connect.mockImplementation((_status, signal: AbortSignal) => new Promise((_, reject) => signal.addEventListener("abort", () => reject(signal.reason), { once: true })));
    const task = startAndroidCompanion();
    await vi.advanceTimersByTimeAsync(0);
    button().click();
    await task;
    expect(status()).toBe("shell.companion.connection-canceled");
    expect(button().disabled).toBe(false);
    await vi.advanceTimersByTimeAsync(300_000);
    expect(mock.connect).toHaveBeenCalledOnce();
  });
  it("caps the whole pairing/authentication attempt and reports timeout", async () => {
    mock.connect.mockImplementation((update, signal: AbortSignal) => {
      update("approve", "pairing");
      return new Promise((_, reject) => signal.addEventListener("abort", () => reject(signal.reason), { once: true }));
    });
    const task = startAndroidCompanion();
    await vi.advanceTimersByTimeAsync(150_000);
    await task;
    expect(status()).toBe("android.companion.attempt-timeout");
    button("companion-preview").click();
    expect((document.getElementById("companion-context") as HTMLTextAreaElement).value).toContain('"category": "timeout"');
  });
  it.each(["android", "extension"] as const)("keeps %s update requirements separate", async (component) => {
    mock.connect.mockRejectedValue(new AndroidCompanionUpdateRequired(component));
    await startAndroidCompanion();
    expect(document.getElementById("companion-update-android")!.hidden).toBe(component !== "android");
    await vi.advanceTimersByTimeAsync(300_000);
    expect(mock.connect).toHaveBeenCalledOnce();
  });
  it("previews closed context without copying raw errors or arbitrary versions", async () => {
    vi.stubGlobal("chrome", { runtime: { getManifest: () => ({ version: "secret/path" }) }, permissions: { contains: mock.permission } });
    mock.connect.mockRejectedValue(new Error("/personal/path token=private account@example.test"));
    await startAndroidCompanion();
    button("companion-preview").click();
    const report = (document.getElementById("companion-context") as HTMLTextAreaElement).value;
    expect(report).not.toMatch(/personal|private|example|secret/u);
    expect(JSON.parse(report)).toMatchObject({ category: "connection_failed", extension_version: "unknown" });
    expect(status()).toBe("android.companion.failed");
  });
  it("labels frozen browser user-agent versions without claiming actual OS builds", async () => {
    vi.stubGlobal("navigator", { userAgent: "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) Chrome/150.0.0.0" });
    mock.permission.mockResolvedValue(false);
    await startAndroidCompanion();
    button("companion-preview").click();
    const report = JSON.parse((document.getElementById("companion-context") as HTMLTextAreaElement).value);
    expect(report).toMatchObject({ chrome_user_agent_version: "150.0.0.0", chromeos_user_agent_version: "14541.0.0",
      chrome_build_version: "unknown", chromeos_build_version: "unknown" });
    expect(report).not.toHaveProperty("os_version");
    expect(report).not.toHaveProperty("chrome_version");
  });
  it("disconnect unmounts and closes before exposing manual recovery", async () => {
    const connection = connected();
    const task = startAndroidCompanion();
    await vi.advanceTimersByTimeAsync(0);
    expect(document.getElementById("companion-identity")!.querySelector("a")!.href).toContain("crostini/setup.html#android");
    expect(document.getElementById("companion-identity")!.textContent).toContain("shell.companion.title");
    connection.disconnect(); await task;
    expect(connection.unmount).toHaveBeenCalledOnce(); expect(connection.close).toHaveBeenCalledOnce();
    expect(document.getElementById("app")!.hidden).toBe(true);
    expect(status()).toBe("shell.companion.disconnected");
    await vi.advanceTimersByTimeAsync(300_000);
    expect(mock.connect).toHaveBeenCalledOnce();
  });
  it("departure during delayed mount joins the view and does not reconnect on restoration", async () => {
    const connection = connected();
    let mounted!: (close: () => Promise<void>) => void;
    mock.mount.mockReturnValue(new Promise(resolve => { mounted = resolve; }));
    const task = startAndroidCompanion(); await vi.advanceTimersByTimeAsync(0);
    window.dispatchEvent(new PageTransitionEvent("pagehide", { persisted: true }));
    window.dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true }));
    mounted(connection.unmount); await task; await vi.advanceTimersByTimeAsync(0);
    expect(connection.unmount).toHaveBeenCalledOnce();
    expect(document.getElementById("app")!.hidden).toBe(true);
    expect(mock.connect).toHaveBeenCalledOnce();
  });
  it("failed UI mount releases its authenticated client", async () => {
    const connection = connected(); mock.mount.mockRejectedValue(new Error("mount failure"));
    await startAndroidCompanion();
    expect(connection.close).toHaveBeenCalledOnce();
    expect(document.getElementById("app")!.hidden).toBe(true);
  });
});
