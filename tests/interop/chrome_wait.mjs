#!/usr/bin/env node

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

const [debuggerUrl, timeoutText, screenshotArgument, scenario = "transfer"] =
  process.argv.slice(2);
const screenshotPath =
  screenshotArgument === undefined || screenshotArgument === ""
    ? undefined
    : screenshotArgument;
if (debuggerUrl === undefined || timeoutText === undefined) {
  throw new Error(
    "usage: chrome_wait.mjs DEBUGGER_URL TIMEOUT_MILLIS [SCREENSHOT_PATH]",
  );
}
const timeoutMillis = Number(timeoutText);
const socket = new WebSocket(debuggerUrl);
const pending = new Map();
const browserExceptions = [];
let nextId = 1;
let pauseClicked = false;
let resumeClicked = false;
let profileClicked = false;
let categoryClicked = false;
const categoryToSelect = scenario === "blocked" ? "discovery" : "lifecycle";

socket.addEventListener("message", (event) => {
  const message = JSON.parse(String(event.data));
  if (message.method === "Runtime.exceptionThrown") {
    browserExceptions.push(
      message.params?.exceptionDetails?.text ?? "unknown browser exception",
    );
    return;
  }
  if (typeof message.id !== "number") return;
  const operation = pending.get(message.id);
  if (operation === undefined) return;
  pending.delete(message.id);
  if (message.error !== undefined) {
    operation.reject(new Error(JSON.stringify(message.error)));
  } else {
    operation.resolve(message.result);
  }
});

await new Promise((resolve, reject) => {
  socket.addEventListener("open", resolve, { once: true });
  socket.addEventListener(
    "error",
    () => reject(new Error("DevTools WebSocket failed")),
    { once: true },
  );
});

function command(method, params = {}) {
  const id = nextId;
  nextId += 1;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}

await command("Runtime.enable");
await command("Page.enable");
await command("Emulation.setDeviceMetricsOverride", {
  width: 1440,
  height: 1000,
  deviceScaleFactor: 1,
  mobile: false,
});

async function captureScreenshot(path) {
  if (path === undefined) return;
  const metrics = await command("Page.getLayoutMetrics");
  const content = metrics.cssContentSize ?? metrics.contentSize;
  const width = Math.max(1, Math.min(4096, Math.ceil(content?.width ?? 1440)));
  const height = Math.max(1, Math.min(16384, Math.ceil(content?.height ?? 1000)));
  const screenshot = await command("Page.captureScreenshot", {
    format: "png",
    fromSurface: true,
    captureBeyondViewport: true,
    clip: {
      x: 0,
      y: 0,
      width,
      height,
      scale: 1,
    },
  });
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, Buffer.from(screenshot.data, "base64"));
}

const deadline = Date.now() + timeoutMillis;
let result;
while (Date.now() < deadline) {
  if (!profileClicked) {
    const profile = await command("Runtime.evaluate", {
      expression: `(() => {
        const button = document.querySelector("button[data-profile=detailed]");
        if (!(button instanceof HTMLButtonElement)) return false;
        button.click();
        return true;
      })()`,
      returnByValue: true,
    });
    profileClicked = profile.result?.value === true;
  } else if (!categoryClicked) {
    const category = await command("Runtime.evaluate", {
      expression: `(() => {
        const button = document.querySelector("button[data-category=${categoryToSelect}]");
        if (!(button instanceof HTMLButtonElement)) return false;
        button.click();
        return true;
      })()`,
      returnByValue: true,
    });
    categoryClicked = category.result?.value === true;
  }
  const control = await command("Runtime.evaluate", {
    expression: `(() => {
      const button = document.querySelector("button[data-command]");
      const state = document.querySelector(".state-pill");
      if (!(button instanceof HTMLButtonElement)) return null;
      return {
        command: button.dataset.command,
        state: state?.textContent?.trim(),
      };
    })()`,
    returnByValue: true,
  });
  if (
    scenario === "transfer" &&
    !pauseClicked &&
    control.result?.value?.command === "pause" &&
    control.result?.value?.state === "downloading"
  ) {
    await command("Runtime.evaluate", {
      expression:
        'document.querySelector("button[data-command=pause]")?.click()',
    });
    pauseClicked = true;
  } else if (
    scenario === "transfer" &&
    pauseClicked &&
    !resumeClicked &&
    control.result?.value?.command === "resume" &&
    control.result?.value?.state === "paused"
  ) {
    await command("Runtime.evaluate", {
      expression:
        'document.querySelector("button[data-command=resume]")?.click()',
    });
    resumeClicked = true;
  }

  result = await command("Runtime.evaluate", {
    expression: `(() => {
      const element = document.querySelector("#interop-result");
      if (!(element instanceof HTMLOutputElement)) return null;
      return {
        complete: element.dataset.complete,
        requested: element.dataset.requested,
        received: element.dataset.received,
        stored: element.dataset.stored,
        control: element.dataset.control,
        progress: element.dataset.progress,
        reason: element.dataset.reason,
        diagnosticCodes: [
          ...document.querySelectorAll("[data-event-code]"),
        ].map((event) => event.getAttribute("data-event-code")),
        html: document.documentElement.outerHTML,
      };
    })()`,
    returnByValue: true,
  });
  if (
    result.result?.value?.complete === "true" &&
    (scenario === "blocked"
      ? result.result?.value?.control === "blocked"
      : result.result?.value?.control === "resumed") &&
    profileClicked &&
    categoryClicked
  ) {
    await captureScreenshot(screenshotPath);
    process.stdout.write(
      JSON.stringify({
        ...result.result.value,
        browserExceptions,
        pauseClicked,
        resumeClicked,
        profileClicked,
        categoryClicked,
        screenshot: screenshotPath,
      }),
    );
    socket.close();
    process.exit(0);
  }
  await new Promise((resolve) => setTimeout(resolve, 25));
}
const diagnostic = await command("Runtime.evaluate", {
  expression: "document.documentElement.outerHTML",
  returnByValue: true,
});
await captureScreenshot(screenshotPath);
socket.close();
throw new Error(
  `browser surface did not complete: ${diagnostic.result?.value ?? "(no DOM)"}\n` +
    `browser exceptions: ${JSON.stringify(browserExceptions)}`,
);
