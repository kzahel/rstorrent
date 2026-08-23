import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

import {
  validateDesktopReleaseConfiguration,
  validateDesktopReleaseRepository,
} from "./validate-desktop-release.mjs";

const root = path.resolve(import.meta.dirname, "..");

test("accepts the checked-in desktop release configuration", () => {
  assert.equal(validateDesktopReleaseRepository(root).version, "0.1.1");
  assert.equal(
    validateDesktopReleaseRepository(root, "desktop-v0.1.1").version,
    "0.1.1",
  );
});

test("rejects identifier and version drift", () => {
  const fixture = repositoryFixture();
  fixture.tauri.identifier = "org.example.wrong";
  assert.throws(
    () => validateDesktopReleaseConfiguration(fixture),
    /unexpected desktop identifier/,
  );

  const versionFixture = repositoryFixture();
  versionFixture.packageJson.version = "9.9.9";
  assert.throws(
    () => validateDesktopReleaseConfiguration(versionFixture),
    /desktop version drift/,
  );
});

test("rejects updater route and public-key drift", () => {
  const endpointFixture = repositoryFixture();
  endpointFixture.tauri.plugins.updater.endpoints[0] = "https://example.test";
  assert.throws(
    () => validateDesktopReleaseConfiguration(endpointFixture),
    /unexpected updater endpoint/,
  );

  const keyFixture = repositoryFixture();
  keyFixture.tauri.plugins.updater.pubkey = "wrong";
  assert.throws(
    () => validateDesktopReleaseConfiguration(keyFixture),
    /unexpected RSTorrent updater public key/,
  );
});

function repositoryFixture() {
  return {
    packageJson: readJson("clients/web/package.json"),
    tauri: readJson("clients/desktop/src-tauri/tauri.conf.json"),
    developmentTauri: readJson("clients/desktop/src-tauri/tauri.dev.conf.json"),
    cargo: fs.readFileSync(path.join(root, "clients/desktop/src-tauri/Cargo.toml"), "utf8"),
    capability: readJson("clients/desktop/src-tauri/capabilities/default.json"),
    product: readJson("update-server/rstorrent.json"),
    changelog: fs.readFileSync(path.join(root, "CHANGELOG.md"), "utf8"),
    tag: undefined,
  };
}

function readJson(relativePath) {
  return JSON.parse(fs.readFileSync(path.join(root, relativePath), "utf8"));
}
