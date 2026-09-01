import { ProductMetricsOwner } from "./product-metrics.js";

const NATIVE_HOST = "com.jstorrent.rstorrent.native";
const PROTOCOL_VERSION = 1;
const CROSTINI_ORIGIN = "http://penguin.linux.test:3030";
const CROSTINI_ROOT = `${CROSTINI_ORIGIN}/`;
const CROSTINI_LAUNCH_URL = `${CROSTINI_ORIGIN}/launch-chromeos`;
const CROSTINI_TAB_KEY = "crostiniUiTabId";
const ANDROID_LAUNCH_URL = "rstorrent://chromeos-companion";
const ANDROID_PAGE = "companion/companion.html";
const ANDROID_TAB_KEY = "androidUiTabId";
const productMetrics = new ProductMetricsOwner(chrome, chrome.runtime.getManifest().version);
productMetrics.start();

function sendNativeOperation(op) {
  const request = {
    id: crypto.randomUUID(),
    protocolVersion: PROTOCOL_VERSION,
    op,
  };

  return new Promise((resolve) => {
    chrome.runtime.sendNativeMessage(NATIVE_HOST, request, (response) => {
      const runtimeError = chrome.runtime.lastError;
      if (runtimeError) {
        resolve({
          ok: false,
          error: {
            code: "native_host_unavailable",
            message:
              "RSTorrent Desktop is unavailable. Install it and open it once to finish setup.",
          },
        });
        return;
      }
      if (!response || response.id !== request.id) {
        resolve({
          ok: false,
          error: {
            code: "invalid_native_response",
            message: "RSTorrent Desktop returned an invalid bootstrap response.",
          },
        });
        return;
      }
      resolve(response);
    });
  });
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type === "productMetrics") {
    const operation = productMetricsOperation(message);
    if (operation === null) return false;
    operation.then(
      (state) => sendResponse({ ok: true, state }),
      (error) => sendResponse({ ok: false, error: String(error).slice(0, 256) }),
    );
    return true;
  }
  if (message?.type === "nativeBootstrap" && ["hello", "launch"].includes(message.op)) {
    sendNativeOperation(message.op).then(sendResponse);
    return true;
  }
  if (message?.type === "crostiniBootstrap" && message.op === "open") {
    focusOrOpenCrostiniTab().then(sendResponse);
    return true;
  }
  if (message?.type === "androidBootstrap" && message.op === "open") {
    launchAndOpenAndroid().then(sendResponse);
    return true;
  }
  return false;
});

function productMetricsOperation(message) {
  switch (message.op) {
    case "get": return productMetrics.snapshot();
    case "session": return productMetrics.recordSession();
    case "connected": return productMetrics.recordConnected();
    case "acknowledge": return productMetrics.acknowledge(message.enabled === true);
    case "setEnabled": return productMetrics.setStatisticsEnabled(message.enabled === true);
    case "reset": return productMetrics.reset();
    default: return null;
  }
}

chrome.runtime.onMessageExternal.addListener((message, sender, sendResponse) => {
  if (!validCrostiniLaunchMessage(message, sender)) {
    return false;
  }
  focusOrOpenCrostiniTab(sender.tab.id).then(sendResponse);
  return true;
});

chrome.tabs.onRemoved.addListener((tabId) => {
  readRememberedCrostiniTab().then((remembered) => {
    if (remembered === tabId) {
      chrome.storage.session.remove(CROSTINI_TAB_KEY);
    }
  });
  readRememberedTab(ANDROID_TAB_KEY).then((remembered) => {
    if (remembered === tabId) {
      chrome.storage.session.remove(ANDROID_TAB_KEY);
    }
  });
});

async function launchAndOpenAndroid() {
  let launchRequested = false;
  try {
    await chrome.tabs.create({ url: ANDROID_LAUNCH_URL, active: true });
    launchRequested = true;
  } catch {
    // ChromeOS may reject or defer the custom-scheme launch. The visible
    // companion page keeps probing and explains how to retry.
  }
  const tab = await focusOrOpenExtensionTab(ANDROID_TAB_KEY, ANDROID_PAGE);
  return {
    ok: true,
    result: {
      kind: "android_ui",
      status: tab,
      launchRequested,
    },
  };
}

