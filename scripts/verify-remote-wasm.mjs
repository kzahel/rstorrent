#!/usr/bin/env node

import { createServer } from "node:http";
import { mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { createRequire } from "node:module";
import { arch, cpus, platform, release, tmpdir, totalmem } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..");
const temporaryRoot = await mkdtemp(join(tmpdir(), "rstorrent-remote-wasm-"));
const productionDir = join(temporaryRoot, "production");
const benchmarkDir = join(temporaryRoot, "benchmark");
const wasmArtifact = join(
  repoRoot,
  "target/wasm32-unknown-unknown/release/rstorrent_remote_wasm.wasm",
);
const moduleName = "rstorrent_remote_wasm";
const testPassphrase = new TextEncoder().encode(
  "correct horse battery staple",
);
const commandPayload = new TextEncoder().encode(
  '{"type":"command","id":"wasm-proof"}',
);
const responsePayload = new TextEncoder().encode(
  '{"type":"snapshot","revision":1}',
);

let browser;
let server;
async function main() {
  try {
    buildArtifacts();
    const bundle = await bundleMeasurements();
    server = await serveWasm(benchmarkDir);
    browser = await launchBrowser();

    const deterministic = await runProtocolFlow("deterministic", false);
    const browserRandom = await runProtocolFlow("browser-random", true);
    const matrix = await measureArgon2Matrix();
    const maximumCandidateMs = Math.max(
      ...matrix.map((candidate) => candidate.durationMs),
    );
    const maximumLinearMemoryBytes = Math.max(
      ...matrix.map((candidate) => candidate.linearMemoryBytes),
    );
    if (maximumCandidateMs > 5_000) {
      throw new Error(
        `Argon2id candidate exceeded five seconds: ${maximumCandidateMs.toFixed(1)} ms`,
      );
    }
    if (maximumLinearMemoryBytes >= 256 * 1024 * 1024) {
      throw new Error(
        `Wasm linear memory exceeded bound: ${maximumLinearMemoryBytes} bytes`,
      );
    }

    process.stdout.write(
      `${JSON.stringify(
        {
          environment: {
            browser: browser.version(),
            headless: true,
            platform: `${platform()} ${release()} ${arch()}`,
            cpu: cpus()[0]?.model ?? "unknown",
            logicalCpuCount: cpus().length,
            systemMemoryBytes: totalmem(),
          },
          bundle,
          deterministic,
          browserRandom,
          matrix,
          maximumCandidateMs,
          maximumLinearMemoryBytes,
        },
        null,
        2,
      )}\n`,
    );
  } finally {
    if (browser !== undefined) {
      await browser.close();
    }
    if (server !== undefined) {
      await new Promise((resolveClose, rejectClose) =>
        server.close((error) =>
          error === undefined ? resolveClose() : rejectClose(error),
        ),
      );
    }
    await rm(temporaryRoot, { recursive: true, force: true });
  }
}

function buildArtifacts() {
  run("cargo", [
    "build",
    "-p",
    "rstorrent-remote-wasm",
    "--target",
    "wasm32-unknown-unknown",
    "--release",
  ]);
  run("wasm-bindgen", [
    wasmArtifact,
    "--out-dir",
    productionDir,
    "--target",
    "web",
  ]);
  run("cargo", [
    "build",
    "-p",
    "rstorrent-remote-wasm",
    "--target",
    "wasm32-unknown-unknown",
    "--release",
    "--features",
    "ksf-bench",
  ]);
  run("wasm-bindgen", [
    wasmArtifact,
    "--out-dir",
    benchmarkDir,
    "--target",
    "web",
  ]);
  run("cargo", [
    "build",
    "-p",
    "rstorrent-remote-crypto",
    "--example",
    "native_wasm_oracle",
  ]);
}

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.status !== 0) {
    process.stderr.write(result.stdout);
    process.stderr.write(result.stderr);
    throw new Error(`${command} failed with status ${result.status}`);
  }
}

