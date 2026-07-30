#!/usr/bin/env node

const [debuggerUrl, timeoutText] = process.argv.slice(2);
if (debuggerUrl === undefined || timeoutText === undefined) {
  throw new Error("usage: chrome_wait.mjs DEBUGGER_URL TIMEOUT_MILLIS");
}
const timeoutMillis = Number(timeoutText);
const socket = new WebSocket(debuggerUrl);
const pending = new Map();
let nextId = 1;

socket.addEventListener("message", (event) => {
  const message = JSON.parse(String(event.data));
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
const deadline = Date.now() + timeoutMillis;
let result;
while (Date.now() < deadline) {
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
        html: document.documentElement.outerHTML,
      };
    })()`,
    returnByValue: true,
  });
  if (
    result.result?.value?.complete === "true" &&
    result.result?.value?.control === "resumed"
  ) {
    process.stdout.write(JSON.stringify(result.result.value));
    socket.close();
    process.exit(0);
  }
  await new Promise((resolve) => setTimeout(resolve, 25));
}
const diagnostic = await command("Runtime.evaluate", {
  expression: "document.documentElement.outerHTML",
  returnByValue: true,
});
socket.close();
throw new Error(
  `browser surface did not complete: ${diagnostic.result?.value ?? "(no DOM)"}`,
);