async function readRememberedTab(key) {
  const stored = await chrome.storage.session.get(key);
  return Number.isInteger(stored?.[key]) ? stored[key] : null;
}

async function focusOrOpenExtensionTab(key, relativeUrl) {
  const remembered = await readRememberedTab(key);
  if (remembered !== null) {
    try {
      await activateTab(remembered);
      return "focused";
    } catch {
      await chrome.storage.session.remove(key);
    }
  }
  const tab = await chrome.tabs.create({
    url: chrome.runtime.getURL(relativeUrl),
    active: true,
  });
  if (!Number.isInteger(tab.id)) {
    throw new Error("Chrome could not open the RSTorrent Android page");
  }
  await chrome.storage.session.set({ [key]: tab.id });
  return "opened";
}

function validCrostiniLaunchMessage(message, sender) {
  if (
    !message ||
    message.type !== "openCrostiniUi" ||
    message.protocolVersion !== PROTOCOL_VERSION ||
    !Number.isInteger(sender?.tab?.id)
  ) {
    return false;
  }
  const keys = Object.keys(message).sort();
  if (JSON.stringify(keys) !== JSON.stringify(["protocolVersion", "type"])) {
    return false;
  }
  try {
    const url = new URL(sender.url);
    return (
      url.href === CROSTINI_LAUNCH_URL &&
      url.protocol === "http:" &&
      url.hostname === "penguin.linux.test" &&
      url.port === "3030" &&
      url.username === "" &&
      url.password === ""
    );
  } catch {
    return false;
  }
}

async function readRememberedCrostiniTab() {
  const stored = await chrome.storage.session.get(CROSTINI_TAB_KEY);
  return Number.isInteger(stored?.[CROSTINI_TAB_KEY]) ? stored[CROSTINI_TAB_KEY] : null;
}

async function rememberCrostiniTab(tabId) {
  await chrome.storage.session.set({ [CROSTINI_TAB_KEY]: tabId });
}

async function activateTab(tabId) {
  const tab = await chrome.tabs.get(tabId);
  await chrome.tabs.update(tabId, { active: true });
  if (Number.isInteger(tab.windowId)) {
    await chrome.windows.update(tab.windowId, { focused: true });
  }
  return tab;
}

async function focusOrOpenCrostiniTab(handoffTabId = null) {
  const remembered = await readRememberedCrostiniTab();
  if (remembered !== null) {
    try {
      await activateTab(remembered);
      if (handoffTabId !== null && handoffTabId !== remembered) {
        await chrome.tabs.remove(handoffTabId);
      }
      return { ok: true, result: { kind: "crostini_ui", status: "focused" } };
    } catch {
      await chrome.storage.session.remove(CROSTINI_TAB_KEY);
    }
  }

  if (handoffTabId !== null) {
    try {
      const tab = await chrome.tabs.update(handoffTabId, {
        url: CROSTINI_ROOT,
        active: true,
      });
      if (Number.isInteger(tab.windowId)) {
        await chrome.windows.update(tab.windowId, { focused: true });
      }
      await rememberCrostiniTab(handoffTabId);
      return { ok: true, result: { kind: "crostini_ui", status: "opened" } };
    } catch {
      // The handoff tab may have closed while the worker was waking. Create one.
    }
  }

  const tab = await chrome.tabs.create({ url: CROSTINI_ROOT, active: true });
  if (!Number.isInteger(tab.id)) {
    return {
      ok: false,
      error: {
        code: "crostini_tab_unavailable",
        message: "Chrome could not open the RSTorrent Linux page.",
      },
    };
  }
  await rememberCrostiniTab(tab.id);
  return { ok: true, result: { kind: "crostini_ui", status: "opened" } };
}
