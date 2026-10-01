// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { AndroidCompanionUnavailable, AndroidCompanionUpdateRequired, connectAndroidCompanion, isLegacyAndroidStatus } from "./android-companion-client";

const origin = "chrome-extension://gcgoepclopkgijmclmlheafaglmbjlcc";
const legacy = { port: 7800, protocolVersion: 1, behaviorVersion: 1, version: "1.0.24", paired: true,
  capabilities: { ioWebSocket: true, controlEvents: true, rootsRead: true, fileOps: true } };
const current = { product: "rstorrent", backend: "android", protocol_min: 1, protocol_max: 1,
  nonce: "abcdefghijklmnop", paired: false, port: 3030 };
const json = (value: unknown) => new Response(JSON.stringify(value));
function setup(handler: (url: URL, init: RequestInit) => Promise<Response>) {
  const set = vi.fn();
  vi.stubGlobal("chrome", { storage: { local: { get: async () => ({}), set } } });
  vi.stubGlobal("location", { origin });
  const fetch = vi.fn(async (input: URL, init: RequestInit) => {
    expect(init.redirect).toBe("error");
    expect(init.credentials).toBe("omit");
    expect(new Headers(init.headers).get("Origin")).toBe(origin);
    expect(input.hostname).toBe("100.115.92.2");
    return handler(input, init);
  });
  vi.stubGlobal("fetch", fetch);
  return { fetch, set };
}
afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); vi.useRealTimers(); });

describe("ChromeOS staggered upgrades", () => {
  it("ends unavailable discovery after 20 seconds without implying app or Play status", async () => {
    vi.useFakeTimers();
    vi.spyOn(AbortSignal, "timeout").mockImplementation((milliseconds) => {
      const abort = new AbortController();
      setTimeout(() => abort.abort(new DOMException("timed out", "TimeoutError")), milliseconds);
      return abort.signal;
    });
    const { fetch, set } = setup(async () => { throw new TypeError("offline"); });
    const pending = expect(connectAndroidCompanion(() => {})).rejects.toBeInstanceOf(AndroidCompanionUnavailable);
    await vi.advanceTimersByTimeAsync(20_000);
    await pending;
    const requests = fetch.mock.calls.length;
    await vi.advanceTimersByTimeAsync(90_000);
    expect(fetch).toHaveBeenCalledTimes(requests);
    expect(set).not.toHaveBeenCalled();
  });
  it("recognizes the released legacy status without accepting generic or malformed responses", () => {
    expect(isLegacyAndroidStatus(legacy, 7800)).toBe(true);
    for (const value of [null, "ok", {}, { ...legacy, port: 3030 }, { ...legacy, protocolVersion: "1" },
      { ...legacy, capabilities: {} }, { ...legacy, capabilities: null }]) {
      expect(isLegacyAndroidStatus(value, 7800)).toBe(false);
    }
    expect(isLegacyAndroidStatus(legacy, 3030)).toBe(false);
  });
  it("new extension/old app terminates with an update requirement and sends no old authority", async () => {
    const { fetch, set } = setup(async (url, init) => {
      if (url.port === "7800") {
        expect(url.pathname).toBe("/status"); expect(init.method).toBe("POST"); expect(init.body).toBe("{}");
        expect(new Headers(init.headers).has("Authorization")).toBe(false);
        return json(legacy);
      }
      throw new TypeError("unavailable");
    });
    await expect(connectAndroidCompanion(() => {})).rejects.toMatchObject({ component: "android", message: expect.stringMatching(/Update the JSTorrent Android app/u) });
    expect(set).not.toHaveBeenCalled();
    expect(fetch.mock.calls.every(([url]) => ["/status", "/rstorrent/companion/v1/hello"].includes(url.pathname))).toBe(true);
  });
  it("prefers a compatible current app and never probes or pairs with legacy", async () => {
    const marker = new Error("reached current pairing");
    const { fetch } = setup(async (url) => {
      if (url.port !== "3030") throw new TypeError("unavailable");
      if (url.pathname.endsWith("/hello")) return json(current);
      if (url.pathname.endsWith("/pairing/request")) throw marker;
      throw new Error("unexpected route");
    });
    await expect(connectAndroidCompanion(() => {})).rejects.toBe(marker);
    expect(fetch.mock.calls.some(([url]) => url.pathname === "/status")).toBe(false);
  });
  it("incompatible successor protocol requires an extension update before pairing", async () => {
    const { fetch } = setup(async (url) => url.port === "3030" ? json({ ...current, protocol_min: 2, protocol_max: 2 }) : Promise.reject(new TypeError("unavailable")));
    await expect(connectAndroidCompanion(() => {})).rejects.toBeInstanceOf(AndroidCompanionUpdateRequired);
    expect(fetch.mock.calls.every(([url]) => url.pathname.endsWith("/hello"))).toBe(true);
  });
  it("offline, malformed and oversized services remain cancellable without creating authority", async () => {
    const abort = new AbortController();
    let probes = 0;
    const { set } = setup(async (url) => {
      probes += 1;
      if (probes === 10) abort.abort(new Error("owned test canceled"));
      if (url.port === "3030") return json({ ...current, protocol_min: "1" });
      if (url.port === "7800") return new Response("x".repeat(65537));
      return json({ unrelated: true });
    });
    await expect(connectAndroidCompanion(() => {}, abort.signal)).rejects.toThrow("owned test canceled");
    expect(set).not.toHaveBeenCalled();
  });
  it("cancels an oversized response stream before accepting a service", async () => {
    const canceled = vi.fn();
    const abort = new AbortController();
    let legacyProbes = 0;
    setup(async (url) => {
      if (url.port === "3030") {
        return new Response(new ReadableStream<Uint8Array>({
          start(controller) { controller.enqueue(new Uint8Array(65537)); },
          cancel: canceled,
        }));
      }
      if (url.pathname === "/status" && ++legacyProbes === 5) abort.abort(new Error("stop after bounded probe"));
      throw new TypeError("unavailable");
    });
    await expect(connectAndroidCompanion(() => {}, abort.signal)).rejects.toThrow("stop after bounded probe");
    expect(canceled).toHaveBeenCalledOnce();
  });
});
