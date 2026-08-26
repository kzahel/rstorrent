import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";

export const extensionRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const storeExtensionId = "gcgoepclopkgijmclmlheafaglmbjlcc";

export const packagedFiles = Object.freeze([
  "manifest.json",
  "icons/icon-32.png",
  "icons/icon-128.png",
  "popup/popup.html",
  "popup/popup.css",
  "popup/popup.js",
  "popup/platform.js",
  "crostini/setup.html",
  "crostini/setup.css",
  "src/service-worker.js",
]);

function fail(message) {
  throw new Error(`extension validation failed: ${message}`);
}

export function extensionIdFromPublicKey(publicKey) {
  if (typeof publicKey !== "string" || publicKey.length === 0) {
    fail("manifest key must be a nonempty base64 public key");
  }
  const der = Buffer.from(publicKey, "base64");
  if (der.length === 0 || der.toString("base64") !== publicKey) {
    fail("manifest key must be canonical unwrapped base64");
  }
  return createHash("sha256")
    .update(der)
    .digest("hex")
    .slice(0, 32)
    .replace(/[0-9a-f]/gu, (nibble) =>
      String.fromCharCode("a".charCodeAt(0) + Number.parseInt(nibble, 16)),
    );
}

export function validateSource() {
  const manifest = JSON.parse(readFileSync(path.join(extensionRoot, "manifest.json"), "utf8"));
  if (manifest.manifest_version !== 3 || manifest.name !== "JSTorrent Beta") {
    fail("expected the reviewed JSTorrent Beta Manifest V3 identity");
  }
  const derivedExtensionId = extensionIdFromPublicKey(manifest.key);
  if (derivedExtensionId !== storeExtensionId) {
    fail(`manifest key derives ${derivedExtensionId}, expected store item ${storeExtensionId}`);
  }
  if (JSON.stringify(manifest.permissions) !== JSON.stringify(["nativeMessaging", "storage"])) {
    fail("only nativeMessaging and storage permissions are accepted");
  }
  for (const forbidden of ["host_permissions", "content_scripts", "web_accessible_resources"]) {
    if (manifest[forbidden] !== undefined) {
      fail(`manifest must not declare ${forbidden}`);
    }
  }
  if (
    JSON.stringify(manifest.externally_connectable) !==
    JSON.stringify({ matches: ["http://penguin.linux.test/*"] })
  ) {
    fail("externally_connectable must contain only the exact Crostini host match");
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
    if (/\beval\s*\(|\bnew\s+Function\b/u.test(source)) {
      fail(`${relativePath} contains dynamic-code syntax`);
    }
    const urls = source.match(/https?:\/\/[^"'`\s)]+/gu) ?? [];
    if (urls.some((url) => url !== "http://penguin.linux.test:3030")) {
      fail(`${relativePath} contains an unexpected remote URL`);
    }
    execFileSync(process.execPath, ["--check", absolutePath], { stdio: "pipe" });
  }

  const popup = readFileSync(path.join(extensionRoot, "popup/popup.html"), "utf8");
  if (/<script(?![^>]*\bsrc=)/iu.test(popup) || /\son[a-z]+\s*=/iu.test(popup)) {
    fail("popup contains inline executable markup");
  }
  if (!popup.includes('<script type="module" src="popup.js"></script>')) {
    fail("popup script must remain a local external module");
  }
  const playStoreUrl = "https://play.google.com/store/apps/details?id=com.jstorrent.app";
  const popupUrls = popup.match(/https?:\/\/[^"'\s<]+/gu) ?? [];
  if (JSON.stringify(popupUrls) !== JSON.stringify([playStoreUrl])) {
    fail("popup must link only to the exact published JSTorrent Android listing");
  }
  if (!popup.includes('id="desktop-surface" hidden') || !popup.includes('id="chromeos-surface" hidden')) {
    fail("popup surfaces must start hidden until the platform decision completes");
  }
  const setup = readFileSync(path.join(extensionRoot, "crostini/setup.html"), "utf8");
  if (/<script/iu.test(setup) || /\son[a-z]+\s*=/iu.test(setup)) {
    fail("Crostini setup must remain a static offline document");
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
