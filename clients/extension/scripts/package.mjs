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

import { extensionRoot, packagedFiles, validateArchive, validateSource } from "./validate.mjs";

validateSource();

const manifest = JSON.parse(readFileSync(path.join(extensionRoot, "manifest.json"), "utf8"));
const outputDirectory = path.resolve(extensionRoot, "../../target/extension");
const outputPath = path.join(outputDirectory, `jstorrent-beta-${manifest.version}.zip`);
const staging = mkdtempSync(path.join(os.tmpdir(), "jstorrent-beta-extension-"));
const fixedTime = new Date("2020-01-01T00:00:00.000Z");

try {
  for (const relativePath of packagedFiles) {
    const destination = path.join(staging, relativePath);
    mkdirSync(path.dirname(destination), { recursive: true });
    copyFileSync(path.join(extensionRoot, relativePath), destination);
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
