#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

if (process.platform === "linux") {
  execFileSync("python3", [fileURLToPath(new URL("./prepare-appimage-notices.py", import.meta.url))], {
    stdio: "inherit",
  });
}
