import { describe, it, expect } from "vitest";
import { validateDesktopReady } from "./desktop-companion-client";
const ready = { kind: "ready", endpoint: "http://127.0.0.1:32145", credential: "ab".repeat(32), instanceId: "cd".repeat(16), profileId: "default" };
describe("protected desktop bootstrap response", () => {
  it("accepts the bounded exact runtime", () => { expect(validateDesktopReady(ready)).toEqual(ready); });
  it.each(["http://localhost:1234", "http://192.0.2.1:1234", "http://127.0.0.1:0", "http://127.0.0.1:65536", "http://127.0.0.1:1234/?token=x"])("refuses foreign or malformed endpoint %s", (endpoint) => { expect(() => validateDesktopReady({ ...ready, endpoint })).toThrow(); });
  it("refuses identity or credential drift", () => {
    for (const change of [{ credential: "" }, { profileId: "other" }, { instanceId: "x" }, { kind: "launch" }]) expect(() => validateDesktopReady({ ...ready, ...change })).toThrow();
  });
});
