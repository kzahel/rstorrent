#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

// Windows' hosted Python installation exposes python.exe; Unix uses python3.
execFileSync(process.platform === "win32" ? "python" : "python3", [
  fileURLToPath(new URL("./prepare-release-notices.py", import.meta.url)),
  ...process.argv.slice(2),
], { stdio: "inherit" });