async function bundleMeasurements() {
  const productionWasm = await stat(
    join(productionDir, `${moduleName}_bg.wasm`),
  );
  const productionJs = await stat(join(productionDir, `${moduleName}.js`));
  const benchmarkWasm = await stat(join(benchmarkDir, `${moduleName}_bg.wasm`));
  const benchmarkJs = await stat(join(benchmarkDir, `${moduleName}.js`));
  return {
    productionWasmBytes: productionWasm.size,
    productionJsBytes: productionJs.size,
    benchmarkWasmBytes: benchmarkWasm.size,
    benchmarkJsBytes: benchmarkJs.size,
  };
}

async function serveWasm(root) {
  const files = new Map([
    [
      `/${moduleName}.js`,
      {
        body: await readFile(join(root, `${moduleName}.js`)),
        type: "text/javascript",
      },
    ],
    [
      `/${moduleName}_bg.wasm`,
      {
        body: await readFile(join(root, `${moduleName}_bg.wasm`)),
        type: "application/wasm",
      },
    ],
    [
      "/",
      {
        body: Buffer.from("<!doctype html><title>RSTorrent remote Wasm proof</title>"),
        type: "text/html",
      },
    ],
  ]);
  const httpServer = createServer((request, response) => {
    const file = files.get(request.url ?? "");
    response.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
    response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
    response.setHeader("Cache-Control", "no-store");
    if (file === undefined) {
      response.writeHead(404).end();
      return;
    }
    response.setHeader("Content-Type", file.type);
    response.writeHead(200).end(file.body);
  });
  await new Promise((resolveListen, rejectListen) => {
    httpServer.once("error", rejectListen);
    httpServer.listen(0, "127.0.0.1", resolveListen);
  });
  return httpServer;
}

async function launchBrowser() {
  const requireFromWeb = createRequire(
    join(repoRoot, "clients/web/package.json"),
  );
  const { chromium } = requireFromWeb("playwright");
  const options = {
    headless: true,
    args: ["--enable-precise-memory-info"],
  };
  try {
    return await chromium.launch({ ...options, channel: "chrome" });
  } catch {
    return chromium.launch(options);
  }
}

function origin() {
  const address = server.address();
  if (address === null || typeof address === "string") {
    throw new Error("Wasm server has no TCP address");
  }
  return `http://127.0.0.1:${address.port}`;
}

async function createWasmPage() {
  const page = await browser.newPage();
  await page.goto(origin());
  await page.evaluate(async (name) => {
    const module = await import(`/${name}.js`);
    const exports = await module.default();
    globalThis.__remoteWasm = { module, exports };
  }, moduleName);
  return page;
}

