// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApplicationViewError } from "./api/client";
const mock = vi.hoisted(() => ({ connect: vi.fn(), mount: vi.fn(), send: vi.fn() }));
vi.mock("./desktop-companion-client", () => ({
  connectDesktopCompanion: mock.connect,
  desktopRuntime: () => ({ sendMessage: mock.send }),
}));
vi.mock("./inspection/companion-bootstrap", () => ({ startCompanionInspection: mock.mount }));
vi.mock("./localization/runtime", () => ({ message: (key: string) => key }));
import { startDesktopCompanion } from "./desktop-companion-main";

describe("desktop companion connection ownership", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetAllMocks();
    document.body.innerHTML = '<section id="companion-bootstrap"><h1></h1><p id="companion-status"></p><button id="companion-cancel"></button></section><div id="companion-identity"></div><div id="app" hidden></div>';
  });
  afterEach(() => {
    window.dispatchEvent(new Event("pagehide"));
    vi.useRealTimers();
  });
  it("retries attach without sending a start request", async () => {
    mock.connect.mockRejectedValue(new Error("stopped"));
    await startDesktopCompanion();
    await vi.advanceTimersByTimeAsync(31_000);
    expect(mock.connect).toHaveBeenCalledTimes(6);
    expect(mock.send).not.toHaveBeenCalled();
    expect(document.getElementById("app")!.hidden).toBe(true);
  });
  it("closes the connection when the shared UI fails to mount", async () => {
    const close = vi.fn().mockResolvedValue(undefined);
    mock.connect.mockResolvedValue({ client: { close }, disconnected: new Promise(() => {}) });
    mock.mount.mockRejectedValue(new Error("mount failed"));
    await startDesktopCompanion();
    expect(close).toHaveBeenCalledOnce();
    expect(document.getElementById("companion-bootstrap")!.hidden).toBe(false);
  });
  it("stops automatic attempts after an authentication failure", async () => {
    mock.connect.mockRejectedValue(new ApplicationViewError("authentication_failed", "identity changed"));
    await startDesktopCompanion();
    await vi.advanceTimersByTimeAsync(90_000);
    expect(mock.connect).toHaveBeenCalledOnce();
    expect(mock.send).not.toHaveBeenCalled();
  });
  it("unmounts a disconnected view before another attachment", async () => {
    let disconnect!: () => void;
    const close = vi.fn().mockResolvedValue(undefined);
    const unmount = vi.fn().mockResolvedValue(undefined);
    mock.connect.mockResolvedValueOnce({ client: { close }, disconnected: new Promise<void>(resolve => { disconnect = resolve; }) });
    mock.connect.mockRejectedValue(new Error("stopped"));
    mock.mount.mockResolvedValue(unmount);
    const running = startDesktopCompanion();
    await vi.advanceTimersByTimeAsync(0);
    expect(document.getElementById("app")!.hidden).toBe(false);
    disconnect();
    await running;
    expect(unmount).toHaveBeenCalledOnce();
    expect(close).toHaveBeenCalledOnce();
    expect(document.getElementById("app")!.hidden).toBe(true);
    await vi.advanceTimersByTimeAsync(1000);
    expect(mock.connect).toHaveBeenCalledTimes(2);
    expect(mock.send).not.toHaveBeenCalled();
  });
});
