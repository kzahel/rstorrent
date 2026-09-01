import assert from "node:assert/strict";
import test, { beforeEach } from "node:test";

let internalListener;
let externalListener;
let removedListener;
let installedListener;
let startupListener;
let storageChangedListener;
let nativeResponse;
let nativeRequest;
let nextTabId = 100;
let focusedWindow;
let stored = {};
let tabs = new Map();
let uninstallUrl;

globalThis.chrome = {
  runtime: {
    lastError: undefined,
    onMessage: {
      addListener(callback) {
        internalListener = callback;
      },
    },
    onMessageExternal: {
      addListener(callback) {
        externalListener = callback;
      },
    },
    sendNativeMessage(host, request, callback) {
      nativeRequest = { host, request };
      callback(nativeResponse(request));
    },
    getURL(path) {
      return `chrome-extension://gcgoepclopkgijmclmlheafaglmbjlcc/${path}`;
    },
    getManifest() {
      return { version: "0.4.0" };
    },
    async setUninstallURL(url) {
      uninstallUrl = url;
    },
    onInstalled: {
      addListener(callback) {
        installedListener = callback;
      },
    },
    onStartup: {
      addListener(callback) {
        startupListener = callback;
      },
    },
  },
  storage: {
    session: {
      async get(key) {
        return { [key]: stored[key] };
      },
      async set(values) {
        Object.assign(stored, values);
      },
      async remove(key) {
        delete stored[key];
      },
    },
    local: {
      async get(key) {
        return { [key]: stored[key] };
      },
      async set(values) {
        Object.assign(stored, values);
      },
    },
    onChanged: {
      addListener(callback) {
        storageChangedListener = callback;
      },
    },
  },
  tabs: {
    onRemoved: {
      addListener(callback) {
        removedListener = callback;
      },
    },
    async get(tabId) {
      const tab = tabs.get(tabId);
      if (!tab) throw new Error("No tab");
      return { ...tab };
    },
    async update(tabId, update) {
      const tab = tabs.get(tabId);
      if (!tab) throw new Error("No tab");
      Object.assign(tab, update);
      return { ...tab };
    },
    async create(options) {
      const tab = { id: nextTabId, windowId: 9, ...options };
      nextTabId += 1;
      tabs.set(tab.id, tab);
      return { ...tab };
    },
    async remove(tabId) {
      tabs.delete(tabId);
      removedListener(tabId);
    },
  },
  windows: {
    async update(windowId, update) {
      focusedWindow = { windowId, update };
    },
  },
};

await import("../src/service-worker.js");

beforeEach(() => {
  chrome.runtime.lastError = undefined;
  nativeResponse = () => undefined;
  nativeRequest = undefined;
  nextTabId = 100;
  focusedWindow = undefined;
  stored = {};
  tabs = new Map();
  uninstallUrl = undefined;
});

function sendInternal(message) {
  return new Promise((resolve) => {
    const keepAlive = internalListener(message, {}, resolve);
    assert.equal(keepAlive, true);
  });
}

function sendExternal(message, sender) {
  return new Promise((resolve) => {
    const keepAlive = externalListener(message, sender, resolve);
    assert.equal(keepAlive, true);
  });
}

test("hello uses the distinct bounded native bootstrap contract", async () => {
  nativeResponse = (request) => ({
    id: request.id,
    ok: true,
    protocolVersion: 1,
    result: { kind: "hello" },
  });

  const response = await sendInternal({ type: "nativeBootstrap", op: "hello" });
  assert.equal(nativeRequest.host, "com.jstorrent.rstorrent.native");
  assert.deepEqual(
    {
      protocolVersion: nativeRequest.request.protocolVersion,
      op: nativeRequest.request.op,
    },
    { protocolVersion: 1, op: "hello" },
  );
  assert.equal(response.ok, true);
  assert.equal(response.id, nativeRequest.request.id);
});

test("runtime errors become actionable path-free setup guidance", async () => {
  chrome.runtime.lastError = { message: "/private/path should not escape" };
  const response = await sendInternal({ type: "nativeBootstrap", op: "launch" });

  assert.equal(response.ok, false);
  assert.equal(response.error.code, "native_host_unavailable");
  assert.match(response.error.message, /Install it and open it once/u);
  assert.doesNotMatch(response.error.message, /private/u);
});

test("exact external Crostini handoff reuses its sender tab", async () => {
  tabs.set(41, {
    id: 41,
    windowId: 7,
    url: "http://penguin.linux.test:3030/launch-chromeos",
  });
  const response = await sendExternal(
    { type: "openCrostiniUi", protocolVersion: 1 },
    {
      url: "http://penguin.linux.test:3030/launch-chromeos",
      tab: { id: 41 },
    },
  );

  assert.deepEqual(response, {
    ok: true,
    result: { kind: "crostini_ui", status: "opened" },
  });
  assert.equal(tabs.get(41).url, "http://penguin.linux.test:3030/");
  assert.equal(stored.crostiniUiTabId, 41);
  assert.deepEqual(focusedWindow, { windowId: 7, update: { focused: true } });
});

