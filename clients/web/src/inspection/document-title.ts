import type { DataUnits } from "./appearance";
import { formatBytes } from "./format";
import type { SessionSummary } from "./model";

export const APPLICATION_TITLE = "RSTorrent";

export function documentTitleForSession(
  session: SessionSummary,
  dataUnits: DataUnits,
): string {
  if (session.connection !== "connected" && session.connection !== "demo") {
    return APPLICATION_TITLE;
  }
  const downloadRate = nonnegativeRate(session.downloadRate);
  const uploadRate = nullableNonnegativeRate(session.uploadRate);
  if (downloadRate === 0 && (uploadRate === null || uploadRate === 0)) {
    return APPLICATION_TITLE;
  }
  return (
    `${APPLICATION_TITLE} - ↓${titleRate(downloadRate, dataUnits)}` +
    ` ↑${titleRate(uploadRate, dataUnits)}`
  );
}

function titleRate(value: number | null, dataUnits: DataUnits): string {
  return value === null ? "—" : `${formatBytes(value, dataUnits)}/s`;
}

function nullableNonnegativeRate(value: number | null): number | null {
  return value === null ? null : nonnegativeRate(value);
}

function nonnegativeRate(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0;
}
