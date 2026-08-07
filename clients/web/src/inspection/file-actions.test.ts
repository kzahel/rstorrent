import { describe, expect, it } from "vitest";

import { FILE_ACTIONS, resolveFileActions } from "./file-actions";

describe("file selection actions", () => {
  it("defines one stable Download now, Normal, and Skip inventory", () => {
    expect(FILE_ACTIONS.map((action) => [action.group, action.id])).toEqual([
      ["download", "download_now"],
      ["priority", "normal"],
      ["priority", "skip"],
    ]);
  });

  it("offers Download now only when at least one target is skipped", () => {
    expect(
      resolveFileActions(1, 0, false).some(
        (action) => action.id === "download_now",
      ),
    ).toBe(false);
    expect(resolveFileActions(2, 1, false)[0]).toMatchObject({
      id: "download_now",
      disabled: false,
    });
  });

  it("shares empty, pending, and product availability reasons", () => {
    expect(resolveFileActions(0, 0, false)[0]).toMatchObject({
      disabled: true,
      disabledReason: "Select a file to use these actions.",
    });
    expect(resolveFileActions(2, 1, true)[0]).toMatchObject({
      disabled: true,
      disabledReason: "Another file action is still in progress.",
    });
    expect(
      resolveFileActions(2, 1, false, "File actions are unavailable.")[0],
    ).toMatchObject({
      disabled: true,
      disabledReason: "File actions are unavailable.",
    });
    expect(
      resolveFileActions(2, 1, false).every((action) => !action.disabled),
    ).toBe(true);
  });
});