async function runProtocolFlow(mode, browserEntropy) {
  const oracle = new NativeOracle(mode);
  const page = await createWasmPage();
  try {
    const ready = await oracle.start();
    const [, relayHex, hostHex, username] = ready.split(" ");
    const relayId = decodeHex(relayHex);
    const hostId = decodeHex(hostHex);
    await page.evaluate(
      ({ password, relay, host, name, random }) => {
        const { module } = globalThis.__remoteWasm;
        const entropy = (byte) => {
          const value = new Uint8Array(32);
          if (random) {
            crypto.getRandomValues(value);
          } else {
            value.fill(byte);
          }
          return value;
        };
        globalThis.__remoteFlow = {
          module,
          password: Uint8Array.from(password),
          relayId: Uint8Array.from(relay),
          hostId: Uint8Array.from(host),
          username: name,
          entropy,
        };
      },
      {
        password: [...testPassphrase],
        relay: [...relayId],
        host: [...hostId],
        name: username,
        random: browserEntropy,
      },
    );

    const shortEntropyFailure = await page.evaluate(() => {
      const { module, password } = globalThis.__remoteFlow;
      try {
        new module.ClientRegistration(password, new Uint8Array(31));
        return "missing rejection";
      } catch (error) {
        return String(error);
      }
    });
    if (!shortEntropyFailure.includes("exactly 32 bytes")) {
      throw new Error(`unexpected entropy rejection: ${shortEntropyFailure}`);
    }

    const registrationRequest = await page.evaluate(() => {
      const flow = globalThis.__remoteFlow;
      flow.registration = new flow.module.ClientRegistration(
        flow.password,
        flow.entropy(3),
      );
      return [...flow.registration.request()];
    });
    const registrationResponse = parsePayload(
      await oracle.send(`REG_START ${encodeHex(registrationRequest)}`),
      "REG_RESPONSE",
    );
    const registration = await page.evaluate(
      ({ response, random }) => {
        const flow = globalThis.__remoteFlow;
        const start = performance.now();
        const upload = flow.registration.finish(
          flow.password,
          flow.relayId,
          flow.username,
          flow.hostId,
          Uint8Array.from(response),
          flow.entropy(4),
        );
        const durationMs = performance.now() - start;
        let reuse;
        try {
          flow.registration.finish(
            flow.password,
            flow.relayId,
            flow.username,
            flow.hostId,
            Uint8Array.from(response),
            flow.entropy(4),
          );
          reuse = "missing rejection";
        } catch (error) {
          reuse = String(error);
        }
        return { upload: [...upload], durationMs, reuse, random };
      },
      { response: registrationResponse, random: browserEntropy },
    );
    if (!registration.reuse.includes("already been consumed")) {
      throw new Error(`registration handle was reusable: ${registration.reuse}`);
    }
    expectOk(
      await oracle.send(`REG_FINISH ${encodeHex(registration.upload)}`),
    );

    const firstLogin = await login(page, oracle, [], browserEntropy);
    expectOk(await oracle.send(`PIN ${encodeHex(firstLogin.pin)}`));
    await exerciseRecords(page, oracle);

    let repeatedPin = false;
    let pinMismatchRejected = false;
    if (browserEntropy) {
      const secondLogin = await login(
        page,
        oracle,
        firstLogin.pin,
        browserEntropy,
      );
      repeatedPin = encodeHex(secondLogin.pin) === encodeHex(firstLogin.pin);
      if (!repeatedPin) {
        throw new Error("repeated login changed the host pin");
      }
      await exerciseRecords(page, oracle);
      await expectPinMismatch(page, oracle, firstLogin.pin);
      pinMismatchRejected = true;
    }
    expectPrefix(await oracle.send("QUIT"), "BYE");
    return {
      browserEntropy,
      entropyFailureRejected: true,
      registrationFinishMs: registration.durationMs,
      loginFinishMs: firstLogin.durationMs,
      consumingHandles: true,
      repeatedPin: browserEntropy ? repeatedPin : undefined,
      pinMismatchRejected: browserEntropy ? pinMismatchRejected : undefined,
      nativeMessageAndRecordEquivalence: !browserEntropy,
    };
  } finally {
    await page.close();
    await oracle.close();
  }
}

async function expectPinMismatch(page, oracle, expectedPin) {
  const loginRequest = await page.evaluate(() => {
    const flow = globalThis.__remoteFlow;
    flow.login = new flow.module.ClientLogin(flow.password, flow.entropy(5));
    return [...flow.login.request()];
  });
  const loginResponse = parsePayload(
    await oracle.send(`LOGIN_START ${encodeHex(loginRequest)}`),
    "LOGIN_RESPONSE",
  );
  const changedPin = [...expectedPin];
  changedPin[changedPin.length - 1] ^= 1;
  const result = await page.evaluate(
    ({ response, pin }) => {
      const flow = globalThis.__remoteFlow;
      let mismatch;
      try {
        flow.login.finish(
          flow.password,
          flow.relayId,
          flow.username,
          flow.hostId,
          Uint8Array.from(pin),
          Uint8Array.from(response),
          flow.entropy(7),
        );
        mismatch = "missing rejection";
      } catch (error) {
        mismatch = String(error);
      }
      let reuse;
      try {
        flow.login.finish(
          flow.password,
          flow.relayId,
          flow.username,
          flow.hostId,
          Uint8Array.from(pin),
          Uint8Array.from(response),
          flow.entropy(7),
        );
        reuse = "missing rejection";
      } catch (error) {
        reuse = String(error);
      }
      return { mismatch, reuse };
    },
    { response: loginResponse, pin: changedPin },
  );
  if (!result.mismatch.includes("host identity changed")) {
    throw new Error(`changed pin was accepted: ${result.mismatch}`);
  }
  if (!result.reuse.includes("already been consumed")) {
    throw new Error(`failed login handle was reusable: ${result.reuse}`);
  }
}

