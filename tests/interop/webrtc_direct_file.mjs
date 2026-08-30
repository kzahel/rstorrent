import { execFile, spawn } from "node:child_process";
import { createRequire } from "node:module";
import process from "node:process";
import readline from "node:readline";

const require = createRequire(new URL("../../clients/web/package.json", import.meta.url));
const { chromium, firefox, webkit } = require("playwright");
const browserName = process.argv[2] || "chromium";
const browserType = { chromium, firefox, webkit }[browserName];
if (!browserType) throw new Error(`unsupported browser: ${browserName}`);
const fixtureMib = process.env.RSTORRENT_WEBRTC_FIXTURE_MIB || "8";
if (!/^[1-9][0-9]{0,2}$/.test(fixtureMib) || Number(fixtureMib) > 256) {
  throw new Error(`invalid RSTORRENT_WEBRTC_FIXTURE_MIB: ${fixtureMib}`);
}
const cargoArguments = [
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
    fixtureMib,
];

const cargo = spawn(
  "cargo",
  cargoArguments,
  { cwd: new URL("../..", import.meta.url), env: { ...process.env, RUST_BACKTRACE: "1" } },
);
cargo.stderr.pipe(process.stderr);

function execText(command, arguments_) {
  return new Promise((resolve, reject) => {
    execFile(command, arguments_, { encoding: "utf8" }, (error, stdout) => {
      if (error) reject(error);
      else resolve(stdout.trim());
    });
  });
}

async function processSample(pid) {
  if (!pid) return null;
  const output = await execText("ps", ["-o", "rss=", "-o", "%cpu=", "-p", String(pid)]).catch(() => "");
  const [rssKib, cpuPercent] = output.trim().split(/\s+/).map(Number);
  return Number.isFinite(rssKib) && Number.isFinite(cpuPercent) ? { rssKib, cpuPercent } : null;
}

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
let sampleTimer;
try {
  const details = await ready;
  await new Promise((resolve) => setTimeout(resolve, 250));
  const rustPid = process.platform === "win32" ? null : details.pid;
  const idleEndpoint = await fetch(new URL("status", details.url)).then((response) => response.json());
  if (idleEndpoint.active_tasks !== 0 || idleEndpoint.open_sockets !== 0) {
    throw new Error(`WebRTC resources existed before signaling: ${JSON.stringify(idleEndpoint)}`);
  }
  const idleProcess = await processSample(rustPid);
  const processHighWater = { rssKib: idleProcess?.rssKib || 0, cpuPercent: idleProcess?.cpuPercent || 0 };
  let sampleInFlight = false;
  sampleTimer = setInterval(async () => {
    if (sampleInFlight) return;
    sampleInFlight = true;
    const sample = await processSample(rustPid);
    if (sample) {
      processHighWater.rssKib = Math.max(processHighWater.rssKib, sample.rssKib);
      processHighWater.cpuPercent = Math.max(processHighWater.cpuPercent, sample.cpuPercent);
    }
    sampleInFlight = false;
  }, 100);
  browser = await browserType.launch({ headless: true });
  const page = await browser.newPage();
  page.on("console", (message) => {
    if (message.type() === "error") process.stderr.write(`browser: ${message.text()}\n`);
  });
  await page.goto(details.url);
  await page.click("#start");
  await page.waitForFunction(() => window.__result !== undefined, null, { timeout: 120_000 });
  const outcome = await page.evaluate(() => window.__result);
  if (!outcome.ok) {
    const server = await fetch(new URL("status", details.url)).then((response) => response.json());
    throw new Error(`${outcome.error}; server=${JSON.stringify(server)}`);
  }
  await page.click("#close");
  await page.waitForFunction(() => window.__terminal !== undefined, null, { timeout: 20_000 });
  const terminal = await page.evaluate(() => window.__terminal);
  if (terminal.active_tasks !== 0 || terminal.open_sockets !== 0 || terminal.active_requests !== 0) {
    throw new Error(`resources remained after close: ${JSON.stringify(terminal)}`);
  }
  clearInterval(sampleTimer);
  sampleTimer = undefined;
  const finalSample = await processSample(rustPid);
  if (finalSample) {
    processHighWater.rssKib = Math.max(processHighWater.rssKib, finalSample.rssKib);
    processHighWater.cpuPercent = Math.max(processHighWater.cpuPercent, finalSample.cpuPercent);
  }
  process.stdout.write(`${JSON.stringify({
    browser: browserName,
    outcome,
    terminal,
    process: { pid: rustPid, idleEndpoint, idle: idleProcess, highWater: processHighWater },
  }, null, 2)}\n`);
} finally {
  if (sampleTimer) clearInterval(sampleTimer);
  if (browser) await browser.close();
  cargo.kill("SIGINT");
  await new Promise((resolve) => {
    cargo.once("exit", resolve);
    setTimeout(resolve, 5_000);
  });
}
