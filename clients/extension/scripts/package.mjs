import { execFileSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  utimesSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";

import {
  companionPackagedFiles,
  extensionRoot,
  packagedFiles,
  validateArchive,
  validateCompanionBuild,
  validateSource,
} from "./validate.mjs";

validateSource();
const webRoot = path.resolve(extensionRoot, "../web");
execFileSync("npm", ["run", "build:companion"], {
  cwd: webRoot,
  env: process.env,
  stdio: "inherit",
});
const companionRoot = path.join(webRoot, "dist/companion");
validateCompanionBuild(companionRoot);

const manifest = JSON.parse(readFileSync(path.join(extensionRoot, "manifest.json"), "utf8"));
const outputDirectory = path.resolve(extensionRoot, "../../target/extension");
const outputPath = path.join(outputDirectory, `jstorrent-beta-${manifest.version}.zip`);
const staging = mkdtempSync(path.join(os.tmpdir(), "jstorrent-beta-extension-"));
const fixedTime = new Date("2020-01-01T00:00:00.000Z");

try {
  for (const relativePath of packagedFiles) {
    const destination = path.join(staging, relativePath);
    mkdirSync(path.dirname(destination), { recursive: true });
    const source = companionPackagedFiles.includes(relativePath)
      ? path.join(companionRoot, relativePath.replace(/^companion\//u, ""))
      : path.join(extensionRoot, relativePath);
    copyFileSync(source, destination);
    chmodSync(destination, 0o644);
    utimesSync(destination, fixedTime, fixedTime);
  }
  mkdirSync(outputDirectory, { recursive: true });
  rmSync(outputPath, { force: true });
  execFileSync("zip", ["-X", "-q", outputPath, ...packagedFiles], {
    cwd: staging,
    env: { ...process.env, TZ: "UTC" },
    stdio: "inherit",
  });
  validateArchive(outputPath);
  console.log(outputPath);
} finally {
  rmSync(staging, { recursive: true, force: true });
}
