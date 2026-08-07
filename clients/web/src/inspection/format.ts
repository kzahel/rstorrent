import type { DataUnits } from "./appearance";
import type { TorrentEta } from "./model";

const UNIT_SYSTEMS = {
  decimal: { base: 1000, suffixes: ["B", "kB", "MB", "GB", "TB", "PB"] },
  binary: { base: 1024, suffixes: ["B", "KiB", "MiB", "GiB", "TiB", "PiB"] },
} as const satisfies Record<
  DataUnits,
  { readonly base: number; readonly suffixes: readonly string[] }
>;

export function formatBytes(
  value: number | null,
  dataUnits: DataUnits,
): string {
  if (value === null) return "—";
  if (value === 0) return "0 B";
  const { base, suffixes } = UNIT_SYSTEMS[dataUnits];
  const exponent = Math.min(
    Math.floor(Math.log(Math.abs(value)) / Math.log(base)),
    suffixes.length - 1,
  );
  const scaled = value / base ** exponent;
  return `${scaled >= 100 || exponent === 0 ? scaled.toFixed(0) : scaled.toFixed(1)} ${suffixes[exponent]}`;
}

export function formatExactBytes(
  value: string | null,
  dataUnits: DataUnits,
): string {
  if (value === null) return "—";
  let bytes: bigint;
  try {
    bytes = BigInt(value);
  } catch {
    return "—";
  }
  if (bytes <= 0n) return "0 B";
  const { base, suffixes } = UNIT_SYSTEMS[dataUnits];
  const bigintBase = BigInt(base);
  let exponent = 0;
  let divisor = 1n;
  while (exponent < suffixes.length - 1 && bytes >= divisor * bigintBase) {
    exponent += 1;
    divisor *= bigintBase;
  }
  const whole = bytes / divisor;
  if (exponent === 0 || whole >= 100n) {
    return `${whole.toString()} ${suffixes[exponent]}`;
  }
  const tenth = ((bytes * 10n) / divisor) % 10n;
  return `${whole.toString()}.${tenth.toString()} ${suffixes[exponent]}`;
}

export function formatDecimalProgress(done: string, length: string): string {
  try {
    const doneBytes = BigInt(done);
    const lengthBytes = BigInt(length);
    if (lengthBytes === 0n) return "100%";
    const tenths = (doneBytes * 1_000n) / lengthBytes;
    return tenths >= 999n
      ? "100%"
      : `${(tenths / 10n).toString()}.${(tenths % 10n).toString()}%`;
  } catch {
    return "—";
  }
}

export function formatRate(value: number | null, dataUnits: DataUnits): string {
  return value === null || value <= 0
    ? "—"
    : `${formatBytes(value, dataUnits)}/s`;
}

export function formatProgress(value: number | null): string {
  if (value === null) return "Metadata";
  return `${(value * 100).toFixed(value >= 0.9995 ? 0 : 1)}%`;
}

export function formatEta(eta: TorrentEta): string {
  if (eta.state === "stalled") return "∞";
  const seconds = estimateSeconds(eta);
  if (seconds === null) return "—";
  if (seconds < 60n) return `${seconds.toString()}s`;
  if (seconds < 3_600n) {
    return `${(seconds / 60n).toString()}m ${(seconds % 60n).toString()}s`;
  }
  return `${(seconds / 3_600n).toString()}h ${((seconds % 3_600n) / 60n).toString()}m`;
}

export function etaAccessibleLabel(eta: TorrentEta): string {
  switch (eta.state) {
    case "estimate":
      return estimateSeconds(eta) === null
        ? "ETA unavailable"
        : `Estimated time remaining: ${formatEta(eta)}`;
    case "warming_up":
      return "Calculating ETA";
    case "stalled":
      return "Transfer stalled";
    case "unavailable":
      return "ETA unavailable";
  }
}

export function etaSortValue(eta: TorrentEta): string | null {
  return estimateSeconds(eta) === null || eta.state !== "estimate"
    ? null
    : eta.seconds;
}

function estimateSeconds(eta: TorrentEta): bigint | null {
  if (eta.state !== "estimate") return null;
  try {
    const seconds = BigInt(eta.seconds);
    return seconds > 0n ? seconds : null;
  } catch {
    return null;
  }
}

export function formatClock(milliseconds: number): string {
  const totalSeconds = Math.floor(milliseconds / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

export function formatTime(milliseconds: number): string {
  return new Intl.DateTimeFormat("en", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
    timeZone: "UTC",
  }).format(new Date(milliseconds));
}
