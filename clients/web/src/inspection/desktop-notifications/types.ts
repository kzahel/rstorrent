export interface DesktopNotificationSettings {
  readonly notify_download_complete: boolean;
  readonly notify_needs_attention: boolean;
  readonly notify_while_focused: boolean;
}

export interface DesktopNotifications {
  readonly getSnapshot: () => DesktopNotificationSettings;
  save(settings: DesktopNotificationSettings): Promise<DesktopNotificationSettings>;
}
