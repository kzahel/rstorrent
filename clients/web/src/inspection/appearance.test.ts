import { describe, expect, it, vi } from "vitest";

import {
  APPEARANCE_STORAGE_KEY,
  DEFAULT_COLOR_THEME,
  DEFAULT_INTERFACE_SIZE,
  applyColorTheme,
  applyStoredColorTheme,
  loadAppearancePreferences,
  saveAppearancePreferences,
} from "./appearance";

describe("appearance preferences", () => {
  it("defaults to Standard and Auto without a stored preference", () => {
    expect(loadAppearancePreferences(null)).toEqual({
      interfaceSize: DEFAULT_INTERFACE_SIZE,
      colorTheme: DEFAULT_COLOR_THEME,
    });
    expect(loadAppearancePreferences({ getItem: () => null })).toEqual({
      interfaceSize: "standard",
      colorTheme: "auto",
    });
  });

  it("migrates valid version-1 interface sizes to Auto", () => {
    for (const interfaceSize of ["compact", "standard", "spacious"] as const) {
      expect(
        loadAppearancePreferences(
          stored(JSON.stringify({ version: 1, interfaceSize })),
        ),
      ).toEqual({ interfaceSize, colorTheme: "auto" });
    }
  });

  it("round trips every accepted interface size and color theme", () => {
    for (const interfaceSize of ["compact", "standard", "spacious"] as const) {
      for (const colorTheme of ["auto", "light", "dark"] as const) {
        let source: string | null = null;
        const storage = {
          getItem: (key: string) =>
            key === APPEARANCE_STORAGE_KEY ? source : null,
          setItem: (key: string, value: string) => {
            if (key === APPEARANCE_STORAGE_KEY) source = value;
          },
        };
        saveAppearancePreferences({ interfaceSize, colorTheme }, storage);
        expect(loadAppearancePreferences(storage)).toEqual({
          interfaceSize,
          colorTheme,
        });
        expect(JSON.parse(source ?? "null")).toEqual({
          version: 2,
          interfaceSize,
          colorTheme,
        });
      }
    }
  });

  it("validates version-2 fields independently", () => {
    expect(
      loadAppearancePreferences(
        stored(
          JSON.stringify({
            version: 2,
            interfaceSize: "huge",
            colorTheme: "dark",
          }),
        ),
      ),
    ).toEqual({ interfaceSize: "standard", colorTheme: "dark" });
    expect(
      loadAppearancePreferences(
        stored(
          JSON.stringify({
            version: 2,
            interfaceSize: "compact",
            colorTheme: "sepia",
          }),
        ),
      ),
    ).toEqual({ interfaceSize: "compact", colorTheme: "auto" });
  });

  it("rejects malformed and future stored values", () => {
    expect(loadAppearancePreferences(stored("not json"))).toEqual({
      interfaceSize: "standard",
      colorTheme: "auto",
    });
    expect(
      loadAppearancePreferences(
        stored(
          JSON.stringify({
            version: 3,
            interfaceSize: "compact",
            colorTheme: "dark",
          }),
        ),
      ),
    ).toEqual({ interfaceSize: "standard", colorTheme: "auto" });
  });

  it("applies validated stored themes to a document root", () => {
    const root = { dataset: {} } as Pick<HTMLElement, "dataset">;
    applyColorTheme("light", root);
    expect(root.dataset.colorTheme).toBe("light");

    const preferences = applyStoredColorTheme(
      stored(
        JSON.stringify({
          version: 2,
          interfaceSize: "spacious",
          colorTheme: "dark",
        }),
      ),
      root,
    );
    expect(preferences).toEqual({
      interfaceSize: "spacious",
      colorTheme: "dark",
    });
    expect(root.dataset.colorTheme).toBe("dark");
  });

  it("tolerates denied browser storage", () => {
    const read = vi.fn(() => {
      throw new Error("denied");
    });
    const write = vi.fn(() => {
      throw new Error("denied");
    });
    expect(loadAppearancePreferences({ getItem: read })).toEqual({
      interfaceSize: "standard",
      colorTheme: "auto",
    });
    expect(() =>
      saveAppearancePreferences(
        { interfaceSize: "spacious", colorTheme: "dark" },
        { setItem: write },
      ),
    ).not.toThrow();
  });
});

function stored(source: string) {
  return { getItem: () => source };
}
