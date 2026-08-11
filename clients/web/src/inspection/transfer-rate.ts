import type { TransferRateLimit } from "../api";

export const RATE_LIMIT_MINIMUM_BYTES = 1_024;
export const RATE_LIMIT_MAXIMUM_BYTES = 4_294_967_295;

export interface RateLimitValidation {
  readonly limit: TransferRateLimit | null;
  readonly error: string | null;
}

export function validateRateLimit(
  unlimited: boolean,
  valueKiB: string,
): RateLimitValidation {
  if (unlimited) return { limit: { type: "unlimited" }, error: null };
  if (!/^\d+(?:\.\d+)?$/.test(valueKiB)) {
    return { limit: null, error: "Enter a positive rate in KiB/s." };
  }
  const bytesPerSecond = Number(valueKiB) * 1_024;
  if (
    !Number.isSafeInteger(bytesPerSecond) ||
    bytesPerSecond < RATE_LIMIT_MINIMUM_BYTES ||
    bytesPerSecond > RATE_LIMIT_MAXIMUM_BYTES
  ) {
    return {
      limit: null,
      error: "Enter at least 1 KiB/s and no more than 4,294,967,295 bytes/s.",
    };
  }
  return {
    limit: { type: "limited", bytes_per_second: bytesPerSecond },
    error: null,
  };
}

export function rateLimitDraftValue(
  limit: TransferRateLimit,
  fallback: string,
): string {
  return limit.type === "limited"
    ? String(limit.bytes_per_second / 1_024)
    : fallback;
}

export function sameRateLimit(
  left: TransferRateLimit,
  right: TransferRateLimit,
): boolean {
  return (
    left.type === right.type &&
    (left.type === "unlimited" ||
      (right.type === "limited" &&
        left.bytes_per_second === right.bytes_per_second))
  );
}

export function rateLimitLabel(limit: TransferRateLimit): string {
  return limit.type === "unlimited"
    ? "unlimited"
    : `${formatRateKiB(limit.bytes_per_second)} KiB/s`;
}

function formatRateKiB(bytesPerSecond: number): string {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 3 }).format(
    bytesPerSecond / 1_024,
  );
}
