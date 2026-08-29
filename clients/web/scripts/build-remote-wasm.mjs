import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(webRoot, "../..");
const output = join(webRoot, ".remote-wasm");
const artifact = join(
  repositoryRoot,
  "target/wasm32-unknown-unknown/release/rstorrent_remote_wasm.wasm",
);

rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
run("cargo", [
  "build",
  "-p",
  "rstorrent-remote-wasm",
  "--target",
  "wasm32-unknown-unknown",
  "--release",
]);
run("wasm-bindgen", [artifact, "--out-dir", output, "--target", "web"]);

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: repositoryRoot,
    encoding: "utf8",
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error(`${command} failed with status ${String(result.status)}`);
  }
}
