import assert from "node:assert/strict";
import test from "node:test";

let listener;
let nativeResponse;
let nativeRequest;

globalThis.chrome = {
  runtime: {
    lastError: undefined,
    onMessage: {
      addListener(callback) {
        listener = callback;
      },
    },
    sendNativeMessage(host, request, callback) {
      nativeRequest = { host, request };
      callback(nativeResponse(request));
    },
  },
};

await import("../src/service-worker.js");

function send(message) {
  return new Promise((resolve) => {
    const keepAlive = listener(message, {}, resolve);
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

  const response = await send({ type: "nativeBootstrap", op: "hello" });
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
  nativeResponse = () => undefined;
  const response = await send({ type: "nativeBootstrap", op: "launch" });
  chrome.runtime.lastError = undefined;

  assert.equal(response.ok, false);
  assert.equal(response.error.code, "native_host_unavailable");
  assert.match(response.error.message, /Install it and open it once/u);
  assert.doesNotMatch(response.error.message, /private/u);
});

test("unrecognized messages are not claimed", () => {
  assert.equal(listener({ type: "other", op: "hello" }, {}, () => {}), false);
  assert.equal(listener({ type: "nativeBootstrap", op: "download" }, {}, () => {}), false);
});
