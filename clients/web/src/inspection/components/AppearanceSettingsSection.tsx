import type { ColorTheme, InterfaceSize } from "../appearance";
import styles from "./SettingsDialog.module.css";

const COLOR_THEME_OPTIONS: readonly {
  readonly value: ColorTheme;
  readonly label: string;
  readonly description: string;
}[] = [
  { value: "auto", label: "Auto", description: "Follow your system appearance." },
  { value: "light", label: "Light", description: "Always use the light appearance." },
  { value: "dark", label: "Dark", description: "Always use the dark appearance." },
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

interface AppearanceSettingsSectionProps {
  readonly colorTheme: ColorTheme;
  readonly interfaceSize: InterfaceSize;
  readonly onColorThemeChange: (colorTheme: ColorTheme) => void;
  readonly onInterfaceSizeChange: (interfaceSize: InterfaceSize) => void;
}

export function AppearanceSettingsSection({
  colorTheme,
  interfaceSize,
  onColorThemeChange,
  onInterfaceSizeChange,
}: AppearanceSettingsSectionProps) {
  return (
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
  );
}
