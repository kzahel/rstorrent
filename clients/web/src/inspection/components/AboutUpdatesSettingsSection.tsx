import { message as localizedMessage } from "../../localization/runtime";
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
        <legend>{localizedMessage("inspection.components.about.updates.settings.section.application")}</legend>
        <dl className={styles.releaseFacts}>
          <div>
            <dt>{localizedMessage("inspection.components.about.updates.settings.section.version")}</dt>
            <dd>{info.version}</dd>
          </div>
          <div>
            <dt>{localizedMessage("inspection.components.about.updates.settings.section.build")}</dt>
            <dd title={info.buildId}>{shortBuildId(info.buildId)}</dd>
          </div>
          <div>
            <dt>{localizedMessage("inspection.components.about.updates.settings.section.target")}</dt>
            <dd>{info.target}</dd>
          </div>
          <div>
            <dt>{localizedMessage("inspection.components.about.updates.settings.section.package")}</dt>
            <dd>{packageLabel(info.bundleType)}</dd>
          </div>
        </dl>
      </fieldset>

      <fieldset className={styles.section}>
        <legend>{localizedMessage("inspection.components.about.updates.settings.section.updates")}</legend>
        <div className={styles.updateStatus} aria-live="polite">
          <strong>{statusTitle(state)}</strong>
          <span>{statusDetail(state, info.version)}</span>
        </div>
        {state.phase === "downloading" ? (
          <progress
            aria-label={localizedMessage("inspection.components.about.updates.settings.section.update.download.progress")}
            max={100}
            {...(progressPercent(state) === undefined
              ? {}
              : { value: progressPercent(state) })}
          />
        ) : null}
        {state.phase === "available" && state.notes ? (
          <div className={styles.releaseNotes}>
            <strong>{localizedMessage("inspection.components.about.updates.settings.section.release.notes")}</strong>
            <p>{state.notes}</p>
          </div>
        ) : null}
        {state.phase === "available" && state.manualApply !== undefined ? (
          <div className={styles.manualUpdate}>
            <strong>{localizedMessage("inspection.components.about.updates.settings.section.apply.from.a.shell.on.this.server")}</strong>
            <code>{state.manualApply.command}</code>
            <a href={state.manualApply.releaseUrl} rel="noreferrer" target="_blank">{localizedMessage("inspection.components.about.updates.settings.section.review.signed.release")}</a>
          </div>
        ) : null}
        <div className={styles.updateActions}>
          <button
            type="button"
            disabled={state.phase === "checking" || isInstalling(state)}
            onClick={() => void updater.check("manual")}
          >
            {state.phase === "checking" ? localizedMessage("inspection.components.about.updates.settings.section.checking") : localizedMessage("inspection.components.about.updates.settings.section.check.for.updates")}
          </button>
          {(state.phase === "available" && state.manualApply === undefined) ||
          (state.phase === "error" && state.operation === "install") ? (
            <button
              type="button"
              className={styles.primaryAction}
              disabled={isInstalling(state)}
              onClick={() => void updater.install()}
            >{localizedMessage("inspection.components.about.updates.settings.section.install.and.restart")}</button>
          ) : null}
          {state.phase === "manual-install" ? (
            <a href={RELEASES_URL} rel="noreferrer" target="_blank">{localizedMessage("inspection.components.about.updates.settings.section.open.release.downloads")}</a>
          ) : null}
        </div>
        <p className={styles.updatePrivacy}>{localizedMessage("inspection.components.about.updates.settings.section.rstorrent.checks.automatically.after.startup.and.about")}{info.checkPrivacy === "anonymous"
            ? localizedMessage("inspection.components.about.updates.settings.section.headless.checks.include.no.installation.identifier")
            : info.checkPrivacy === "preference-controlled"
              ? localizedMessage("inspection.components.about.updates.settings.section.checks.follow.the.usage.statistics.preference")
              : localizedMessage("inspection.components.about.updates.settings.section.checks.include.a.random.resettable.installation.identifier")}
        </p>
      </fieldset>
    </div>
  );
}

function statusTitle(state: UpdaterState): string {
  switch (state.phase) {
    case "idle":
      return localizedMessage("inspection.components.about.updates.settings.section.automatic.updates.enabled");
    case "checking":
      return localizedMessage("inspection.components.about.updates.settings.section.checking.for.updates");
    case "up-to-date":
      return localizedMessage("inspection.components.about.updates.settings.section.rstorrent.is.up.to.date");
    case "available":
      return `RSTorrent ${state.version} is available`;
    case "manual-install":
      return localizedMessage("inspection.components.about.updates.settings.section.manual.update.required");
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
      return localizedMessage("inspection.components.about.updates.settings.section.contacting.the.rstorrent.update.service");
    case "up-to-date":
      return `Version ${currentVersion} is the newest compatible release.`;
    case "available":
      return state.manualApply === undefined
        ? "Installation happens only after you approve it."
        : "The browser checks the signed channel but cannot replace the running service.";
    case "manual-install":
      return `This ${state.packageLabel} stays with its package channel.`;
    case "downloading": {
      const percent = progressPercent(state);
      return percent === undefined
        ? `${formatBytes(state.downloadedBytes)} downloaded.`
        : `${percent}% · ${formatBytes(state.downloadedBytes)} downloaded.`;
    }
    case "installing":
      return localizedMessage("inspection.components.about.updates.settings.section.rstorrent.will.relaunch.after.installation.succeeds");
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
      return localizedMessage("inspection.components.about.updates.settings.section.macos.app");
    case "nsis":
      return localizedMessage("inspection.components.about.updates.settings.section.windows.nsis");
    case "appimage":
      return localizedMessage("inspection.components.about.updates.settings.section.linux.appimage");
    case "msi":
      return localizedMessage("inspection.components.about.updates.settings.section.windows.msi");
    case "deb":
      return localizedMessage("inspection.components.about.updates.settings.section.linux.deb");
    case "rpm":
      return localizedMessage("inspection.components.about.updates.settings.section.linux.rpm");
    case "headless":
      return localizedMessage("inspection.components.about.updates.settings.section.linux.headless.service");
    case "unknown":
      return localizedMessage("inspection.components.about.updates.settings.section.development.build");
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KiB`;
  return `${(bytes / 1_048_576).toFixed(1)} MiB`;
}
