export interface DesktopPowerSettings {
  readonly prevent_sleep_during_active_downloads: boolean;
}

export interface DesktopPower {
  readonly getSnapshot: () => DesktopPowerSettings;
  save(settings: DesktopPowerSettings): Promise<DesktopPowerSettings>;
}
