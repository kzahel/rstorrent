import { installPolicy } from "./policy";
import { scheduleAutomaticChecks, type UpdaterTimers } from "./schedule";
import type {
  CheckReason,
  DesktopReleaseInfo,
  DesktopUpdateBackend,
  DesktopUpdater,
  DesktopUpdaterSnapshot,
  UpdateCandidate,
  UpdaterState,
} from "./types";

export const UPDATE_CHECK_TIMEOUT_MS = 20_000;
const MAX_RELEASE_NOTES_CHARS = 16_384;

export class DesktopUpdaterController implements DesktopUpdater {
  readonly getSnapshot = () => this.snapshot;
  readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  private snapshot: DesktopUpdaterSnapshot;
  private readonly listeners = new Set<() => void>();
  private candidate: UpdateCandidate | null = null;
  private activeCheck: Promise<void> | null = null;
  private readonly disposeSchedule: () => void;
  private closed = false;

  constructor(
    private readonly backend: DesktopUpdateBackend,
    info: DesktopReleaseInfo,
    timers: UpdaterTimers = globalThis,
  ) {
    this.snapshot = { info, state: { phase: "idle" } };
    this.disposeSchedule = scheduleAutomaticChecks(
      (reason) => void this.check(reason),
      timers,
    );
  }

  async check(reason: CheckReason = "manual"): Promise<void> {
    if (this.closed) return;
    if (this.activeCheck !== null) {
      await this.activeCheck;
      return;
    }
    const request = this.performCheck(reason);
    this.activeCheck = request;
    try {
      await request;
    } finally {
      if (this.activeCheck === request) this.activeCheck = null;
    }
  }

  async install(): Promise<void> {
    const candidate = this.candidate;
    if (this.closed) return;
    if (candidate === null) {
      await this.check("manual");
      return;
    }
    if (candidate.manualApply !== undefined) return;

    const version = candidate.version;
    let downloadedBytes = 0;
    let totalBytes: number | undefined;
    this.setState({ phase: "downloading", version, downloadedBytes });
    try {
      await candidate.downloadAndInstall((event) => {
        if (event.type === "started") {
          downloadedBytes = 0;
          totalBytes = event.contentLength;
          this.setState({
            phase: "downloading",
            version,
            downloadedBytes,
            ...(totalBytes === undefined ? {} : { totalBytes }),
          });
        } else if (event.type === "progress") {
          downloadedBytes += event.chunkLength;
          this.setState({
            phase: "downloading",
            version,
            downloadedBytes,
            ...(totalBytes === undefined ? {} : { totalBytes }),
          });
        } else {
          this.setState({ phase: "installing", version });
        }
      });
      this.setState({ phase: "installing", version });
      await this.backend.relaunch();
    } catch (error) {
      this.setState({
        phase: "error",
        operation: "install",
        message: errorMessage(error),
        version,
      });
    }
  }

  dismiss(): void {
    if (this.closed) return;
    this.closeCandidate();
    this.setState({ phase: "idle" });
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.disposeSchedule();
    this.closeCandidate();
    this.listeners.clear();
  }

  private async performCheck(reason: CheckReason): Promise<void> {
    const policy = installPolicy(this.snapshot.info.bundleType);
    if (!policy.canCheck) {
      if (reason === "manual") {
        this.setState({
          phase: "manual-install",
          packageLabel: policy.packageLabel,
        });
      }
      return;
    }
    if (reason !== "manual" && this.candidate !== null) return;
    if (reason === "manual") this.setState({ phase: "checking", reason });

    try {
      if (reason === "manual") this.closeCandidate();
      const candidate = await this.backend.check(
        reason,
        UPDATE_CHECK_TIMEOUT_MS,
      );
      if (this.closed) {
        if (candidate !== null) void candidate.close().catch(console.error);
        return;
      }
      if (candidate === null) {
        this.setState(
          reason === "manual"
            ? { phase: "up-to-date", reason }
            : { phase: "idle", lastReason: reason },
        );
        return;
      }
      this.candidate = candidate;
      this.setState({
        phase: "available",
        version: candidate.version,
        ...(candidate.notes === undefined
          ? {}
          : { notes: boundedReleaseNotes(candidate.notes) }),
        ...(candidate.manualApply === undefined
          ? {}
          : { manualApply: candidate.manualApply }),
        reason,
      });
    } catch (error) {
      if (reason === "manual") {
        this.setState({
          phase: "error",
          operation: "check",
          message: errorMessage(error),
        });
      } else {
        console.error(`Automatic ${reason} update check failed:`, error);
      }
    }
  }

  private closeCandidate(): void {
    const candidate = this.candidate;
    this.candidate = null;
    if (candidate !== null) void candidate.close().catch(console.error);
  }

  private setState(state: UpdaterState): void {
    if (this.closed) return;
    this.snapshot = { ...this.snapshot, state };
    for (const listener of this.listeners) listener();
  }
}

export function progressPercent(state: UpdaterState): number | undefined {
  if (
    state.phase !== "downloading" ||
    state.totalBytes === undefined ||
    state.totalBytes <= 0
  ) {
    return undefined;
  }
  return Math.min(
    100,
    Math.round((state.downloadedBytes / state.totalBytes) * 100),
  );
}

function boundedReleaseNotes(notes: string): string {
  return notes.length <= MAX_RELEASE_NOTES_CHARS
    ? notes
    : `${notes.slice(0, MAX_RELEASE_NOTES_CHARS)}\n…`;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
