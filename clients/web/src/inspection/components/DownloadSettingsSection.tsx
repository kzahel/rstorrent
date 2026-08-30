import { useState } from "react";

import type { DownloadRoot, DownloadStorageSettings } from "../model";
import {
  CrostiniStorageHelp,
  describeCrostiniStoragePath,
} from "./CrostiniStorageHelp";
import styles from "./SettingsDialog.module.css";

interface DownloadSettingsSectionProps {
  readonly storage: DownloadStorageSettings;
  readonly manageable: boolean;
  readonly showCrostiniStorageHelp: boolean;
  readonly oneCurrentRoot?: boolean;
  readonly onChooseFolder: (repairRoot?: string) => Promise<DownloadRoot | null>;
  readonly onDefaultRootChange: (rootId: string) => Promise<void>;
  readonly onShowAddOptionsChange: (show: boolean) => Promise<void>;
  readonly onRemoveRoot: (rootId: string) => Promise<void>;
}

export function DownloadSettingsSection({
  storage,
  manageable,
  showCrostiniStorageHelp,
  oneCurrentRoot = false,
  onChooseFolder,
  onDefaultRootChange,
  onShowAddOptionsChange,
  onRemoveRoot,
}: DownloadSettingsSectionProps) {
  const [pendingAction, setPendingAction] = useState<string | null>(null);
  const [storageStatus, setStorageStatus] = useState("");

  const runStorageAction = async (
    action: string,
    operation: () => Promise<string>,
  ) => {
    setPendingAction(action);
    setStorageStatus("");
    try {
      setStorageStatus(await operation());
    } catch (error) {
      setStorageStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setPendingAction(null);
    }
  };

  return (
    <fieldset className={styles.section}>
      <legend>Downloads</legend>
      {!manageable ? (
        <p className={styles.storageNote}>
          Download folders are managed by the live application.
        </p>
      ) : (
        <>
          <div className={styles.settingHeading}>
            <strong>Download folders</strong>
            <span>
              {oneCurrentRoot
                ? "The current folder applies to future torrents only. Existing torrents stay attached to their earlier folder."
                : "The default applies to future torrents only. Existing torrents stay attached to their selected folder."}
            </span>
          </div>
          {showCrostiniStorageHelp ? <CrostiniStorageHelp /> : null}
          <div className={styles.rootList}>
            {storage.roots.length === 0 ? (
              <p className={styles.storageNote}>
                No download folder has been chosen yet.
              </p>
            ) : (
              storage.roots.map((root) => {
                const performance = showCrostiniStorageHelp
                  ? describeCrostiniStoragePath(root.path)
                  : null;
                return (
                  <article
                    key={root.id}
                    className={styles.root}
                    data-availability={root.availability}
                  >
                    <div>
                      <strong>{root.label}</strong>
                      <span>
                        {root.path ??
                          (oneCurrentRoot && root.availability === "available"
                            ? "Managed by Android"
                            : "Location is unavailable")}
                      </span>
                      <small>
                        {root.availability === "available"
                          ? root.id === storage.defaultRoot
                            ? oneCurrentRoot
                              ? "Current download folder"
                              : "Default download folder"
                            : "Available"
                          : "Unavailable — repair required"}
                      </small>
                      {performance === null ? null : (
                        <small className={styles.rootPerformance}>
                          {performance}
                        </small>
                      )}
                    </div>
                    <div className={styles.rootActions}>
                      {root.availability === "unavailable" ? (
                        <button
                          type="button"
                          disabled={pendingAction !== null}
                          onClick={() =>
                            void runStorageAction(
                              `repair-${root.id}`,
                              async () => {
                                const repaired = await onChooseFolder(root.id);
                                return repaired === null
                                  ? "Folder selection canceled"
                                  : `${repaired.label} repaired`;
                              },
                            )
                          }
                        >
                          {pendingAction === `repair-${root.id}`
                            ? "Repairing…"
                            : "Repair…"}
                        </button>
                      ) : root.id !== storage.defaultRoot ? (
                        <button
                          type="button"
                          disabled={pendingAction !== null}
                          onClick={() =>
                            void runStorageAction(
                              `default-${root.id}`,
                              async () => {
                                await onDefaultRootChange(root.id);
                                return `${root.label} is now the default`;
                              },
                            )
                          }
                        >
                          {oneCurrentRoot ? "Make current" : "Make default"}
                        </button>
                      ) : null}
                      {root.id === storage.defaultRoot && oneCurrentRoot ? null : (
                        <button
                          type="button"
                          disabled={pendingAction !== null}
                          onClick={() =>
                            void runStorageAction(
                              `remove-${root.id}`,
                              async () => {
                                await onRemoveRoot(root.id);
                                return `${root.label} removed`;
                              },
                            )
                          }
                        >
                          {pendingAction === `remove-${root.id}`
                            ? "Removing…"
                            : "Remove"}
                        </button>
                      )}
                    </div>
                  </article>
                );
              })
            )}
          </div>
          <button
            className={styles.addFolder}
            type="button"
            disabled={pendingAction !== null}
            onClick={() =>
              void runStorageAction("add", async () => {
                const root = await onChooseFolder();
                return root === null
                  ? "Folder selection canceled"
                  : `${root.label} added`;
              })
            }
          >
            {pendingAction === "add" ? "Choosing…" : "Add folder…"}
          </button>
          {oneCurrentRoot ? null : (
            <label className={styles.preference}>
              <input
                type="checkbox"
                checked={storage.showAddOptions}
                disabled={pendingAction !== null}
                onChange={(event) => {
                  const show = event.currentTarget.checked;
                  void runStorageAction("preference", async () => {
                    await onShowAddOptionsChange(show);
                    return show
                      ? "Add options will be shown"
                      : "The usable default will be used automatically";
                  });
                }}
              />
              <span>
                <strong>Show options when adding torrents</strong>
                <small>
                  Options are always shown when no usable default exists.
                </small>
              </span>
            </label>
          )}
          {storageStatus === "" ? null : (
            <output className={styles.storageStatus} aria-live="polite">
              {storageStatus}
            </output>
          )}
        </>
      )}
    </fieldset>
  );
}
