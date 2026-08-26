import { describe, expect, it } from "vitest";

import { installPolicy } from "./policy";

describe("desktop update package policy", () => {
  it.each(["app", "nsis", "appimage"] as const)(
    "allows Tauri-owned package %s",
    (bundleType) => {
      expect(installPolicy(bundleType).canInstallInApp).toBe(true);
    },
  );

  it.each(["msi", "deb", "rpm", "unknown"] as const)(
    "keeps package %s outside in-app replacement",
    (bundleType) => {
      expect(installPolicy(bundleType).canInstallInApp).toBe(false);
    },
  );

  it("checks headless releases without permitting browser replacement", () => {
    expect(installPolicy("headless")).toEqual({
      canCheck: true,
      canInstallInApp: false,
      packageLabel: "Linux headless service",
    });
  });
});
