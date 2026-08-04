export const INTERFACE_SIZES = ["compact", "standard", "spacious"] as const;
export const COLOR_THEMES = ["auto", "light", "dark"] as const;

export type InterfaceSize = (typeof INTERFACE_SIZES)[number];
export type ColorTheme = (typeof COLOR_THEMES)[number];

export interface InterfaceMetrics {
  readonly tableHeaderHeight: number;
  readonly tableRowHeight: number;
}

export interface AppearancePreferences {
  readonly interfaceSize: InterfaceSize;
  readonly colorTheme: ColorTheme;
}

export const DEFAULT_INTERFACE_SIZE: InterfaceSize = "standard";
export const DEFAULT_COLOR_THEME: ColorTheme = "auto";

export const INTERFACE_METRICS: Readonly<
  Record<InterfaceSize, InterfaceMetrics>
> = {
  compact: {
    tableHeaderHeight: 34,
    tableRowHeight: 32,
  },
  standard: {
    tableHeaderHeight: 38,
    tableRowHeight: 36,
  },
  spacious: {
    tableHeaderHeight: 44,
    tableRowHeight: 42,
  },
};

export const APPEARANCE_STORAGE_KEY = "rstorrent.presentation.appearance";

const APPEARANCE_VERSION = 2;
const LEGACY_APPEARANCE_VERSION = 1;

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;
type AppearanceRoot = Pick<HTMLElement, "dataset">;

export type AppearanceStorage = ReadableStorage & WritableStorage;

export function isInterfaceSize(value: unknown): value is InterfaceSize {
  return INTERFACE_SIZES.some((candidate) => candidate === value);
}

export function isColorTheme(value: unknown): value is ColorTheme {
  return COLOR_THEMES.some((candidate) => candidate === value);
}

export function loadAppearancePreferences(
  storage: ReadableStorage | null = browserStorage(),
): AppearancePreferences {
  if (storage === null) return defaultAppearancePreferences();
  try {
    const source = storage.getItem(APPEARANCE_STORAGE_KEY);
    if (source === null) return defaultAppearancePreferences();
    const value = JSON.parse(source) as {
      readonly version?: unknown;
      readonly interfaceSize?: unknown;
      readonly colorTheme?: unknown;
    };
    if (value.version === LEGACY_APPEARANCE_VERSION) {
      return {
        interfaceSize: isInterfaceSize(value.interfaceSize)
          ? value.interfaceSize
          : DEFAULT_INTERFACE_SIZE,
        colorTheme: DEFAULT_COLOR_THEME,
      };
    }
    if (value.version !== APPEARANCE_VERSION) {
      return defaultAppearancePreferences();
    }
    return {
      interfaceSize: isInterfaceSize(value.interfaceSize)
        ? value.interfaceSize
        : DEFAULT_INTERFACE_SIZE,
      colorTheme: isColorTheme(value.colorTheme)
        ? value.colorTheme
        : DEFAULT_COLOR_THEME,
    };
  } catch {
    return defaultAppearancePreferences();
  }
}

export function saveAppearancePreferences(
  preferences: AppearancePreferences,
  storage: WritableStorage | null = browserStorage(),
): void {
  if (storage === null) return;
  try {
    storage.setItem(
      APPEARANCE_STORAGE_KEY,
      JSON.stringify({
        version: APPEARANCE_VERSION,
        interfaceSize: preferences.interfaceSize,
        colorTheme: preferences.colorTheme,
      }),
    );
  } catch {
    // Browser-local presentation settings are optional in sandboxed contexts.
  }
}

export function applyColorTheme(
  colorTheme: ColorTheme,
  root: AppearanceRoot | null = browserDocumentRoot(),
): void {
  if (root !== null) root.dataset.colorTheme = colorTheme;
}

export function applyInterfaceSize(
  interfaceSize: InterfaceSize,
  root: AppearanceRoot | null = browserDocumentRoot(),
): void {
  if (root !== null) root.dataset.interfaceSize = interfaceSize;
}

export function applyAppearancePreferences(
  preferences: AppearancePreferences,
  root: AppearanceRoot | null = browserDocumentRoot(),
): void {
  applyColorTheme(preferences.colorTheme, root);
  applyInterfaceSize(preferences.interfaceSize, root);
}

export function applyStoredAppearance(
  storage: ReadableStorage | null = browserStorage(),
  root: AppearanceRoot | null = browserDocumentRoot(),
): AppearancePreferences {
  const preferences = loadAppearancePreferences(storage);
  applyAppearancePreferences(preferences, root);
  return preferences;
}

function defaultAppearancePreferences(): AppearancePreferences {
  return {
    interfaceSize: DEFAULT_INTERFACE_SIZE,
    colorTheme: DEFAULT_COLOR_THEME,
  };
}

function browserStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function browserDocumentRoot(): HTMLElement | null {
  try {
    return globalThis.document?.documentElement ?? null;
  } catch {
    return null;
  }
}
