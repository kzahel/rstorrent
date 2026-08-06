import {
  useEffect,
  useRef,
  type KeyboardEvent,
  type MouseEvent,
  type RefObject,
} from "react";

import type { ClientSettings, ClientSettingsRuntimeView } from "../../api";
import type { ColorTheme, DataUnits, InterfaceSize } from "../appearance";
import type { DownloadRoot, DownloadStorageSettings } from "../model";
import { AppearanceSettingsSection } from "./AppearanceSettingsSection";
import { ConnectionSeedingSettingsSection } from "./ConnectionSeedingSettingsSection";
import { DownloadSettingsSection } from "./DownloadSettingsSection";
import { Icon } from "./Icon";
import styles from "./SettingsDialog.module.css";

export interface SettingsDialogProps {
  readonly colorTheme: ColorTheme;
  readonly interfaceSize: InterfaceSize;
  readonly dataUnits: DataUnits;
  readonly storage: DownloadStorageSettings;
  readonly clientSettings: ClientSettingsRuntimeView;
  readonly downloadsManageable: boolean;
  readonly clientSettingsManageable: boolean;
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
  readonly onClientSettingsSave: (settings: ClientSettings) => Promise<void>;
  readonly onClose: () => void;
}

export function SettingsDialog({
  colorTheme,
  interfaceSize,
  dataUnits,
  storage,
  clientSettings,
  downloadsManageable,
  clientSettingsManageable,
  returnFocus,
  onColorThemeChange,
  onInterfaceSizeChange,
  onDataUnitsChange,
  onChooseFolder,
  onDefaultRootChange,
  onShowAddOptionsChange,
  onRemoveRoot,
  onClientSettingsSave,
  onClose,
}: SettingsDialogProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

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
      'button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex]:not([tabindex="-1"])',
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
          <AppearanceSettingsSection
            colorTheme={colorTheme}
            interfaceSize={interfaceSize}
            dataUnits={dataUnits}
            onColorThemeChange={onColorThemeChange}
            onInterfaceSizeChange={onInterfaceSizeChange}
            onDataUnitsChange={onDataUnitsChange}
          />
          <DownloadSettingsSection
            storage={storage}
            manageable={downloadsManageable}
            onChooseFolder={onChooseFolder}
            onDefaultRootChange={onDefaultRootChange}
            onShowAddOptionsChange={onShowAddOptionsChange}
            onRemoveRoot={onRemoveRoot}
          />
          <ConnectionSeedingSettingsSection
            settings={clientSettings}
            manageable={clientSettingsManageable}
            onSave={onClientSettingsSave}
          />
        </div>
      </section>
    </div>
  );
}
