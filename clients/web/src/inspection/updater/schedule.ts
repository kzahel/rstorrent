import type { CheckReason } from "./types";

export const STARTUP_CHECK_DELAY_MS = 5_000;
export const PERIODIC_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1_000;

type TimerHandle = ReturnType<typeof globalThis.setTimeout>;

export interface UpdaterTimers {
  setTimeout(callback: () => void, delay: number): TimerHandle;
  clearTimeout(handle: TimerHandle): void;
  setInterval(callback: () => void, delay: number): TimerHandle;
  clearInterval(handle: TimerHandle): void;
}

export function scheduleAutomaticChecks(
  check: (reason: Extract<CheckReason, "startup" | "periodic">) => void,
  timers: UpdaterTimers = globalThis,
): () => void {
  const startup = timers.setTimeout(
    () => check("startup"),
    STARTUP_CHECK_DELAY_MS,
  );
  const periodic = timers.setInterval(
    () => check("periodic"),
    PERIODIC_CHECK_INTERVAL_MS,
  );
  return () => {
    timers.clearTimeout(startup);
    timers.clearInterval(periodic);
  };
}
