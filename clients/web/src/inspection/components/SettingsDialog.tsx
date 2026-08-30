import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type RefObject,
} from "react";

import type { ClientSettingsPatch, ClientSettingsRuntimeView } from "../../api";
import type { ColorTheme, DataUnits, InterfaceSize } from "../appearance";
import type { CommandResult, DownloadRoot, DownloadStorageSettings } from "../model";
import { AppearanceSettingsSection } from "./AppearanceSettingsSection";
import { AboutUpdatesSettingsSection } from "./AboutUpdatesSettingsSection";
import { ConnectionSeedingSettingsSection } from "./ConnectionSeedingSettingsSection";
import { DownloadSettingsSection } from "./DownloadSettingsSection";
import { Icon } from "./Icon";
import { NotificationsSettingsSection } from "./NotificationsSettingsSection";
import { PowerSettingsSection } from "./PowerSettingsSection";
import { RemoteAccessSettingsSection } from "./RemoteAccessSettingsSection";
import { WebAccessSettingsSection } from "./WebAccessSettingsSection";
import styles from "./SettingsDialog.module.css";
import type { WebAuthClient } from "../../web-auth-client";
import type { DesktopUpdater, DesktopUpdaterSnapshot } from "../updater/types";
import type { DesktopNotifications } from "../desktop-notifications/types";
import type { DesktopPower } from "../desktop-power/types";
import type { DesktopRemoteAccess } from "../remote-access/types";

export type SettingsCategory =
  | "appearance"
  | "downloads"
  | "connection"
  | "notifications"
  | "power"
  | "remote-access"
  | "web-access"
  | "updates";

export interface SettingsDialogProps {
  readonly colorTheme: ColorTheme;
  readonly interfaceSize: InterfaceSize;
  readonly dataUnits: DataUnits;
  readonly storage: DownloadStorageSettings;
  readonly clientSettings: ClientSettingsRuntimeView;
  readonly downloadsManageable: boolean;
  readonly showCrostiniStorageHelp: boolean;
  readonly oneCurrentRoot?: boolean;
  readonly clientSettingsManageable: boolean;
  readonly notifications?: DesktopNotifications | undefined;
  readonly power?: DesktopPower | undefined;
  readonly remoteAccess?: DesktopRemoteAccess | undefined;
  readonly webAuth?: WebAuthClient | undefined;
  readonly updater?: DesktopUpdater | undefined;
  readonly updaterSnapshot?: DesktopUpdaterSnapshot | undefined;
  readonly initialCategory?: SettingsCategory;
  readonly returnFocus: RefObject<HTMLButtonElement | null>;
  readonly onColorThemeChange: (colorTheme: ColorTheme) => void;
  readonly onInterfaceSizeChange: (interfaceSize: InterfaceSize) => void;
  readonly onDataUnitsChange: (dataUnits: DataUnits) => void;
  readonly onChooseFolder: (
    repairRoot?: string,
  ) => Promise<DownloadRoot | null>;
  readonly onDefaultRootChange: (rootId: string) => Promise<void>;
  readonly onShowAddOptionsChange: (show: boolean) => Promise<void>;
  readonly onRemoveRoot: (rootId: string) => Promise<void>;
  readonly onClientSettingsSave: (patch: ClientSettingsPatch) => Promise<CommandResult>;
  readonly onWebAuthSignedOut: () => void;
  readonly onClose: () => void;
}