async function login(page, oracle, expectedPin, browserEntropy) {
  const loginRequest = await page.evaluate(() => {
    const flow = globalThis.__remoteFlow;
    flow.login = new flow.module.ClientLogin(flow.password, flow.entropy(5));
    return [...flow.login.request()];
  });
  const loginResponse = parsePayload(
    await oracle.send(`LOGIN_START ${encodeHex(loginRequest)}`),
    "LOGIN_RESPONSE",
  );
  const result = await page.evaluate(
    ({ response, pin, random }) => {
      const flow = globalThis.__remoteFlow;
      const start = performance.now();
      flow.session = flow.login.finish(
        flow.password,
        flow.relayId,
        flow.username,
        flow.hostId,
        Uint8Array.from(pin),
        Uint8Array.from(response),
        flow.entropy(7),
      );
      const durationMs = performance.now() - start;
      const finalization = flow.session.take_finalization();
      let reuse;
      try {
        flow.session.take_finalization();
        reuse = "missing rejection";
      } catch (error) {
        reuse = String(error);
      }
      return {
        finalization: [...finalization],
        pin: [...flow.session.host_pin()],
        durationMs,
        reuse,
        random,
      };
    },
    { response: loginResponse, pin: expectedPin, random: browserEntropy },
  );
  if (!result.reuse.includes("already been consumed")) {
    throw new Error(`finalization was reusable: ${result.reuse}`);
  }
  expectOk(
    await oracle.send(`LOGIN_FINISH ${encodeHex(result.finalization)}`),
  );
  return result;
}

async function exerciseRecords(page, oracle) {
  const clientRecord = await page.evaluate((payload) => {
    return [...globalThis.__remoteFlow.session.seal(Uint8Array.from(payload))];
  }, [...commandPayload]);
  const opened = await oracle.send(`OPEN ${encodeHex(clientRecord)}`);
  const [openedPrefix, close, plaintext] = opened.split(" ");
  expectPrefix(openedPrefix, "OPENED");
  if (close !== "0" || plaintext !== encodeHex(commandPayload)) {
    throw new Error("native host opened the wrong browser record");
  }

  const hostRecord = parsePayload(
    await oracle.send(`SEAL ${encodeHex(responsePayload)}`),
    "RECORD",
  );
  const browserOpened = await page.evaluate((record) => {
    const openedRecord = globalThis.__remoteFlow.session.open(
      Uint8Array.from(record),
    );
    return {
      plaintext: [...openedRecord.plaintext],
      close: openedRecord.isClose,
    };
  }, hostRecord);
  if (
    browserOpened.close ||
    encodeHex(browserOpened.plaintext) !== encodeHex(responsePayload)
  ) {
    throw new Error("browser opened the wrong native record");
  }

  const closeRecord = await page.evaluate(() => [
    ...globalThis.__remoteFlow.session.seal_close(),
  ]);
  const nativeClose = await oracle.send(`OPEN ${encodeHex(closeRecord)}`);
  if (!nativeClose.startsWith("OPENED 1 ")) {
    throw new Error(`native host did not accept close: ${nativeClose}`);
  }
  const postClose = await page.evaluate((payload) => {
    try {
      globalThis.__remoteFlow.session.seal(Uint8Array.from(payload));
      return "missing rejection";
    } catch (error) {
      return String(error);
    }
  }, [...commandPayload]);
  if (!postClose.includes("closed")) {
    throw new Error(`post-close record was accepted: ${postClose}`);
  }
}

