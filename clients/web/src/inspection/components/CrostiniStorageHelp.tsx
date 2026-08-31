import { message as localizedMessage } from "../../localization/runtime";
import styles from "./CrostiniStorageHelp.module.css";

const LINUX_DOWNLOADS = /^\/home\/[^/]+\/Downloads(?:\/.*)?$/u;

export function describeCrostiniStoragePath(
  path: string | null,
): string | null {
  if (path === null) return null;
  if (path === "/mnt/chromeos" || path.startsWith("/mnt/chromeos/")) {
    return localizedMessage("inspection.components.crostini.storage.help.chromeos.shared.folder.convenient.but.slower");
  }
  if (path === "~/Downloads" || LINUX_DOWNLOADS.test(path)) {
    return localizedMessage("inspection.components.crostini.storage.help.linux.downloads.faster.recommended");
  }
  return null;
}

export function CrostiniStorageHelp() {
  return (
    <aside className={styles.callout} aria-label={localizedMessage("inspection.components.crostini.storage.help.chromebook.storage.guidance")}>
      <strong>{localizedMessage("inspection.components.crostini.storage.help.linux.downloads.is.faster")}</strong>
      <p>{localizedMessage("inspection.components.crostini.storage.help.keep.downloads.in.linux.for.the.best")}{" "}<b>{localizedMessage("inspection.components.crostini.storage.help.linux.files")}</b>
        {" → "}
        <b>{localizedMessage("inspection.components.crostini.storage.help.downloads")}</b>{localizedMessage("inspection.components.crostini.storage.help.no.sharing.step.is.required")}</p>
      <details>
        <summary>{localizedMessage("inspection.components.crostini.storage.help.how.to.use.a.folder.from.my")}</summary>
        <div className={styles.instructions}>
          <p>{localizedMessage("inspection.components.crostini.storage.help.chromeos.calls.this.permission.share.with.linux")}</p>
          <ol>
            <li>{localizedMessage("inspection.components.crostini.storage.help.open.the.chromeos.files.app")}</li>
            <li>{localizedMessage("inspection.components.crostini.storage.help.under")}{" "}<b>{localizedMessage("inspection.components.crostini.storage.help.my.files")}</b>{localizedMessage("inspection.components.crostini.storage.help.right.click")}{" "}<b>{localizedMessage("inspection.components.crostini.storage.help.downloads")}</b>{" "}{localizedMessage("inspection.components.crostini.storage.help.or.another.folder.and.choose")}{" "}<b>{localizedMessage("inspection.components.crostini.storage.help.share.with.linux")}</b>.
            </li>
            <li>{localizedMessage("inspection.components.crostini.storage.help.return.to.rstorrent.and.choose")}{" "}<b>{localizedMessage("inspection.components.crostini.storage.help.choose.folder")}</b>{" "}{localizedMessage("inspection.components.crostini.storage.help.or")}{" "}
              <b>{localizedMessage("inspection.components.crostini.storage.help.add.folder")}</b>.
            </li>
            <li>{localizedMessage("inspection.components.crostini.storage.help.in.the.folder.picker.select.the.folder")}{" "}<b>{localizedMessage("inspection.components.crostini.storage.help.downloads")}</b>{" "}{localizedMessage("inspection.components.crostini.storage.help.if.you.shared")}{" "}
              <b>{localizedMessage("inspection.components.crostini.storage.help.my.files")}</b> {" → "} <b>{localizedMessage("inspection.components.crostini.storage.help.downloads")}</b>.
            </li>
          </ol>
          <p>{localizedMessage("inspection.components.crostini.storage.help.a.shared.chromeos.folder.is.easier.to")}</p>
        </div>
      </details>
    </aside>
  );
}
