import assert from "node:assert/strict";
import test from "node:test";

import {
  HOSTED_PRODUCT_CONTEXT_READY,
  ProductMetricsOwner,
  UNINSTALL_BASE_URL,
  buildUninstallUrl,
} from "../src/product-metrics.js";

function fixture() {
  const local = {};
  const urls = [];
  const chrome = {
    runtime: {
      onInstalled: { addListener() {} },
      onStartup: { addListener() {} },
      async setUninstallURL(url) { urls.push(url); },
    },
    storage: {
      local: {
        async get(key) { return { [key]: local[key] }; },
        async set(values) { Object.assign(local, values); },
      },
      onChanged: { addListener() {} },
    },
  };
  return { owner: new ProductMetricsOwner(chrome, "0.4.0", () => 1_800_000_000_000), local, urls };
}

test("fresh disclosure is default-on locally but registers no optional context", async () => {
  const { owner, urls } = fixture();
  const state = await owner.refresh();
  assert.equal(state.statisticsEnabled, true);
  assert.equal(state.disclosureVersion, 0);
  assert.equal(urls.at(-1), `${UNINSTALL_BASE_URL}?v=0.4.0`);
  assert.equal(HOSTED_PRODUCT_CONTEXT_READY, false);

  await owner.acknowledge(true);
  assert.equal(urls.at(-1), `${UNINSTALL_BASE_URL}?v=0.4.0`);
  assert.doesNotMatch(urls.at(-1), /id=|sessions=|connected=/u);
});

test("serialized events saturate state and reset rotates only extension metrics", async () => {
  const { owner } = fixture();
  const initial = await owner.snapshot();
  const results = await Promise.all([
    owner.recordSession(),
    owner.recordSession(),
    owner.recordConnected(),
  ]);
  const updated = results.at(-1);
  assert.equal(updated.sessions, "2");
  assert.equal(updated.everConnected, true);
  const reset = await owner.reset();
  assert.notEqual(reset.installationId, initial.installationId);
  assert.equal(reset.sessions, "0");
  assert.equal(reset.everConnected, false);
});

test("malformed future state resets locally and first registers the bare survey", async () => {
  const { owner, local, urls } = fixture();
  local.productMetricsV1 = { schemaVersion: 99, secret: "must not survive" };
  const state = await owner.refresh();
  assert.equal(state.schemaVersion, 1);
  assert.equal(local.productMetricsV1.secret, undefined);
  assert.equal(urls.at(-1), UNINSTALL_BASE_URL);
});

test("URL construction rejects state shape drift and never admits arbitrary fields", () => {
  const state = {
    schemaVersion: 1,
    installationId: "87e66203-9849-44c5-a557-8e77c29e7587",
    createdAtMillis: "1800000000000",
    firstVersion: "0.4.0",
    currentVersion: "0.4.0",
    sessions: "2",
    everConnected: true,
    disclosureVersion: 1,
    statisticsEnabled: true,
    backendSummary: null,
  };
  assert.equal(buildUninstallUrl(state), `${UNINSTALL_BASE_URL}?v=0.4.0`);
  assert.throws(() => buildUninstallUrl({ ...state, torrent: "secret" }), /shape/u);
});
