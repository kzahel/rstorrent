import { applyPresentation, presentationForPlatform } from "./platform.js";

const desktopSurface = document.querySelector("#desktop-surface");
const chromeosSurface = document.querySelector("#chromeos-surface");
const statusDot = document.querySelector("#status-dot");
const statusTitle = document.querySelector("#status-title");
const statusDetail = document.querySelector("#status-detail");
const launchButton = document.querySelector("#launch");
const linuxButton = document.querySelector("#launch-linux");
const linuxHelpButton = document.querySelector("#linux-help");
const linuxStatus = document.querySelector("#linux-status");

function setStatus(kind, title, detail) {
  statusDot.className = `status-dot ${kind}`;
  statusTitle.textContent = title;
  statusDetail.textContent = detail;
}

function request(op) {
  return chrome.runtime.sendMessage({ type: "nativeBootstrap", op });
}

async function checkDesktop() {
  setStatus("checking", "Checking desktop setup…", "Looking for the RSTorrent native host.");
  launchButton.disabled = true;
  try {
    const response = await request("hello");
    if (!response?.ok || response.result?.kind !== "hello") {
      throw new Error(response?.error?.message || "RSTorrent Desktop is unavailable.");
    }
    setStatus(
      "ready",
      "RSTorrent Desktop is ready",
      `Native bootstrap ${response.result.hostVersion} is installed.`,
    );
    launchButton.disabled = false;
  } catch (error) {
    setStatus(
      "error",
      "Desktop setup needed",
      error instanceof Error
        ? error.message
        : "Install RSTorrent Desktop and open it once to finish setup.",
    );
  }
}

launchButton.addEventListener("click", async () => {
  launchButton.disabled = true;
  setStatus("checking", "Opening RSTorrent…", "Sending a launch request to the desktop app.");
  try {
    const response = await request("launch");
    if (!response?.ok || response.result?.status !== "requested") {
      throw new Error(response?.error?.message || "RSTorrent could not be opened.");
    }
    setStatus(
      "ready",
      "Launch requested",
      "The desktop app should open or focus shortly.",
    );
  } catch (error) {
    setStatus(
      "error",
      "RSTorrent did not open",
      error instanceof Error ? error.message : "Open RSTorrent Desktop directly and try again.",
    );
    launchButton.disabled = false;
  }
});

linuxButton.addEventListener("click", async () => {
  linuxButton.disabled = true;
  linuxStatus.textContent = "Opening the ChromeOS Linux UI…";
  try {
    const response = await requestCrostini();
    if (!response?.ok) {
      throw new Error(response?.error?.message || "Chrome could not open RSTorrent Linux.");
    }
    linuxStatus.textContent =
      "Opened. If Chrome shows that the page is unavailable, launch RSTorrent for ChromeOS Linux from the Chromebook Launcher.";
  } catch (error) {
    linuxStatus.textContent =
      error instanceof Error
        ? error.message
        : "Use RSTorrent for ChromeOS Linux from the Chromebook Launcher.";
  } finally {
    linuxButton.disabled = false;
  }
});

linuxHelpButton.addEventListener("click", () => {
  chrome.tabs.create({ url: chrome.runtime.getURL("crostini/setup.html") });
});

function requestCrostini() {
  return chrome.runtime.sendMessage({ type: "crostiniBootstrap", op: "open" });
}

async function initializePresentation() {
  let presentation;
  try {
    const platform = await chrome.runtime.getPlatformInfo();
    presentation = presentationForPlatform(platform?.os);
  } catch {
    presentation = presentationForPlatform(undefined);
  }

  applyPresentation(presentation, {
    desktop: desktopSurface,
    chromeos: chromeosSurface,
  });
  if (presentation.desktop) {
    await checkDesktop();
  }
}

initializePresentation();
