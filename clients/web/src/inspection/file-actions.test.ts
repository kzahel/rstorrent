import { describe, expect, it } from "vitest";

import { FILE_ACTIONS, resolveFileActions } from "./file-actions";

describe("file selection actions", () => {
  it("defines one stable Normal and Skip inventory for every presentation", () => {
    expect(FILE_ACTIONS.map((action) => [action.group, action.id])).toEqual([
      ["priority", "normal"],
      ["priority", "skip"],
    ]);
  });

  it("shares empty, pending, and product availability reasons", () => {
    expect(resolveFileActions(0, false)[0]).toMatchObject({
      disabled: true,
      disabledReason: "Select a file to use these actions.",
    });
    expect(resolveFileActions(2, true)[1]).toMatchObject({
      disabled: true,
      disabledReason: "Another file action is still in progress.",
    });
    expect(
      resolveFileActions(2, false, "Priority is unavailable.")[0],
    ).toMatchObject({
      disabled: true,
      disabledReason: "Priority is unavailable.",
    });
    expect(resolveFileActions(2, false).every((action) => !action.disabled)).toBe(
      true,
    );
  });
});
