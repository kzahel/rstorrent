export const INTERFACE_SIZES = ["compact", "standard", "spacious"] as const;

export type InterfaceSize = (typeof INTERFACE_SIZES)[number];

export interface InterfaceMetrics {
  readonly tableHeaderHeight: number;
  readonly tableRowHeight: number;
}

export const DEFAULT_INTERFACE_SIZE: InterfaceSize = "standard";

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

const APPEARANCE_VERSION = 1;

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;

export type AppearanceStorage = ReadableStorage & WritableStorage;

export function isInterfaceSize(value: unknown): value is InterfaceSize {
  return INTERFACE_SIZES.some((candidate) => candidate === value);
}

export function loadInterfaceSize(
  storage: ReadableStorage | null = browserStorage(),
): InterfaceSize {
  if (storage === null) return DEFAULT_INTERFACE_SIZE;
  try {
    const source = storage.getItem(APPEARANCE_STORAGE_KEY);
    if (source === null) return DEFAULT_INTERFACE_SIZE;
    const value = JSON.parse(source) as {
      readonly version?: unknown;
      readonly interfaceSize?: unknown;
    };
    return value.version === APPEARANCE_VERSION &&
      isInterfaceSize(value.interfaceSize)
      ? value.interfaceSize
      : DEFAULT_INTERFACE_SIZE;
  } catch {
    return DEFAULT_INTERFACE_SIZE;
  }
}

export function saveInterfaceSize(
  interfaceSize: InterfaceSize,
  storage: WritableStorage | null = browserStorage(),
): void {
  if (storage === null) return;
  try {
    storage.setItem(
      APPEARANCE_STORAGE_KEY,
      JSON.stringify({
        version: APPEARANCE_VERSION,
        interfaceSize,
      }),
    );
  } catch {
    // Browser-local presentation settings are optional in sandboxed contexts.
  }
}

function browserStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}
