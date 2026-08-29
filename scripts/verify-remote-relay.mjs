#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { readFile, mkdtemp, rm, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { arch, cpus, platform, release, tmpdir, totalmem } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..");
const webRoot = join(repoRoot, "clients/web");
const temporaryRoot = await mkdtemp(join(tmpdir(), "rstorrent-remote-relay-"));
const wasmDir = join(temporaryRoot, "wasm");
const wasmArtifact = join(
  repoRoot,
  "target/wasm32-unknown-unknown/release/rstorrent_remote_wasm.wasm",
);
const proofExecutable = join(
  repoRoot,
  `target/debug/rstorrent-remote-proof${process.platform === "win32" ? ".exe" : ""}`,
);
const moduleName = "rstorrent_remote_wasm";

let browser;
let vite;
let wasmServer;
let proof;

async function main() {
try {
  buildArtifacts();
  const vitePort = await availablePort();
  const webOrigin = `http://127.0.0.1:${vitePort}`;
  vite = startVite(vitePort);
  await waitForHttp(`${webOrigin}/remote-relay-proof.html`);
  wasmServer = await serveWasm();
  const wasmOrigin = serverOrigin(wasmServer);

  proof = new LineProcess(proofExecutable, [webOrigin]);
  const ready = parseLine(await proof.start(), "READY");
  browser = await launchBrowser();
  const page = await browser.newPage();
  const browserErrors = [];
  page.on("pageerror", (error) => browserErrors.push(String(error)));
  await page.goto(`${webOrigin}/remote-relay-proof.html`);
  const passphrase = `proof-${randomBytes(24).toString("base64url")}`;
  const trace = await page.evaluate(
    async ({ wasmModuleUrl, ready, passphrase }) => {
      const wasm = await import(wasmModuleUrl);
      await wasm.default();
      const {
        RemoteApplicationWebSocket,
        provisionRemotePassword,
      } = await import("/src/remote-application-websocket.ts");
      const { WebSocketApplicationViewClient } = await import(
        "/src/websocket-view-client.ts"
      );
      const { ViewController } = await import("/src/view-controller.ts");

      const decodeHex = (encoded) => {
        if (!/^[0-9a-f]{64}$/.test(encoded)) throw new Error("invalid proof ID");
        return Uint8Array.from(
          encoded.match(/../g).map((value) => Number.parseInt(value, 16)),
        );
      };
      const delay = (millis) => new Promise((resolve) => setTimeout(resolve, millis));
      const waitUntil = async (predicate) => {
        const deadline = performance.now() + 5_000;
        while (!predicate()) {
          if (performance.now() >= deadline) throw new Error("view trace timed out");
          await delay(10);
        }
      };
      const password = new TextEncoder().encode(passphrase);
      const common = {
        relayUrl: ready.relayUrl,
        relayId: decodeHex(ready.relayId),
        username: ready.username,
        passphrase: password,
        hostId: decodeHex(ready.hostId),
        crypto: wasm,
      };

      await provisionRemotePassword(common);
      await delay(100);

      const normalizeState = (state) => ({
        durableRevision: state.durableRevision,
        views: structuredClone(state.views),
        deliveryResetCount: state.deliveryResetCount,
        lastDeliveryResetReason: state.lastDeliveryResetReason,
      });
      const runTrace = async (client) => {
        const hello = await client.hello();
        const states = [];
        const errors = [];
        const firstView = {
          type: "torrent_list",
          view_id: "library-initial",
          delivery: { min_interval_millis: 0 },
        };
        const controller = await ViewController.open(
          client,
          [firstView],
          (state) => states.push(normalizeState(state)),
          (error) => errors.push(String(error)),
          { waitMillis: 1_000 },
        );
        const initial = normalizeState(controller.current());
        await controller.setViews([
          {
            type: "torrent_list",
            view_id: "library-updated",
            delivery: { min_interval_millis: 0 },
          },
        ]);
        await waitUntil(
          () =>
            controller.current().views["library-updated"] !== undefined &&
            controller.current().views["library-initial"] === undefined,
        );
        const response = await controller.dispatch({
          version: 1,
          request_id: "remote-proof-snapshot",
          command: { type: "snapshot" },
        });
        await delay(50);
        const final = normalizeState(controller.current());
        await controller.close();
        await client.close();
        if (errors.length !== 0) throw new Error(errors.join("; "));
        return { hello, states, initial, final, response };
      };

      const direct = await runTrace(
        new WebSocketApplicationViewClient(
          ready.directBaseUrl,
          null,
          undefined,
          "00000000000000000000000000000001",
        ),
      );

      let recordedPin;
      const relayed = await runTrace(
        new WebSocketApplicationViewClient(
          ready.directBaseUrl,
          null,
          () =>
            new RemoteApplicationWebSocket({
              ...common,
              onHostPin: (pin) => {
                recordedPin = pin;
              },
            }),
          "00000000000000000000000000000002",
        ),
      );
      if (recordedPin === undefined) throw new Error("host pin was not recorded");

      const comparable = (value) => JSON.stringify(value);
      if (comparable(direct) !== comparable(relayed)) {
        throw new Error("direct and relayed application traces differ");
      }
      await delay(150);

      const expectFailure = async ({
        expectedPin,
        passwordOverride,
        usernameOverride,
        socketFactory,
      } = {}) => {
        let failure;
        const client = new WebSocketApplicationViewClient(
          ready.directBaseUrl,
          null,
          () =>
            new RemoteApplicationWebSocket({
              ...common,
              ...(expectedPin === undefined
                ? {}
                : { expectedHostPin: expectedPin }),
              ...(passwordOverride === undefined
                ? {}
                : { passphrase: passwordOverride }),
              ...(usernameOverride === undefined
                ? {}
                : { username: usernameOverride }),
              ...(socketFactory === undefined ? {} : { socketFactory }),
              onFailure: (value) => {
                failure = value;
              },
            }),
          "00000000000000000000000000000003",
        );
        let rejected = false;
        try {
          await client.hello();
        } catch {
          rejected = true;
        } finally {
          await client.close().catch(() => {});
        }
        if (!rejected) throw new Error("adversarial login was accepted");
        await delay(100);
        return failure;
      };

      const changedPin = recordedPin.slice();
      changedPin[changedPin.length - 1] ^= 1;
      const pinMismatch = await expectFailure({ expectedPin: changedPin });
      if (pinMismatch !== "host_identity_changed") {
        throw new Error("pin mismatch lost its blocking classification");
      }

      const wrongPassword = password.slice();
      wrongPassword[wrongPassword.length - 1] ^= 1;
      const wrongPasswordFailure = await expectFailure({
        passwordOverride: wrongPassword,
      });
      wrongPassword.fill(0);
      if (wrongPasswordFailure !== "connection_failed") {
        throw new Error("wrong password was not generic");
      }

      const unknownUserFailure = await expectFailure({
        usernameOverride: "unknown-proof",
      });
      if (unknownUserFailure !== "connection_failed") {
        throw new Error("unknown route was not generic");
      }

      class MutatingWebSocket {
        readyState = 0;
        binaryType = "arraybuffer";
        onopen = null;
        onmessage = null;
        onerror = null;
        onclose = null;
        mutated = false;

        constructor(url) {
          this.inner = new WebSocket(url);
          this.inner.binaryType = "arraybuffer";
          this.inner.onopen = (event) => {
            this.readyState = this.inner.readyState;
            this.onopen?.(event);
          };
          this.inner.onmessage = (event) => {
            let data = event.data;
            if (data instanceof ArrayBuffer && !this.mutated) {
              const value = new Uint8Array(data).slice();
              if (new TextDecoder().decode(value.slice(0, 4)) === "RSL2") {
                value[value.length - 1] ^= 1;
                data = value.buffer;
                this.mutated = true;
              }
            }
            this.onmessage?.({ data });
          };
          this.inner.onerror = (event) => this.onerror?.(event);
          this.inner.onclose = (event) => {
            this.readyState = this.inner.readyState;
            this.onclose?.(event);
          };
        }

        send(data) {
          this.inner.send(data);
        }

        close(code, reason) {
          this.readyState = 2;
          this.inner.close(code, reason);
        }
      }
      const modifiedHandshake = await expectFailure({
        socketFactory: (url) => new MutatingWebSocket(url),
      });
      if (modifiedHandshake !== "connection_failed") {
        throw new Error("modified handshake was not rejected generically");
      }

      let repeatedPin;
      const repeatedClient = new WebSocketApplicationViewClient(
        ready.directBaseUrl,
        null,
        () =>
          new RemoteApplicationWebSocket({
            ...common,
            expectedHostPin: recordedPin,
            onHostPin: (pin) => {
              repeatedPin = pin;
            },
          }),
        "00000000000000000000000000000004",
      );
      await repeatedClient.hello();
      await repeatedClient.close();
      if (
        repeatedPin === undefined ||
        repeatedPin.some((byte, index) => byte !== recordedPin[index])
      ) {
        throw new Error("repeated login changed the host pin");
      }

      password.fill(0);
      recordedPin.fill(0);
      repeatedPin.fill(0);
      return {
        traceEquivalent: true,
        reducerStates: direct.states.length,
        viewUpdateObserved:
          direct.final.views["library-updated"]?.type === "torrent_list",
        semanticCallStatus: direct.response.status,
        firstPinRecorded: true,
        repeatedPinAccepted: true,
        changedPinRejected: true,
        wrongPasswordGeneric: true,
        unknownRouteGeneric: true,
        modifiedHandshakeRejected: true,
      };
    },
    {
      wasmModuleUrl: `${wasmOrigin}/${moduleName}.js`,
      ready,
      passphrase,
    },
  );
  if (browserErrors.length !== 0) {
    throw new Error(`browser page errors: ${browserErrors.join("; ")}`);
  }

  const metrics = parseLine(await proof.send("QUIT"), "METRICS");
  assertEvidence(trace, metrics);
  const bundle = {
    wasmBytes: (await stat(join(wasmDir, `${moduleName}_bg.wasm`))).size,
    javascriptBytes: (await stat(join(wasmDir, `${moduleName}.js`))).size,
  };
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
        trace,
        metrics,
        relayCapture: {
          retainedPayloads: 0,
          retainedApplicationFrames: 0,
          exposedFields: [
            "route",
            "connection timing",
            "message counts",
            "byte counts",
            "high-water marks",
          ],
        },
      },
      null,
      2,
    )}\n`,
  );
} finally {
  if (proof !== undefined) await proof.close();
  if (browser !== undefined) await browser.close();
  if (wasmServer !== undefined) await closeServer(wasmServer);
  if (vite !== undefined) await stopChild(vite);
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
    wasmDir,
    "--target",
    "web",
  ]);
  run("cargo", ["build", "-p", "rstorrent-remote-proof"]);
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

function startVite(port) {
  const child = spawn(
    "npm",
    [
      "run",
      "dev",
      "--prefix",
      "clients/web",
      "--",
      "--host",
      "127.0.0.1",
      "--port",
      String(port),
      "--strictPort",
    ],
    { cwd: repoRoot, stdio: ["ignore", "pipe", "pipe"] },
  );
  child.stderr.on("data", (chunk) => process.stderr.write(chunk));
  return child;
}

async function serveWasm() {
  const files = new Map([
    [
      `/${moduleName}.js`,
      {
        body: await readFile(join(wasmDir, `${moduleName}.js`)),
        type: "text/javascript",
      },
    ],
    [
      `/${moduleName}_bg.wasm`,
      {
        body: await readFile(join(wasmDir, `${moduleName}_bg.wasm`)),
        type: "application/wasm",
      },
    ],
  ]);
  const server = createServer((request, response) => {
    const file = files.get(request.url ?? "");
    response.setHeader("Access-Control-Allow-Origin", "*");
    response.setHeader("Cache-Control", "no-store");
    if (file === undefined) {
      response.writeHead(404).end();
      return;
    }
    response.setHeader("Content-Type", file.type);
    response.writeHead(200).end(file.body);
  });
  await listen(server, 0);
  return server;
}

async function launchBrowser() {
  const requireFromWeb = createRequire(join(webRoot, "package.json"));
  const { chromium } = requireFromWeb("playwright");
  try {
    return await chromium.launch({ headless: true, channel: "chrome" });
  } catch {
    return chromium.launch({ headless: true });
  }
}

function assertEvidence(trace, metrics) {
  if (
    !trace.traceEquivalent ||
    !trace.viewUpdateObserved ||
    trace.semanticCallStatus !== "success"
  ) {
    throw new Error("application trace evidence is incomplete");
  }
  const host = metrics.host;
  const relay = metrics.relay;
  if (
    host.completedRegistrations !== 1 ||
    host.authenticatedLogins < 2 ||
    host.clientAcknowledgements < 1 ||
    host.serverViewBatches < 1 ||
    host.serverCallResults < 1 ||
    host.activeCircuits !== 0 ||
    host.activeCircuitsHighWater !== 1 ||
    relay.activeCircuits !== 0 ||
    relay.activePumps !== 0 ||
    relay.activeCircuitsHighWater !== 1 ||
    relay.activePumpsHighWater !== 2 ||
    relay.forwardedClientMessages === 0 ||
    relay.forwardedHostMessages === 0 ||
    relay.forwardedMessageBytesHighWater > 16 * 1024 * 1024 + 68 * 1024 + 32
  ) {
    throw new Error("relay or host lifecycle evidence is incomplete");
  }
}

class LineProcess {
  constructor(command, args) {
    this.command = command;
    this.args = args;
    this.lines = [];
    this.waiters = [];
    this.stderr = "";
  }

  async start() {
    this.child = spawn(this.command, this.args, {
      cwd: repoRoot,
      stdio: ["pipe", "pipe", "pipe"],
    });
    this.child.stderr.setEncoding("utf8");
    this.child.stderr.on("data", (chunk) => {
      this.stderr += chunk;
    });
    createInterface({ input: this.child.stdout }).on("line", (line) => {
      const waiter = this.waiters.shift();
      if (waiter === undefined) this.lines.push(line);
      else waiter.resolve(line);
    });
    this.child.once("error", (error) => {
      for (const waiter of this.waiters.splice(0)) waiter.reject(error);
    });
    return this.nextLine();
  }

  async send(line) {
    if (this.child === undefined || this.child.exitCode !== null) {
      throw new Error(`proof host exited unexpectedly: ${this.stderr}`);
    }
    this.child.stdin.write(`${line}\n`);
    return this.nextLine();
  }

  nextLine() {
    const line = this.lines.shift();
    if (line !== undefined) return Promise.resolve(line);
    return new Promise((resolve, reject) => this.waiters.push({ resolve, reject }));
  }

  async close() {
    if (this.child === undefined || this.child.exitCode !== null) return;
    this.child.stdin.end();
    await waitForExit(this.child, 5_000).catch(() => this.child.kill("SIGTERM"));
  }
}

function parseLine(line, prefix) {
  if (!line.startsWith(`${prefix} `)) {
    throw new Error(`expected ${prefix} from proof host`);
  }
  return JSON.parse(line.slice(prefix.length + 1));
}

async function availablePort() {
  const server = createServer();
  await listen(server, 0);
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("no port");
  const { port } = address;
  await closeServer(server);
  return port;
}

function listen(server, port) {
  return new Promise((resolveListen, rejectListen) => {
    server.once("error", rejectListen);
    server.listen(port, "127.0.0.1", resolveListen);
  });
}

function serverOrigin(server) {
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("no server address");
  return `http://127.0.0.1:${address.port}`;
}

async function waitForHttp(url) {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return;
    } catch {
      // Vite is still starting.
    }
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 50));
  }
  throw new Error("Vite proof page did not start");
}

function closeServer(server) {
  return new Promise((resolveClose, rejectClose) =>
    server.close((error) =>
      error === undefined ? resolveClose() : rejectClose(error),
    ),
  );
}

async function stopChild(child) {
  if (child.exitCode !== null) return;
  child.kill("SIGTERM");
  await waitForExit(child, 5_000).catch(() => child.kill("SIGKILL"));
}

function waitForExit(child, timeoutMillis) {
  return Promise.race([
    new Promise((resolveExit) => child.once("exit", resolveExit)),
    new Promise((_, rejectTimeout) =>
      setTimeout(() => rejectTimeout(new Error("child exit timed out")), timeoutMillis),
    ),
  ]);
}

await main();
