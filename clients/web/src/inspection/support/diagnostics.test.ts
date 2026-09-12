import { describe, expect, it } from "vitest";
import type { DesktopUpdaterSnapshot } from "../updater/types";
import { buildDiagnostics } from "./diagnostics";

describe("local diagnostics allowlist", () => {
  it("preserves useful build facts and closed failure operation only", () => {
    const source: DesktopUpdaterSnapshot = {
      info: { version: "0.1.3", buildId: "abcdef1234567-dirty", arch: "aarch64",
        target: "aarch64-apple-darwin", bundleType: "app", checkPrivacy: "anonymous" },
      state: { phase: "error", operation: "install", message: "/private/payload?token=secret" },
    };
    expect(JSON.parse(buildDiagnostics(source))).toEqual({ schema: 1, product: "RSTorrent",
      version: "0.1.3", build: "abcdef1234567-dirty", target: "aarch64-apple-darwin", package: "app",
      update_check_privacy: "anonymous", updater_phase: "error", updater_operation: "install" });
  });

  it("rejects hostile and oversized fields without serializing unknown properties", () => {
    const secret = "/Users/private/torrent?token=credential";
    const source = {
      info: { version: secret, buildId: "a".repeat(100000), target: secret,
        arch: secret, bundleType: secret, checkPrivacy: secret, installationId: secret },
      state: { phase: "error", operation: secret, message: secret, version: secret,
        notes: secret, manualApply: { command: secret, releaseUrl: secret } },
      logs: [secret],
    } as unknown as DesktopUpdaterSnapshot;
    const text = buildDiagnostics(source);
    expect(text).not.toContain(secret);
    expect(text).not.toContain("installationId");
    expect(text).not.toContain("logs");
    expect(new TextEncoder().encode(text).length).toBeLessThan(2048);
    expect(JSON.parse(text).build).toBe("unknown");
    expect(JSON.parse(text).updater_operation).toBe("unknown");
  });
});
