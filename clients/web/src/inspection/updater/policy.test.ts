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
});
