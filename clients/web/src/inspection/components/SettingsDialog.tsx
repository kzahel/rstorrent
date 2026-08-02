import {
  useEffect,
  useRef,
  type KeyboardEvent,
  type MouseEvent,
  type RefObject,
} from "react";

import type { ColorTheme, InterfaceSize } from "../appearance";
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
  readonly returnFocus: RefObject<HTMLButtonElement | null>;
  readonly onColorThemeChange: (colorTheme: ColorTheme) => void;
  readonly onInterfaceSizeChange: (interfaceSize: InterfaceSize) => void;
  readonly onClose: () => void;
}

export function SettingsDialog({
  colorTheme,
  interfaceSize,
  returnFocus,
  onColorThemeChange,
  onInterfaceSizeChange,
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
        </div>
      </section>
    </div>
  );
}
