import { describe, expect, it, vi } from "vitest";

import {
  APPEARANCE_STORAGE_KEY,
  DEFAULT_INTERFACE_SIZE,
  loadInterfaceSize,
  saveInterfaceSize,
} from "./appearance";

describe("appearance preferences", () => {
  it("defaults to Standard without a stored preference", () => {
    expect(loadInterfaceSize(null)).toBe(DEFAULT_INTERFACE_SIZE);
    expect(loadInterfaceSize({ getItem: () => null })).toBe("standard");
  });

  it("round trips every accepted interface size", () => {
    for (const interfaceSize of ["compact", "standard", "spacious"] as const) {
      let stored: string | null = null;
      const storage = {
        getItem: (key: string) =>
          key === APPEARANCE_STORAGE_KEY ? stored : null,
        setItem: (key: string, value: string) => {
          if (key === APPEARANCE_STORAGE_KEY) stored = value;
        },
      };
      saveInterfaceSize(interfaceSize, storage);
      expect(loadInterfaceSize(storage)).toBe(interfaceSize);
    }
  });

  it("rejects malformed, future, and unknown stored values", () => {
    const stored = (source: string) => ({ getItem: () => source });
    expect(loadInterfaceSize(stored("not json"))).toBe("standard");
    expect(
      loadInterfaceSize(
        stored(JSON.stringify({ version: 2, interfaceSize: "compact" })),
      ),
    ).toBe("standard");
    expect(
      loadInterfaceSize(
        stored(JSON.stringify({ version: 1, interfaceSize: "huge" })),
      ),
    ).toBe("standard");
  });

  it("tolerates denied browser storage", () => {
    const read = vi.fn(() => {
      throw new Error("denied");
    });
    const write = vi.fn(() => {
      throw new Error("denied");
    });
    expect(loadInterfaceSize({ getItem: read })).toBe("standard");
    expect(() => saveInterfaceSize("spacious", { setItem: write })).not.toThrow();
  });
});
