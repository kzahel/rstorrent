import type { DesktopBundleType } from "./types";

export interface InstallPolicy {
  readonly canCheck: boolean;
  readonly canInstallInApp: boolean;
  readonly packageLabel: string;
}

export function installPolicy(bundleType: DesktopBundleType): InstallPolicy {
  switch (bundleType) {
    case "app":
      return { canCheck: true, canInstallInApp: true, packageLabel: "macOS app" };
    case "nsis":
      return {
        canCheck: true,
        canInstallInApp: true,
        packageLabel: "Windows NSIS installer",
      };
    case "appimage":
      return {
        canCheck: true,
        canInstallInApp: true,
        packageLabel: "Linux AppImage",
      };
    case "msi":
      return { canCheck: false, canInstallInApp: false, packageLabel: "Windows MSI" };
    case "deb":
      return {
        canCheck: false,
        canInstallInApp: false,
        packageLabel: "Linux DEB package",
      };
    case "rpm":
      return {
        canCheck: false,
        canInstallInApp: false,
        packageLabel: "Linux RPM package",
      };
    case "headless":
      return {
        canCheck: true,
        canInstallInApp: false,
        packageLabel: "Linux headless service",
      };
    case "unknown":
      return {
        canCheck: false,
        canInstallInApp: false,
        packageLabel: "development build",
      };
  }
}
