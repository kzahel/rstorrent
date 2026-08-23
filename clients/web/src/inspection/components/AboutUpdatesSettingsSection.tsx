import { progressPercent } from "../updater/controller";
import type {
  DesktopUpdater,
  DesktopUpdaterSnapshot,
  UpdaterState,
} from "../updater/types";
import styles from "./SettingsDialog.module.css";

const RELEASES_URL = "https://github.com/kzahel/rstorrent/releases/latest";

export interface AboutUpdatesSettingsSectionProps {
  readonly updater: DesktopUpdater;
  readonly snapshot: DesktopUpdaterSnapshot;
}

export function AboutUpdatesSettingsSection({
  updater,
  snapshot,
}: AboutUpdatesSettingsSectionProps) {
  const { info, state } = snapshot;
  return (
    <div className={styles.aboutUpdates}>
      <fieldset className={styles.section}>
        <legend>Application</legend>
        <dl className={styles.releaseFacts}>
          <div>
            <dt>Version</dt>
            <dd>{info.version}</dd>
          </div>
          <div>
            <dt>Build</dt>
            <dd title={info.buildId}>{shortBuildId(info.buildId)}</dd>
          </div>
          <div>
            <dt>Target</dt>
            <dd>{info.target}</dd>
          </div>
          <div>
            <dt>Package</dt>
            <dd>{packageLabel(info.bundleType)}</dd>
          </div>
        </dl>
      </fieldset>

      <fieldset className={styles.section}>
        <legend>Updates</legend>
        <div className={styles.updateStatus} aria-live="polite">
          <strong>{statusTitle(state)}</strong>
          <span>{statusDetail(state, info.version)}</span>
        </div>
        {state.phase === "downloading" ? (
          <progress
            aria-label="Update download progress"
            max={100}
            {...(progressPercent(state) === undefined
              ? {}
              : { value: progressPercent(state) })}
          />
        ) : null}
        {state.phase === "available" && state.notes ? (
          <div className={styles.releaseNotes}>
            <strong>Release notes</strong>
            <p>{state.notes}</p>
          </div>
        ) : null}
        <div className={styles.updateActions}>
          <button
            type="button"
            disabled={state.phase === "checking" || isInstalling(state)}
            onClick={() => void updater.check("manual")}
          >
            {state.phase === "checking" ? "Checking…" : "Check for updates"}
          </button>
          {state.phase === "available" ||
          (state.phase === "error" && state.operation === "install") ? (
            <button
              type="button"
              className={styles.primaryAction}
              disabled={isInstalling(state)}
              onClick={() => void updater.install()}
            >
              Install and restart
            </button>
          ) : null}
          {state.phase === "manual-install" ? (
            <a href={RELEASES_URL} rel="noreferrer" target="_blank">
              Open release downloads
            </a>
          ) : null}
        </div>
        <p className={styles.updatePrivacy}>
          RSTorrent checks automatically after startup and about once per day.
          Checks include a random resettable installation identifier used only
          to estimate active installations.
        </p>
      </fieldset>
    </div>
  );
}

function statusTitle(state: UpdaterState): string {
  switch (state.phase) {
    case "idle":
      return "Automatic updates enabled";
    case "checking":
      return "Checking for updates";
    case "up-to-date":
      return "RSTorrent is up to date";
    case "available":
      return `RSTorrent ${state.version} is available`;
    case "manual-install":
      return "Manual update required";
    case "downloading":
      return `Downloading RSTorrent ${state.version}`;
    case "installing":
      return `Installing RSTorrent ${state.version}`;
    case "error":
      return state.operation === "check"
        ? "Update check failed"
        : "Update installation failed";
  }
}

function statusDetail(state: UpdaterState, currentVersion: string): string {
  switch (state.phase) {
    case "idle":
      return `Currently running ${currentVersion}.`;
    case "checking":
      return "Contacting the RSTorrent update service.";
    case "up-to-date":
      return `Version ${currentVersion} is the newest compatible release.`;
    case "available":
      return "Installation happens only after you approve it.";
    case "manual-install":
      return `This ${state.packageLabel} stays with its package channel.`;
    case "downloading": {
      const percent = progressPercent(state);
      return percent === undefined
        ? `${formatBytes(state.downloadedBytes)} downloaded.`
        : `${percent}% · ${formatBytes(state.downloadedBytes)} downloaded.`;
    }
    case "installing":
      return "RSTorrent will relaunch after installation succeeds.";
    case "error":
      return state.message;
  }
}

function isInstalling(state: UpdaterState): boolean {
  return state.phase === "downloading" || state.phase === "installing";
}

function shortBuildId(buildId: string): string {
  return buildId === "development" ? buildId : buildId.slice(0, 12);
}

function packageLabel(bundleType: DesktopUpdaterSnapshot["info"]["bundleType"]): string {
  switch (bundleType) {
    case "app":
      return "macOS app";
    case "nsis":
      return "Windows NSIS";
    case "appimage":
      return "Linux AppImage";
    case "msi":
      return "Windows MSI";
    case "deb":
      return "Linux DEB";
    case "rpm":
      return "Linux RPM";
    case "unknown":
      return "Development build";
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KiB`;
  return `${(bytes / 1_048_576).toFixed(1)} MiB`;
}
