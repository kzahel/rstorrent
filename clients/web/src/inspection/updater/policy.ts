import type { DesktopBundleType } from "./types";

export interface InstallPolicy {
  readonly canInstallInApp: boolean;
  readonly packageLabel: string;
}

export function installPolicy(bundleType: DesktopBundleType): InstallPolicy {
  switch (bundleType) {
    case "app":
      return { canInstallInApp: true, packageLabel: "macOS app" };
    case "nsis":
      return { canInstallInApp: true, packageLabel: "Windows NSIS installer" };
    case "appimage":
      return { canInstallInApp: true, packageLabel: "Linux AppImage" };
    case "msi":
      return { canInstallInApp: false, packageLabel: "Windows MSI" };
    case "deb":
      return { canInstallInApp: false, packageLabel: "Linux DEB package" };
    case "rpm":
      return { canInstallInApp: false, packageLabel: "Linux RPM package" };
    case "unknown":
      return { canInstallInApp: false, packageLabel: "development build" };
  }
}