test("repeated handoff focuses one remembered tab and closes the disposable tab", async () => {
  stored.crostiniUiTabId = 41;
  tabs.set(41, { id: 41, windowId: 7, url: "http://penguin.linux.test:3030/" });
  tabs.set(42, {
    id: 42,
    windowId: 8,
    url: "http://penguin.linux.test:3030/launch-chromeos",
  });

  const response = await sendExternal(
    { type: "openCrostiniUi", protocolVersion: 1 },
    {
      url: "http://penguin.linux.test:3030/launch-chromeos",
      tab: { id: 42 },
    },
  );
  assert.equal(response.result.status, "focused");
  assert.equal(tabs.get(41).active, true);
  assert.equal(tabs.has(42), false);
  assert.deepEqual(focusedWindow, { windowId: 7, update: { focused: true } });
});

test("warm popup action creates the fixed UI tab without probing the backend", async () => {
  const response = await sendInternal({ type: "crostiniBootstrap", op: "open" });
  assert.equal(response.result.status, "opened");
  assert.equal(tabs.get(100).url, "http://penguin.linux.test:3030/");
  assert.equal(stored.crostiniUiTabId, 100);
});

test("Android action attempts the fixed launch and opens one packaged React tab", async () => {
  const response = await sendInternal({ type: "androidBootstrap", op: "open" });

  assert.deepEqual(response, {
    ok: true,
    result: {
      kind: "android_ui",
      status: "opened",
      launchRequested: true,
    },
  });
  assert.equal(tabs.get(100).url, "rstorrent://chromeos-companion");
  assert.equal(
    tabs.get(101).url,
    "chrome-extension://gcgoepclopkgijmclmlheafaglmbjlcc/companion/companion.html",
  );
  assert.equal(stored.androidUiTabId, 101);
});

test("repeated Android action focuses the one visible application tab", async () => {
  stored.androidUiTabId = 77;
  tabs.set(77, {
    id: 77,
    windowId: 5,
    url: "chrome-extension://gcgoepclopkgijmclmlheafaglmbjlcc/companion/companion.html",
  });

  const response = await sendInternal({ type: "androidBootstrap", op: "open" });

  assert.equal(response.result.status, "focused");
  assert.equal(tabs.get(77).active, true);
  assert.equal(stored.androidUiTabId, 77);
});

test("external messages fail closed for URL and shape drift", () => {
  for (const [message, url] of [
    [{ type: "openCrostiniUi", protocolVersion: 2 }, "http://penguin.linux.test:3030/launch-chromeos"],
    [{ type: "openCrostiniUi", protocolVersion: 1, url: "http://evil" }, "http://penguin.linux.test:3030/launch-chromeos"],
    [{ type: "openCrostiniUi", protocolVersion: 1 }, "https://penguin.linux.test:3030/launch-chromeos"],
    [{ type: "openCrostiniUi", protocolVersion: 1 }, "http://penguin.linux.test:3031/launch-chromeos"],
    [{ type: "openCrostiniUi", protocolVersion: 1 }, "http://penguin.linux.test:3030/not-launch"],
    [{ type: "openCrostiniUi", protocolVersion: 1 }, "http://penguin.linux.test.evil:3030/launch-chromeos"],
  ]) {
    assert.equal(externalListener(message, { url, tab: { id: 5 } }, () => {}), false, url);
  }
});

test("unrecognized internal messages are not claimed", () => {
  assert.equal(internalListener({ type: "other", op: "hello" }, {}, () => {}), false);
  assert.equal(
    internalListener({ type: "nativeBootstrap", op: "download" }, {}, () => {}),
    false,
  );
});

test("product metric messages serialize disclosure session and reset state", async () => {
  const first = await sendInternal({ type: "productMetrics", op: "session" });
  assert.equal(first.ok, true);
  assert.equal(first.state.sessions, "1");
  assert.equal(first.state.disclosureVersion, 0);
  assert.equal(uninstallUrl, "https://jstorrent.com/uninstall.html?v=0.4.0");

  const acknowledged = await sendInternal({
    type: "productMetrics",
    op: "acknowledge",
    enabled: false,
  });
  assert.equal(acknowledged.state.statisticsEnabled, false);
  const reset = await sendInternal({ type: "productMetrics", op: "reset" });
  assert.equal(reset.state.sessions, "0");
  assert.notEqual(reset.state.installationId, first.state.installationId);
});
