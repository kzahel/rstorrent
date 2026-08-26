const DESKTOP_PLATFORMS = new Set(["linux", "mac", "openbsd", "win"]);

export function presentationForPlatform(os) {
  if (os === "cros") {
    return Object.freeze({ desktop: false, chromeos: true });
  }
  if (DESKTOP_PLATFORMS.has(os)) {
    return Object.freeze({ desktop: true, chromeos: false });
  }
  return Object.freeze({ desktop: true, chromeos: true });
}

export function applyPresentation(presentation, surfaces) {
  surfaces.desktop.hidden = !presentation.desktop;
  surfaces.chromeos.hidden = !presentation.chromeos;
}
