import { describe, expect, it } from "vitest";

import { resolveInspectionBootstrapTarget } from "./bootstrap-target";

describe("inspection bootstrap target", () => {
  it("retains the named demo for an ordinary browser build", () => {
    expect(
      resolveInspectionBootstrapTarget(
        new URLSearchParams(),
        false,
        undefined,
        "https://preview.example",
      ).type,
    ).toBe("demo");
  });

  it("selects same-origin live mode only for an explicit hosted build", () => {
    const target = resolveInspectionBootstrapTarget(
      new URLSearchParams(),
      false,
      "same-origin",
      "https://preview.example",
    );
    expect(target.type).toBe("live");
    if (target.type !== "live") throw new Error("expected live target");
    expect(target.parameters.get("live")).toBe("https://preview.example");
  });

  it("keeps explicit demo, live, and Tauri selection ahead of the hosted default", () => {
    expect(
      resolveInspectionBootstrapTarget(
        new URLSearchParams("demo=large"),
        true,
        "same-origin",
        "https://preview.example",
      ).type,
    ).toBe("demo");
    expect(
      resolveInspectionBootstrapTarget(
        new URLSearchParams("live=http%3A%2F%2F127.0.0.1%3A3030"),
        true,
        "same-origin",
        "https://preview.example",
      ).type,
    ).toBe("live");
    expect(
      resolveInspectionBootstrapTarget(
        new URLSearchParams(),
        true,
        "same-origin",
        "https://preview.example",
      ).type,
    ).toBe("tauri");
  });
});
