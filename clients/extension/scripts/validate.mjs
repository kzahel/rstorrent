import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";

export const extensionRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export const packagedFiles = Object.freeze([
  "manifest.json",
  "icons/icon-32.png",
  "icons/icon-128.png",
  "popup/popup.html",
  "popup/popup.css",
  "popup/popup.js",
  "src/service-worker.js",
]);

function fail(message) {
  throw new Error(`extension validation failed: ${message}`);
}

export function validateSource() {
  const manifest = JSON.parse(readFileSync(path.join(extensionRoot, "manifest.json"), "utf8"));
  if (manifest.manifest_version !== 3 || manifest.name !== "JSTorrent Beta") {
    fail("expected the reviewed JSTorrent Beta Manifest V3 identity");
  }
  if (manifest.key !== undefined) {
    fail("store-seed manifest must omit key until the dashboard public key is returned");
  }
  if (JSON.stringify(manifest.permissions) !== JSON.stringify(["nativeMessaging"])) {
    fail("nativeMessaging must be the only extension permission");
  }
  for (const forbidden of [
    "host_permissions",
    "content_scripts",
    "web_accessible_resources",
    "externally_connectable",
  ]) {
    if (manifest[forbidden] !== undefined) {
      fail(`manifest must not declare ${forbidden}`);
    }
  }
  if (manifest.background?.service_worker !== "src/service-worker.js") {
    fail("unexpected service worker entry point");
  }
  if (manifest.action?.default_popup !== "popup/popup.html") {
    fail("unexpected popup entry point");
  }

  for (const relativePath of packagedFiles) {
    readFileSync(path.join(extensionRoot, relativePath));
  }

  for (const relativePath of packagedFiles.filter((file) => file.endsWith(".js"))) {
    const absolutePath = path.join(extensionRoot, relativePath);
    const source = readFileSync(absolutePath, "utf8");
    if (/https?:\/\//u.test(source) || /\beval\s*\(|\bnew\s+Function\b/u.test(source)) {
      fail(`${relativePath} contains remote-code or dynamic-code syntax`);
    }
    execFileSync(process.execPath, ["--check", absolutePath], { stdio: "pipe" });
  }

  const popup = readFileSync(path.join(extensionRoot, "popup/popup.html"), "utf8");
  if (/<script(?![^>]*\bsrc=)/iu.test(popup) || /\son[a-z]+\s*=/iu.test(popup)) {
    fail("popup contains inline executable markup");
  }
  if (!popup.includes('<script src="popup.js"></script>')) {
    fail("popup script must remain a local external file");
  }
}

export function validateArchive(archivePath) {
  const entries = execFileSync("unzip", ["-Z1", archivePath], { encoding: "utf8" })
    .trim()
    .split("\n")
    .filter(Boolean)
    .sort();
  const expected = [...packagedFiles].sort();
  if (JSON.stringify(entries) !== JSON.stringify(expected)) {
    fail(`archive entries differ from reviewed allowlist: ${entries.join(", ")}`);
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  validateSource();
  const archiveIndex = process.argv.indexOf("--archive");
  if (archiveIndex !== -1) {
    const archivePath = process.argv[archiveIndex + 1];
    if (!archivePath) {
      fail("--archive requires a path");
    }
    validateArchive(path.resolve(archivePath));
  }
  console.log("JSTorrent Beta extension validation passed.");
}
