import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type RefObject,
} from "react";

import type { ColorTheme, InterfaceSize } from "../appearance";
import type { DownloadRoot, DownloadStorageSettings } from "../model";
import { Icon } from "./Icon";
import styles from "./SettingsDialog.module.css";

const COLOR_THEME_OPTIONS: readonly {
  readonly value: ColorTheme;
  readonly label: string;
  readonly description: string;
}[] = [
  {
    value: "auto",
    label: "Auto",
    description: "Follow your system appearance.",
  },
  {
    value: "light",
    label: "Light",
    description: "Always use the light appearance.",
  },
  {
    value: "dark",
    label: "Dark",
    description: "Always use the dark appearance.",
  },
];

const INTERFACE_SIZE_OPTIONS: readonly {
  readonly value: InterfaceSize;
  readonly label: string;
  readonly description: string;
}[] = [
  {
    value: "compact",
    label: "Compact",
    description: "Fit more information on screen.",
  },
  {
    value: "standard",
    label: "Standard",
    description: "Balanced text, controls, and table spacing.",
  },
  {
    value: "spacious",
    label: "Spacious",
    description: "Use larger text and more generous targets.",
  },
];

export interface SettingsDialogProps {
  readonly colorTheme: ColorTheme;
  readonly interfaceSize: InterfaceSize;
  readonly storage: DownloadStorageSettings;
  readonly downloadsManageable: boolean;
  readonly returnFocus: RefObject<HTMLButtonElement | null>;
  readonly onColorThemeChange: (colorTheme: ColorTheme) => void;
  readonly onInterfaceSizeChange: (interfaceSize: InterfaceSize) => void;
  readonly onChooseFolder: (repairRoot?: string) => Promise<DownloadRoot | null>;
  readonly onDefaultRootChange: (rootId: string) => Promise<void>;
  readonly onShowAddOptionsChange: (show: boolean) => Promise<void>;
  readonly onRemoveRoot: (rootId: string) => Promise<void>;
  readonly onClose: () => void;
}

export function SettingsDialog({
  colorTheme,
  interfaceSize,
  storage,
  downloadsManageable,
  returnFocus,
  onColorThemeChange,
  onInterfaceSizeChange,
  onChooseFolder,
  onDefaultRootChange,
  onShowAddOptionsChange,
  onRemoveRoot,
  onClose,
}: SettingsDialogProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
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

  useEffect(() => {
    closeRef.current?.focus();
    return () => returnFocus.current?.focus();
  }, [returnFocus]);

  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = dialogRef.current?.querySelectorAll<HTMLElement>(
      'button:not(:disabled), input:not(:disabled), [tabindex]:not([tabindex="-1"])',
    );
    if (focusable === undefined || focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  };

  const closeFromBackdrop = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) onClose();
  };

  return (
    <div className={styles.backdrop} onMouseDown={closeFromBackdrop}>
      <section
        ref={dialogRef}
        className={styles.sheet}
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
        onKeyDown={handleKeyDown}
      >
        <header className={styles.header}>
          <div>
            <p>Application</p>
            <h2 id="settings-title">Settings</h2>
          </div>
          <button
            ref={closeRef}
            className={styles.close}
            type="button"
            aria-label="Close settings"
            onClick={onClose}
          >
            <Icon name="close" />
          </button>
        </header>
        <div className={styles.content}>
          <fieldset className={styles.section}>
            <legend>Appearance</legend>
            <div
              className={styles.settingGroup}
              role="group"
              aria-labelledby="color-theme-heading"
            >
              <div className={styles.settingHeading}>
                <strong id="color-theme-heading">Color theme</strong>
                <span>Choose a palette or follow your system.</span>
              </div>
              <div className={styles.options}>
                {COLOR_THEME_OPTIONS.map((option) => (
                  <label key={option.value} className={styles.option}>
                    <input
                      type="radio"
                      name="color-theme"
                      value={option.value}
                      checked={colorTheme === option.value}
                      onChange={() => onColorThemeChange(option.value)}
                    />
                    <span>
                      <strong>{option.label}</strong>
                      <small>{option.description}</small>
                    </span>
                  </label>
                ))}
              </div>
            </div>
            <div
              className={styles.settingGroup}
              role="group"
              aria-labelledby="interface-size-heading"
            >
              <div className={styles.settingHeading}>
                <strong id="interface-size-heading">Interface size</strong>
                <span>Changes apply immediately.</span>
              </div>
              <div className={styles.options}>
                {INTERFACE_SIZE_OPTIONS.map((option) => (
                  <label key={option.value} className={styles.option}>
                    <input
                      type="radio"
                      name="interface-size"
                      value={option.value}
                      checked={interfaceSize === option.value}
                      onChange={() => onInterfaceSizeChange(option.value)}
                    />
                    <span>
                      <strong>{option.label}</strong>
                      <small>{option.description}</small>
                    </span>
                  </label>
                ))}
              </div>
            </div>
          </fieldset>
          <fieldset className={`${styles.section} ${styles.downloads}`}>
            <legend>Downloads</legend>
            {!downloadsManageable ? (
              <p className={styles.storageNote}>
                Download folders are managed by the live application.
              </p>
            ) : (
              <>
                <div className={styles.settingHeading}>
                  <strong>Download folders</strong>
                  <span>
                    The default applies to future torrents only. Existing
                    torrents stay attached to their selected folder.
                  </span>
                </div>
                <div className={styles.rootList}>
                  {storage.roots.length === 0 ? (
                    <p className={styles.storageNote}>
                      No download folder has been chosen yet.
                    </p>
                  ) : (
                    storage.roots.map((root) => (
                      <article
                        key={root.id}
                        className={styles.root}
                        data-availability={root.availability}
                      >
                        <div>
                          <strong>{root.label}</strong>
                          <span>{root.path ?? "Location is unavailable"}</span>
                          <small>
                            {root.availability === "available"
                              ? root.id === storage.defaultRoot
                                ? "Default download folder"
                                : "Available"
                              : "Unavailable — repair required"}
                          </small>
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
                              Make default
                            </button>
                          ) : null}
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
                        </div>
                      </article>
                    ))
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
                {storageStatus === "" ? null : (
                  <output className={styles.storageStatus} aria-live="polite">
                    {storageStatus}
                  </output>
                )}
              </>
            )}
          </fieldset>
        </div>
      </section>
    </div>
  );
}
