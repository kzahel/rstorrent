// @vitest-environment node

import { describe, expect, it } from "vitest";

import {
  LAN_NONE_NOTICE_STORAGE_KEY,
  loadLanNoneNoticeDismissed,
  saveLanNoneNoticeDismissed,
} from "./lan-none-notice";

describe("credential-free LAN notice preference", () => {
  it("accepts only the exact versioned dismissal value", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    };

    expect(loadLanNoneNoticeDismissed(storage)).toBe(false);
    values.set(LAN_NONE_NOTICE_STORAGE_KEY, "yes");
    expect(loadLanNoneNoticeDismissed(storage)).toBe(false);
    saveLanNoneNoticeDismissed(storage);
    expect(values.get(LAN_NONE_NOTICE_STORAGE_KEY)).toBe("true");
    expect(loadLanNoneNoticeDismissed(storage)).toBe(true);
  });

  it("fails open to a visible notice when browser storage is unavailable", () => {
    expect(loadLanNoneNoticeDismissed(null)).toBe(false);
    expect(
      loadLanNoneNoticeDismissed({
        getItem: () => {
          throw new Error("storage unavailable");
        },
      }),
    ).toBe(false);
    expect(() =>
      saveLanNoneNoticeDismissed({
        setItem: () => {
          throw new Error("storage unavailable");
        },
      }),
    ).not.toThrow();
  });
});
