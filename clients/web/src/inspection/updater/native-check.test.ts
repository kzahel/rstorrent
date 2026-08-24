import { describe, expect, it, vi } from "vitest";

import { createNativeUpdateCheckHandler } from "./native-check";

describe("native desktop update checks", () => {
  it("accepts each positive generation once and rejects malformed replay", () => {
    const check = vi.fn();
    const handle = createNativeUpdateCheckHandler(check);

    for (const invalid of [undefined, null, "1", 0, -1, 1.5, Number.MAX_VALUE]) {
      expect(handle(invalid)).toBe(false);
    }
    expect(handle(1)).toBe(true);
    expect(handle(1)).toBe(false);
    expect(handle(3)).toBe(true);
    expect(handle(2)).toBe(false);
    expect(check).toHaveBeenCalledTimes(2);
  });
});