async function measureArgon2Matrix() {
  const results = [];
  for (const memoryKib of [32, 64, 96, 128].map((value) => value * 1024)) {
    for (const passes of [1, 2, 3, 4]) {
      const page = await createWasmPage();
      try {
        const result = await page.evaluate(
          ({ memory, iterations }) => {
            const { module, exports } = globalThis.__remoteWasm;
            const input = new Uint8Array(64);
            input.fill(0x42);
            const before = exports.memory.buffer.byteLength;
            const start = performance.now();
            module.exerciseArgon2idCandidate(input, memory, iterations);
            const durationMs = performance.now() - start;
            return {
              memoryKib: memory,
              passes: iterations,
              durationMs,
              initialLinearMemoryBytes: before,
              linearMemoryBytes: exports.memory.buffer.byteLength,
              jsHeapBytes:
                performance.memory?.usedJSHeapSize === undefined
                  ? null
                  : performance.memory.usedJSHeapSize,
            };
          },
          { memory: memoryKib, iterations: passes },
        );
        results.push(result);
      } finally {
        await page.close();
      }
    }
  }
  return results;
}

class NativeOracle {
  constructor(mode) {
    this.mode = mode;
    this.pending = [];
    this.lines = [];
    this.stderr = "";
  }

  async start() {
    const executable = join(
      repoRoot,
      `target/debug/examples/native_wasm_oracle${process.platform === "win32" ? ".exe" : ""}`,
    );
    this.child = spawn(executable, [], {
      cwd: repoRoot,
      stdio: ["pipe", "pipe", "pipe"],
    });
    this.child.stderr.setEncoding("utf8");
    this.child.stderr.on("data", (chunk) => {
      this.stderr += chunk;
    });
    const lines = createInterface({ input: this.child.stdout });
    lines.on("line", (line) => {
      const waiter = this.pending.shift();
      if (waiter === undefined) {
        this.lines.push(line);
      } else {
        waiter.resolve(line);
      }
    });
    this.child.once("error", (error) => {
      for (const waiter of this.pending.splice(0)) {
        waiter.reject(error);
      }
    });
    this.child.stdin.write(`MODE ${this.mode}\n`);
    const ready = await this.nextLine();
    expectPrefix(ready, "READY");
    return ready;
  }

  async send(line) {
    if (this.child === undefined || this.child.exitCode !== null) {
      throw new Error(`native oracle is unavailable: ${this.stderr}`);
    }
    this.child.stdin.write(`${line}\n`);
    const response = await this.nextLine();
    if (response.startsWith("ERROR ")) {
      throw new Error(response);
    }
    return response;
  }

  nextLine() {
    const buffered = this.lines.shift();
    if (buffered !== undefined) {
      return Promise.resolve(buffered);
    }
    return new Promise((resolveLine, rejectLine) => {
      this.pending.push({ resolve: resolveLine, reject: rejectLine });
    });
  }

  async close() {
    if (this.child === undefined || this.child.exitCode !== null) {
      return;
    }
    this.child.stdin.end();
    await new Promise((resolveExit) => {
      this.child.once("exit", resolveExit);
      setTimeout(() => {
        if (this.child.exitCode === null) {
          this.child.kill();
        }
      }, 1_000).unref();
    });
  }
}

function parsePayload(line, prefix) {
  const [actual, encoded] = line.split(" ");
  expectPrefix(actual, prefix);
  if (encoded === undefined) {
    throw new Error(`${prefix} omitted its payload`);
  }
  return [...decodeHex(encoded)];
}

function expectOk(line) {
  expectPrefix(line, "OK");
}

function expectPrefix(actual, expected) {
  if (actual !== expected && !actual.startsWith(`${expected} `)) {
    throw new Error(`expected ${expected}, received ${actual}`);
  }
}

function encodeHex(bytes) {
  return [...bytes]
    .map((byte) => Number(byte).toString(16).padStart(2, "0"))
    .join("");
}

function decodeHex(encoded) {
  if (encoded.length % 2 !== 0 || !/^[0-9a-f]*$/.test(encoded)) {
    throw new Error("invalid oracle hex");
  }
  return Uint8Array.from(
    encoded.match(/../g)?.map((pair) => Number.parseInt(pair, 16)) ?? [],
  );
}

await main();
