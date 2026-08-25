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

test("rejects updater restart that bypasses joined native shutdown", () => {
  const permissionFixture = repositoryFixture();
  permissionFixture.capability.permissions.push("process:default");
  assert.throws(
    () => validateDesktopReleaseConfiguration(permissionFixture),
    /raw process restart permission must stay disabled/,
  );

  const webFixture = repositoryFixture();
  webFixture.tauriUpdater = webFixture.tauriUpdater.replace(
    'relaunch: () => invoke("application_restart")',
    "relaunch: rawProcessRestart",
  );
  assert.throws(
    () => validateDesktopReleaseConfiguration(webFixture),
    /web updater does not use joined native restart/,
  );
});

test("rejects a release desktop binary with a Windows console", () => {
  const fixture = repositoryFixture();
  fixture.desktopMain = "fn main() {}";
  assert.throws(
    () => validateDesktopReleaseConfiguration(fixture),
    /release desktop binary must use the Windows GUI subsystem/,
  );
});

test("rejects desktop association and command-quoting drift", () => {
  const associationFixture = repositoryFixture();
  associationFixture.tauri.bundle.fileAssociations[0].name = "torrent";
  assert.throws(
    () => validateDesktopReleaseConfiguration(associationFixture),
    /unexpected desktop torrent file association/,
  );

  const schemeFixture = repositoryFixture();
  schemeFixture.tauri.plugins["deep-link"].desktop.schemes.push("jstorrent");
  assert.throws(
    () => validateDesktopReleaseConfiguration(schemeFixture),
    /unexpected desktop deep-link schemes/,
  );

  const quotingFixture = repositoryFixture();
  quotingFixture.nsisHooks = quotingFixture.nsisHooks.replace(
    '$\\"$INSTDIR\\${MAINBINARYNAME}.exe$\\"',
    "$INSTDIR\\${MAINBINARYNAME}.exe",
  );
  assert.throws(
    () => validateDesktopReleaseConfiguration(quotingFixture),
    /must quote the executable and input path/,
  );

  const linuxFixture = repositoryFixture();
  linuxFixture.linuxDesktop = linuxFixture.linuxDesktop.replace(" %U", "");
  assert.throws(
    () => validateDesktopReleaseConfiguration(linuxFixture),
    /Linux desktop handler must forward URLs\/files/,
  );
});

test("rejects incompatible plugin registration", () => {
  const cargoFixture = repositoryFixture();
  cargoFixture.cargo = cargoFixture.cargo.replace(', features = ["deep-link"]', "");
  assert.throws(
    () => validateDesktopReleaseConfiguration(cargoFixture),
    /single-instance dependency must use its compatible deep-link integration/,
  );

  const orderFixture = repositoryFixture();
  orderFixture.desktopSource = orderFixture.desktopSource.replace(
    ".plugin(tauri_plugin_single_instance::init(",
    ".plugin(tauri_plugin_deep_link::init())\n        .plugin(tauri_plugin_single_instance::init(",
  );
  assert.throws(
    () => validateDesktopReleaseConfiguration(orderFixture),
    /single-instance must be registered before the deep-link plugin/,
  );
});

test("rejects notification authority or dependency drift", () => {
  const dependencyFixture = repositoryFixture();
  dependencyFixture.cargo = dependencyFixture.cargo.replace(
    'tauri-plugin-notification = "=2.3.3"',
    'tauri-plugin-notification = "2"',
  );
  assert.throws(
    () => validateDesktopReleaseConfiguration(dependencyFixture),
    /notification dependency must stay pinned/,
  );

  const linuxDependencyFixture = repositoryFixture();
  linuxDependencyFixture.cargo = linuxDependencyFixture.cargo.replace(
    'notify-rust = "=4.18.0"',
    'notify-rust = "4"',
  );
  assert.throws(
    () => validateDesktopReleaseConfiguration(linuxDependencyFixture),
    /Linux notification dependency must stay pinned/,
  );

  const permissionFixture = repositoryFixture();
  permissionFixture.capability.permissions.push("notification:default");
  assert.throws(
    () => validateDesktopReleaseConfiguration(permissionFixture),
    /webview notification permissions must stay disabled/,
  );

  const ownerFixture = repositoryFixture();
  ownerFixture.desktopSource = ownerFixture.desktopSource.replace(
    "async fn run_notification_owner(",
    "async fn missing_notification_owner(",
  );
  assert.throws(
    () => validateDesktopReleaseConfiguration(ownerFixture),
    /Rust-owned desktop notification integration is incomplete/,
  );
});

function repositoryFixture() {
  return {
    packageJson: readJson("clients/web/package.json"),
    tauri: readJson("clients/desktop/src-tauri/tauri.conf.json"),
    developmentTauri: readJson("clients/desktop/src-tauri/tauri.dev.conf.json"),
    cargo: fs.readFileSync(path.join(root, "clients/desktop/src-tauri/Cargo.toml"), "utf8"),
    capability: readJson("clients/desktop/src-tauri/capabilities/default.json"),
    desktopMain: fs.readFileSync(
      path.join(root, "clients/desktop/src-tauri/src/main.rs"),
      "utf8",
    ),
    desktopSource: fs.readFileSync(
      path.join(root, "clients/desktop/src-tauri/src/lib.rs"),
      "utf8",
    ),
    nsisHooks: fs.readFileSync(
      path.join(root, "clients/desktop/src-tauri/nsis/hooks.nsh"),
      "utf8",
    ),
    linuxDesktop: fs.readFileSync(
      path.join(root, "clients/desktop/src-tauri/linux/rstorrent.desktop"),
      "utf8",
    ),
    tauriUpdater: fs.readFileSync(
      path.join(root, "clients/web/src/tauri-updater.ts"),
      "utf8",
    ),
    product: readJson("update-server/rstorrent.json"),
    changelog: fs.readFileSync(path.join(root, "CHANGELOG.md"), "utf8"),
    tag: undefined,
  };
}

function readJson(relativePath) {
  return JSON.parse(fs.readFileSync(path.join(root, relativePath), "utf8"));
}
