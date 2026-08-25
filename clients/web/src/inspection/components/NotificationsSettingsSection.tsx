import { useState } from "react";

import type {
  DesktopNotifications,
  DesktopNotificationSettings,
} from "../desktop-notifications/types";
import styles from "./SettingsDialog.module.css";

export interface NotificationsSettingsSectionProps {
  readonly notifications: DesktopNotifications;
}

type SettingName = keyof DesktopNotificationSettings;

const OPTIONS: readonly {
  readonly setting: SettingName;
  readonly label: string;
  readonly description: string;
}[] = [
  {
    setting: "notify_download_complete",
    label: "Download complete",
    description: "Notify when a torrent finishes downloading and is published.",
  },
  {
    setting: "notify_needs_attention",
    label: "Download needs attention",
    description: "Notify when a torrent enters a fatal or repair-required state.",
  },
  {
    setting: "notify_while_focused",
    label: "Notify while RSTorrent is focused",
    description:
      "Keep showing notifications while the main window is the active window.",
  },
];

export function NotificationsSettingsSection({
  notifications,
}: NotificationsSettingsSectionProps) {
  const [settings, setSettings] = useState(notifications.getSnapshot);
  const [pending, setPending] = useState(false);
  const [status, setStatus] = useState<
    { readonly type: "success" | "error"; readonly message: string } | null
  >(null);

  const change = async (setting: SettingName, checked: boolean) => {
    if (pending) return;
    const previous = settings;
    const next = { ...settings, [setting]: checked };
    setSettings(next);
    setPending(true);
    setStatus(null);
    try {
      setSettings(await notifications.save(next));
      setStatus({ type: "success", message: "Notification settings saved." });
    } catch (error) {
      setSettings(previous);
      setStatus({
        type: "error",
        message: `Notification settings were not saved: ${errorMessage(error)}`,
      });
    } finally {
      setPending(false);
    }
  };

  return (
    <fieldset className={styles.section} disabled={pending}>
      <legend>Notifications</legend>
      <p className={styles.sectionIntroduction}>
        Native desktop notifications are edge-triggered and are not replayed
        after startup or a settings change.
      </p>
      {OPTIONS.map((option) => (
        <label className={styles.preference} key={option.setting}>
          <input
            type="checkbox"
            checked={settings[option.setting]}
            onChange={(event) =>
              void change(option.setting, event.currentTarget.checked)
            }
          />
          <span>
            <strong>{option.label}</strong>
            <small>{option.description}</small>
          </span>
        </label>
      ))}
      {status === null ? null : (
        <p
          className={
            status.type === "error" ? styles.errorStatus : styles.successStatus
          }
          role={status.type === "error" ? "alert" : "status"}
        >
          {status.message}
        </p>
      )}
    </fieldset>
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
