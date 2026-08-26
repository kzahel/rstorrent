#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const release = process.argv.includes("--release");
const explicitTarget =
  process.env.RSTORRENT_NATIVE_HOST_TARGET || process.env.TAURI_ENV_TARGET_TRIPLE;
const hostTarget = execFileSync("rustc", ["-vV"], { encoding: "utf8" })
  .split("\n")
  .find((line) => line.startsWith("host: "))
  ?.slice("host: ".length);
const target = explicitTarget || hostTarget;
if (!target) {
  throw new Error("could not determine the Rust target triple for the native host");
}

const windows = target.includes("windows");
const executable = `rstorrent-native-host${windows ? ".exe" : ""}`;
const cargoArguments = ["build", "-p", "rstorrent-native-host"];
if (release) cargoArguments.push("--release");
if (explicitTarget) cargoArguments.push("--target", target);
execFileSync("cargo", cargoArguments, { cwd: repositoryRoot, stdio: "inherit" });

const targetRoot = path.resolve(
  repositoryRoot,
  process.env.CARGO_TARGET_DIR || "target",
  ...(explicitTarget ? [target] : []),
  release ? "release" : "debug",
);
const source = path.join(targetRoot, executable);
const destinationDirectory = path.join(repositoryRoot, "clients/desktop/src-tauri/binaries");
const destination = path.join(
  destinationDirectory,
  `rstorrent-native-host-${target}${windows ? ".exe" : ""}`,
);
mkdirSync(destinationDirectory, { recursive: true });
copyFileSync(source, destination);
if (!windows) chmodSync(destination, 0o755);
console.log(`Prepared ${path.relative(repositoryRoot, destination)}`);
