#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const EXPECTED_IDENTIFIER = "com.jstorrent.rstorrent";
const EXPECTED_ENDPOINT =
  "https://updates.graehlarts.com/rstorrent/tauri/{{target}}/{{arch}}/{{current_version}}";
const EXPECTED_PUBLIC_KEY =
  "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDc4OEE3ODUxMzEzNjcwOTYKUldTV2NEWXhVWGlLZUpKK0trSG5XZ09qQ1ZPVFo2ZGV0MC9Cc001UWlGSCtvaE1iNDY0RmNRZkwK";

function fail(message) {
  throw new Error(message);
}

function cargoVersion(contents) {
  const packageSection = contents.match(/\[package\]([\s\S]*?)(?:\n\[|$)/)?.[1];
  const version = packageSection?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) fail("desktop Cargo.toml has no package version");
  return version;
}

export function validateDesktopReleaseConfiguration({
  packageJson,
  tauri,
  developmentTauri,
  cargo,
  capability,
  desktopSource,
  tauriUpdater,
  product,
  changelog,
  tag,
}) {
  const versions = {
    web: packageJson.version,
    tauri: tauri.version,
    cargo: cargoVersion(cargo),
  };
  if (new Set(Object.values(versions)).size !== 1) {
    fail(`desktop version drift: ${JSON.stringify(versions)}`);
  }
  const version = tauri.version;
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    fail(`desktop version is not stable semver: ${version}`);
  }
  if (tag !== undefined && tag !== `desktop-v${version}`) {
    fail(`desktop tag ${tag} does not match version ${version}`);
  }
  if (!changelog.includes(`## [${version}]`)) {
    fail(`CHANGELOG.md has no ${version} entry`);
  }
  if (tauri.productName !== "RSTorrent") {
    fail(`unexpected desktop product name: ${tauri.productName}`);
  }
  if (tauri.identifier !== EXPECTED_IDENTIFIER) {
    fail(`unexpected desktop identifier: ${tauri.identifier}`);
  }
  if (developmentTauri.identifier !== EXPECTED_IDENTIFIER) {
    fail(`development identifier drift: ${developmentTauri.identifier}`);
  }
  if (tauri.bundle?.createUpdaterArtifacts !== false) {
    fail("base config must leave updater artifacts disabled until release CI");
  }
  if (tauri.bundle?.windows?.nsis?.installMode !== "currentUser") {
    fail("Windows NSIS must use currentUser installation");
  }
  const endpoints = tauri.plugins?.updater?.endpoints;
  if (
    !Array.isArray(endpoints) ||
    endpoints.length !== 1 ||
    endpoints[0] !== EXPECTED_ENDPOINT
  ) {
    fail(`unexpected updater endpoint: ${JSON.stringify(endpoints)}`);
  }
  if (tauri.plugins?.updater?.pubkey !== EXPECTED_PUBLIC_KEY) {
    fail("unexpected RSTorrent updater public key");
  }
  const decodedPublicKey = Buffer.from(EXPECTED_PUBLIC_KEY, "base64").toString("utf8");
  if (
    !decodedPublicKey.startsWith("untrusted comment: minisign public key") ||
    !decodedPublicKey.includes("\nRW")
  ) {
    fail("RSTorrent updater public key is malformed");
  }
  const permissions = new Set(capability.permissions ?? []);
  if (!permissions.has("updater:default")) {
    fail("missing desktop permission updater:default");
  }
  if (permissions.has("process:default")) {
    fail("raw process restart permission must stay disabled");
  }
  if (!cargo.includes("tauri-plugin-updater =")) {
    fail("missing Rust dependency tauri-plugin-updater");
  }
  if (packageJson.dependencies?.["@tauri-apps/plugin-updater"] === undefined) {
    fail("missing web dependency for tauri-plugin-updater");
  }
  if (
    cargo.includes("tauri-plugin-process =") ||
    packageJson.dependencies?.["@tauri-apps/plugin-process"] !== undefined
  ) {
    fail("updater restart must not bypass joined native shutdown");
  }
  if (
    !desktopSource.includes("async fn application_restart(") ||
    !desktopSource.includes("application_restart,") ||
    !desktopSource.includes("app.request_restart();")
  ) {
    fail("desktop joined restart command is missing");
  }
  if (!tauriUpdater.includes('relaunch: () => invoke("application_restart")')) {
    fail("web updater does not use joined native restart");
  }

  const expectedProduct = {
    id: "rstorrent",
    displayName: "RSTorrent",
    hostnames: ["updates.graehlarts.com"],
    pathPrefix: "/rstorrent",
    githubRepo: "kzahel/rstorrent",
    tagPrefix: "desktop-v",
    tauriUpdates: true,
  };
  if (JSON.stringify(product) !== JSON.stringify(expectedProduct)) {
    fail(`unexpected updater product config: ${JSON.stringify(product)}`);
  }
  return { version, endpoint: EXPECTED_ENDPOINT };
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

export function validateDesktopReleaseRepository(root, tag) {
  return validateDesktopReleaseConfiguration({
    packageJson: readJson(path.join(root, "clients", "web", "package.json")),
    tauri: readJson(
      path.join(root, "clients", "desktop", "src-tauri", "tauri.conf.json"),
    ),
    developmentTauri: readJson(
      path.join(root, "clients", "desktop", "src-tauri", "tauri.dev.conf.json"),
    ),
    cargo: fs.readFileSync(
      path.join(root, "clients", "desktop", "src-tauri", "Cargo.toml"),
      "utf8",
    ),
    capability: readJson(
      path.join(
        root,
        "clients",
        "desktop",
        "src-tauri",
        "capabilities",
        "default.json",
      ),
    ),
    desktopSource: fs.readFileSync(
      path.join(root, "clients", "desktop", "src-tauri", "src", "lib.rs"),
      "utf8",
    ),
    tauriUpdater: fs.readFileSync(
      path.join(root, "clients", "web", "src", "tauri-updater.ts"),
      "utf8",
    ),
    product: readJson(path.join(root, "update-server", "rstorrent.json")),
    changelog: fs.readFileSync(path.join(root, "CHANGELOG.md"), "utf8"),
    tag,
  });
}

function parseTag(argv) {
  if (argv.length === 0) return undefined;
  if (argv.length === 2 && argv[0] === "--tag") return argv[1];
  fail("usage: validate-desktop-release.mjs [--tag desktop-vX.Y.Z]");
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    const root = path.resolve(import.meta.dirname, "..");
    const result = validateDesktopReleaseRepository(root, parseTag(process.argv.slice(2)));
    console.log(`Validated RSTorrent desktop release configuration ${result.version}`);
  } catch (error) {
    console.error(`Desktop release configuration failed: ${error.message}`);
    process.exitCode = 1;
  }
}
