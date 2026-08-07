import { describe, expect, it } from "vitest";

import { resolveInspectionBootstrapTarget } from "./bootstrap-target";

describe("inspection bootstrap target", () => {
  it("retains the named demo for an ordinary browser build", () => {
    expect(
      resolveInspectionBootstrapTarget(
        new URLSearchParams(),
        false,
        undefined,
      ).type,
    ).toBe("demo");
  });

  it("selects same-origin live mode only for an explicit hosted build", () => {
    const target = resolveInspectionBootstrapTarget(
      new URLSearchParams(),
      false,
      "same-origin",
    );
    expect(target.type).toBe("live");
  });

  it("keeps explicit demo and Tauri selection ahead of the hosted default", () => {
    expect(
      resolveInspectionBootstrapTarget(
        new URLSearchParams("demo=large"),
        true,
        "same-origin",
      ).type,
    ).toBe("demo");
    expect(
      resolveInspectionBootstrapTarget(
        new URLSearchParams(),
        true,
        "same-origin",
      ).type,
    ).toBe("tauri");
  });
});
