import type { DesktopUpdaterSnapshot } from "../updater/types";

const TARGETS = new Set([
  "aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc",
  "aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu", "linux-gnu",
]);
const PACKAGES = new Set(["app", "nsis", "appimage", "msi", "deb", "rpm", "headless"]);
const PHASES = new Set(["idle", "checking", "up-to-date", "available", "manual-install",
  "downloading", "installing", "error"]);
const PRIVACY = new Set(["installation-id", "preference-controlled", "anonymous"]);

function member(value: unknown, allowed: ReadonlySet<string>): string {
  return typeof value === "string" && allowed.has(value) ? value : "unknown";
}

function bounded(value: unknown, pattern: RegExp): string {
  return typeof value === "string" && value.length <= 48 && pattern.test(value) ? value : "unknown";
}

/** Local support format, deliberately independent from raw application/log DTOs. */
export function buildDiagnostics(snapshot: DesktopUpdaterSnapshot): string {
  const { info, state } = snapshot;
  const report = {
    schema: 1,
    product: "RSTorrent",
    version: bounded(info.version, /^\d{1,6}\.\d{1,6}\.\d{1,6}(?:-(?:alpha|beta|rc)(?:\.\d{1,6})?)?$/),
    build: bounded(info.buildId, /^(?:[a-f0-9]{7,40}(?:-dirty)?|local|development|unknown)$/i),
    target: member(info.target, TARGETS),
    package: member(info.bundleType, PACKAGES),
    update_check_privacy: member(info.checkPrivacy, PRIVACY),
    updater_phase: member(state.phase, PHASES),
    ...(state.phase === "error" ? {
      updater_operation: state.operation === "check" || state.operation === "install"
        ? state.operation : "unknown",
    } : {}),
  };
  const text = JSON.stringify(report, null, 2) + "\n";
  if (new TextEncoder().encode(text).length > 2048) {
    throw new Error("support diagnostics exceeded its fixed bound");
  }
  return text;
}

export const DIAGNOSTICS_FILENAME = "rstorrent-diagnostics.json";
