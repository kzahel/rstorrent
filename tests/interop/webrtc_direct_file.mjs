import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import process from "node:process";
import readline from "node:readline";

const require = createRequire(new URL("../../clients/web/package.json", import.meta.url));
const { chromium, firefox } = require("playwright");
const browserName = process.argv[2] || "chromium";
const browserType = { chromium, firefox }[browserName];
if (!browserType) throw new Error(`unsupported browser: ${browserName}`);

const cargo = spawn(
  "cargo",
  [
    "run",
    "--quiet",
    "-p",
    "rstorrent-direct-file",
    "--features",
    "experiment",
    "--bin",
    "rstorrent-direct-file-experiment",
    "--",
    "--fixture-mib",
    "8",
  ],
  { cwd: new URL("../..", import.meta.url), env: { ...process.env, RUST_BACKTRACE: "1" } },
);
cargo.stderr.pipe(process.stderr);

const ready = new Promise((resolve, reject) => {
  const lines = readline.createInterface({ input: cargo.stdout });
  const timeout = setTimeout(() => reject(new Error("experiment startup timed out")), 120_000);
  lines.on("line", (line) => {
    const prefix = "RSTORRENT_DIRECT_FILE_READY ";
    if (line.startsWith(prefix)) {
      clearTimeout(timeout);
      resolve(JSON.parse(line.slice(prefix.length)));
    }
  });
  cargo.once("exit", (code) => reject(new Error(`experiment exited before ready: ${code}`)));
});

let browser;
try {
  const details = await ready;
  browser = await browserType.launch({ headless: true });
  const page = await browser.newPage();
  page.on("console", (message) => {
    if (message.type() === "error") process.stderr.write(`browser: ${message.text()}\n`);
  });
  await page.goto(details.url);
  await page.click("#start");
  await page.waitForFunction(() => window.__result !== undefined, null, { timeout: 120_000 });
  const outcome = await page.evaluate(() => window.__result);
  if (!outcome.ok) throw new Error(outcome.error);
  await page.click("#close");
  await page.waitForFunction(() => window.__terminal !== undefined, null, { timeout: 20_000 });
  const terminal = await page.evaluate(() => window.__terminal);
  if (terminal.active_tasks !== 0 || terminal.open_sockets !== 0 || terminal.active_requests !== 0) {
    throw new Error(`resources remained after close: ${JSON.stringify(terminal)}`);
  }
  process.stdout.write(`${JSON.stringify({ browser: browserName, outcome, terminal }, null, 2)}\n`);
} finally {
  if (browser) await browser.close();
  cargo.kill("SIGINT");
  await new Promise((resolve) => {
    cargo.once("exit", resolve);
    setTimeout(resolve, 5_000);
  });
}