export function SettingsDialog({
  colorTheme,
  interfaceSize,
  dataUnits,
  storage,
  clientSettings,
  downloadsManageable,
  showCrostiniStorageHelp,
  oneCurrentRoot = false,
  clientSettingsManageable,
  notifications,
  power,
  remoteAccess,
  webAuth,
  updater,
  updaterSnapshot,
  initialCategory = "appearance",
  returnFocus,
  onColorThemeChange,
  onInterfaceSizeChange,
  onDataUnitsChange,
  onChooseFolder,
  onDefaultRootChange,
  onShowAddOptionsChange,
  onRemoveRoot,
  onClientSettingsSave,
  onWebAuthSignedOut,
  onClose,
}: SettingsDialogProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const [category, setCategory] = useState<SettingsCategory>(initialCategory);
  const categories: readonly {
    readonly id: SettingsCategory;
    readonly label: string;
  }[] = [
    { id: "appearance", label: "Appearance" },
    { id: "downloads", label: "Downloads" },
    { id: "connection", label: "Connection & seeding" },
    ...(notifications === undefined
      ? []
      : [{ id: "notifications" as const, label: "Notifications" }]),
    ...(power === undefined ? [] : [{ id: "power" as const, label: "Power" }]),
    ...(remoteAccess === undefined
      ? []
      : [{ id: "remote-access" as const, label: "Remote access" }]),
    ...(webAuth === undefined
      ? []
      : [{ id: "web-access" as const, label: "Web access" }]),
    ...(updater === undefined || updaterSnapshot === undefined
      ? []
      : [{ id: "updates" as const, label: "About & updates" }]),
  ];

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
    const focusable = [
      ...(dialogRef.current?.querySelectorAll<HTMLElement>(
        'button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex]:not([tabindex="-1"])',
      ) ?? []),
    ].filter((element) => element.closest("[hidden]") === null);
    if (focusable.length === 0) return;
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

  const moveCategory = (
    event: KeyboardEvent<HTMLButtonElement>,
    active: SettingsCategory,
  ) => {
    const current = categories.findIndex(
      (candidate) => candidate.id === active,
    );
    let next = current;
    if (event.key === "ArrowDown" || event.key === "ArrowRight")
      next = current + 1;
    else if (event.key === "ArrowUp" || event.key === "ArrowLeft")
      next = current - 1;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = categories.length - 1;
    else return;
    event.preventDefault();
    const selected = categories[(next + categories.length) % categories.length];
    if (selected === undefined) return;
    setCategory(selected.id);
    requestAnimationFrame(() =>
      dialogRef.current
        ?.querySelector<HTMLButtonElement>(`#settings-tab-${selected.id}`)
        ?.focus(),
    );
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
        <div className={styles.workspace}>
          <div
            className={styles.categories}
            role="tablist"
            aria-label="Settings categories"
            aria-orientation="vertical"
          >
            {categories.map((item) => (
              <button
                id={`settings-tab-${item.id}`}
                key={item.id}
                type="button"
                role="tab"
                aria-selected={category === item.id}
                aria-controls={`settings-panel-${item.id}`}
                tabIndex={category === item.id ? 0 : -1}
                onClick={() => setCategory(item.id)}
                onKeyDown={(event) => moveCategory(event, item.id)}
              >
                {item.label}
              </button>
            ))}
          </div>
          <div className={styles.content}>
            <div
              id="settings-panel-appearance"
              role="tabpanel"
              aria-labelledby="settings-tab-appearance"
              hidden={category !== "appearance"}
            >
              <AppearanceSettingsSection
                colorTheme={colorTheme}
                interfaceSize={interfaceSize}
                dataUnits={dataUnits}
                onColorThemeChange={onColorThemeChange}
                onInterfaceSizeChange={onInterfaceSizeChange}
                onDataUnitsChange={onDataUnitsChange}
              />
            </div>
            <div
              id="settings-panel-downloads"
              role="tabpanel"
              aria-labelledby="settings-tab-downloads"
              hidden={category !== "downloads"}
            >
              <DownloadSettingsSection
                storage={storage}
                manageable={downloadsManageable}
                showCrostiniStorageHelp={showCrostiniStorageHelp}
                oneCurrentRoot={oneCurrentRoot}
                onChooseFolder={onChooseFolder}
                onDefaultRootChange={onDefaultRootChange}
                onShowAddOptionsChange={onShowAddOptionsChange}
                onRemoveRoot={onRemoveRoot}
              />
            </div>
            <div
              id="settings-panel-connection"
              role="tabpanel"
              aria-labelledby="settings-tab-connection"
              hidden={category !== "connection"}
            >
              <ConnectionSeedingSettingsSection
                settings={clientSettings}
                manageable={clientSettingsManageable}
                onSave={onClientSettingsSave}
              />
            </div>
            {notifications === undefined ? null : (
              <div
                id="settings-panel-notifications"
                role="tabpanel"
                aria-labelledby="settings-tab-notifications"
                hidden={category !== "notifications"}
              >
                <NotificationsSettingsSection notifications={notifications} />
              </div>
            )}
            {power === undefined ? null : (
              <div
                id="settings-panel-power"
                role="tabpanel"
                aria-labelledby="settings-tab-power"
                hidden={category !== "power"}
              >
                <PowerSettingsSection power={power} />
              </div>
            )}
            {remoteAccess === undefined ? null : (
              <div
                id="settings-panel-remote-access"
                role="tabpanel"
                aria-labelledby="settings-tab-remote-access"
                hidden={category !== "remote-access"}
              >
                <RemoteAccessSettingsSection remoteAccess={remoteAccess} />
              </div>
            )}
            {webAuth === undefined ? null : (
              <div
                id="settings-panel-web-access"
                role="tabpanel"
                aria-labelledby="settings-tab-web-access"
                hidden={category !== "web-access"}
              >
                <WebAccessSettingsSection
                  client={webAuth}
                  onSignedOut={onWebAuthSignedOut}
                />
              </div>
            )}
            {updater === undefined || updaterSnapshot === undefined ? null : (
              <div
                id="settings-panel-updates"
                role="tabpanel"
                aria-labelledby="settings-tab-updates"
                hidden={category !== "updates"}
              >
                <AboutUpdatesSettingsSection
                  updater={updater}
                  snapshot={updaterSnapshot}
                />
              </div>
            )}
          </div>
        </div>
      </section>
    </div>
  );
}
