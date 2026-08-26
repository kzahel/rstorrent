#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import fsp from "node:fs/promises";
import http from "node:http";
import https from "node:https";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const archive = parseArchive(process.argv.slice(2));
const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const wsModule = pathToFileURL(
  path.join(repositoryRoot, "clients/web/node_modules/ws/wrapper.mjs"),
).href;
const { WebSocket } = await import(wsModule);
const temporaryRoot = await fsp.mkdtemp(
  path.join(os.tmpdir(), "rstorrent-headless-proxy-"),
);

let proxy;
let service;
try {
  const extracted = path.join(temporaryRoot, "bundle");
  await fsp.mkdir(extracted, { recursive: true, mode: 0o755 });
  await run("tar", ["-xzf", archive, "-C", extracted]);
  const version = (await fsp.readFile(path.join(extracted, "VERSION"), "utf8")).trim();
  const applicationRoot = path.join(temporaryRoot, "application");
  const release = path.join(applicationRoot, "versions", version);
  await fsp.mkdir(path.dirname(release), { recursive: true, mode: 0o755 });
  await fsp.cp(extracted, release, { recursive: true, force: false });
  await normalizeDirectoryModes(release);
  for (const directory of [applicationRoot, path.join(applicationRoot, "versions"), release]) {
    await fsp.chmod(directory, 0o755);
  }
  await fsp.symlink(path.join("versions", version), path.join(applicationRoot, "current"));

  const backendPort = await reservePort();
  const proxyPort = await reservePort();
  const publicOrigin = `https://127.0.0.1:${proxyPort}`;
  const publicHost = `127.0.0.1:${proxyPort}`;
  const username = "owner";
  const password = crypto.randomBytes(24).toString("base64url");
  const authorization = `Basic ${Buffer.from(`${username}:${password}`, "utf8").toString("base64")}`;
  const configDirectory = path.join(temporaryRoot, "config");
  const configPath = path.join(configDirectory, "headless.toml");
  const passwordFile = path.join(configDirectory, "basic-password");
  const profileRoot = path.join(temporaryRoot, "state", "profile");
  const missingPayload = path.join(temporaryRoot, "missing-payload");
  await fsp.mkdir(configDirectory, { recursive: true, mode: 0o700 });
  await fsp.writeFile(passwordFile, `${password}\n`, { mode: 0o600 });
  await fsp.writeFile(
    configPath,
    `version = 1
profile_root = ${JSON.stringify(profileRoot)}
listen = "127.0.0.1:${backendPort}"
public_origin = ${JSON.stringify(publicOrigin)}

[[storage_roots]]
id = "downloads"
label = "Downloads"
path = ${JSON.stringify(missingPayload)}

[authentication]
mode = "basic"
username = ${JSON.stringify(username)}
password_file = ${JSON.stringify(passwordFile)}
`,
    { mode: 0o600 },
  );

  const certificate = await createCertificate(temporaryRoot);
  proxy = await startProxy(certificate, backendPort, proxyPort);
  const executable = path.join(release, "bin", "rstorrent-headless");
  const context = {
    executable,
    configPath,
    publicOrigin,
    publicHost,
    proxyPort,
    authorization,
    password,
    version,
  };

  service = startService(executable, configPath);
  await verifyGeneration(service, context);
  await stopService(service, password);
  service = undefined;
  if (!(await isFile(path.join(profileRoot, "session.db")))) {
    throw new Error("headless generation did not persist its profile database");
  }
  if (await exists(missingPayload)) {
    throw new Error("headless generation recreated the missing payload root");
  }

  service = startService(executable, configPath);
  await verifyGeneration(service, context);
  await stopService(service, password);
  service = undefined;
  if (await exists(missingPayload)) {
    throw new Error("headless restart recreated the missing payload root");
  }

  process.stdout.write(
    `verified headless ${version} HTTPS static, health, HTTP, WSS, exact auth/Host/Origin, zero-child lifetime, and joined restart\n`,
  );
} finally {
  if (service && service.exitCode === null) {
    service.kill("SIGTERM");
    await Promise.race([service.done, delay(5000)]);
  }
  if (proxy) {
    const closed = new Promise((resolve) => proxy.close(resolve));
    proxy.closeAllConnections();
    for (const socket of proxy.fixtureSockets) socket.destroy();
    await Promise.race([closed, delay(2000)]);
  }
  await fsp.rm(temporaryRoot, { recursive: true, force: true });
}
process.exit(0);

async function verifyGeneration(child, context) {
  const healthy = await retry(async () => {
    if (child.exitCode !== null) {
      const output = child.output();
      throw new Error(
        `headless exited before health code=${child.exitCode}: ${output.stderr}`,
      );
    }
    const response = await tlsRequest(context, "/healthz", {
      authorization: context.authorization,
    });
    return response.status === 200 ? response : undefined;
  });
  const health = JSON.parse(healthy.body.toString("utf8"));
  if (
    health.status !== "ok" ||
    health.product !== "rstorrent-headless" ||
    health.build_id !== context.version
  ) {
    throw new Error("headless health returned the wrong product/build identity");
  }

  const missing = await tlsRequest(context, "/", {});
  if (missing.status !== 401 || !missing.headers["www-authenticate"]?.startsWith("Basic ")) {
    throw new Error("missing Basic credential was not challenged before static assets");
  }
  if (
    (await tlsRequest(context, "/healthz", { authorization: "Basic wrong" })).status !== 401
  ) {
    throw new Error("wrong Basic credential reached health");
  }
  if (
    (
      await tlsRequest(context, "/healthz", {
        authorization: context.authorization,
        host: "wrong.example",
      })
    ).status !== 403
  ) {
    throw new Error("wrong Host reached health");
  }
  const index = await tlsRequest(context, "/", { authorization: context.authorization });
  if (index.status !== 200 || !index.body.includes(Buffer.from("<html"))) {
    throw new Error("authenticated static index was unavailable through TLS");
  }

  const helloHeaders = {
    authorization: context.authorization,
    origin: context.publicOrigin,
    owner: "00000000000000000000000000000001",
  };
  const hello = await tlsRequest(context, "/api/v1/hello", helloHeaders);
  if (
    hello.status !== 200 ||
    JSON.parse(hello.body.toString("utf8")).api?.current !== 1
  ) {
    throw new Error("authenticated HTTP application hello failed through TLS");
  }
  if (
    (
      await tlsRequest(context, "/api/v1/hello", {
        ...helloHeaders,
        origin: "https://wrong.example",
      })
    ).status !== 403
  ) {
    throw new Error("wrong HTTP Origin reached the application route");
  }

  if ((await rejectedWebSocketStatus(context, { authorization: "Basic wrong" })) !== 401) {
    throw new Error("wrong Basic credential reached WebSocket upgrade");
  }
  if (
    (await rejectedWebSocketStatus(context, {
      authorization: context.authorization,
      host: "wrong.example",
    })) !== 403
  ) {
    throw new Error("wrong Host reached WebSocket upgrade");
  }
  if (
    (await rejectedWebSocketStatus(context, {
      authorization: context.authorization,
      origin: "https://wrong.example",
    })) !== 403
  ) {
    throw new Error("wrong Origin reached WebSocket upgrade");
  }
  await connectedWebSocket(context);

  const childrenPath = `/proc/${child.pid}/task/${child.pid}/children`;
  const children = (await fsp.readFile(childrenPath, "utf8")).trim();
  if (children !== "") {
    throw new Error(`headless service retained child processes: ${children}`);
  }
  if (child.exitCode !== null) {
    throw new Error(`headless service exited unexpectedly with ${child.exitCode}`);
  }
}

async function connectedWebSocket(context) {
  const socket = new WebSocket(`wss://127.0.0.1:${context.proxyPort}/api/v1/connect`, {
    rejectUnauthorized: false,
    headers: {
      Authorization: context.authorization,
      Origin: context.publicOrigin,
    },
  });
  await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("WebSocket verification timed out")), 5000);
    socket.once("open", () => {
      socket.send(
        JSON.stringify({
          type: "connect",
          api_version: 1,
          encoding: "json",
          client_instance_id: "00000000000000000000000000000001",
        }),
      );
    });
    socket.once("message", (data) => {
      try {
        const frame = JSON.parse(data.toString());
        if (frame.type !== "connected" || frame.api_version !== 1) {
          throw new Error("WebSocket did not negotiate the application contract");
        }
        clearTimeout(timeout);
        socket.close(1000, "fixture detach");
        resolve();
      } catch (error) {
        clearTimeout(timeout);
        reject(error);
      }
    });
    socket.once("error", (error) => {
      clearTimeout(timeout);
      reject(error);
    });
  });
  await new Promise((resolve) => socket.once("close", resolve));
}

async function rejectedWebSocketStatus(context, overrides) {
  const headers = {
    Authorization: overrides.authorization,
    Origin: overrides.origin ?? context.publicOrigin,
  };
  if (overrides.host) headers.Host = overrides.host;
  const socket = new WebSocket(`wss://127.0.0.1:${context.proxyPort}/api/v1/connect`, {
    rejectUnauthorized: false,
    headers,
  });
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      socket.terminate();
      reject(new Error("rejected WebSocket verification timed out"));
    }, 5000);
    socket.once("unexpected-response", (_request, response) => {
      clearTimeout(timeout);
      response.destroy();
      resolve(response.statusCode);
    });
    socket.once("open", () => {
      clearTimeout(timeout);
      socket.terminate();
      reject(new Error("invalid WebSocket request was accepted"));
    });
    socket.once("error", (error) => {
      if (error.message.includes("Unexpected server response")) return;
      clearTimeout(timeout);
      reject(error);
    });
  });
}

function startService(executable, configPath) {
  const child = spawn(executable, ["--config", configPath], {
    env: process.env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => {
    stdout = boundedAppend(stdout, chunk);
  });
  child.stderr.on("data", (chunk) => {
    stderr = boundedAppend(stderr, chunk);
  });
  child.done = new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve({ code, signal }));
  });
  child.output = () => ({ stdout, stderr });
  return child;
}

async function stopService(child, password) {
  child.kill("SIGTERM");
  const result = await Promise.race([
    child.done,
    delay(10000).then(() => {
      throw new Error("headless SIGTERM shutdown timed out");
    }),
  ]);
  const output = child.output();
  if (result.code !== 0 || result.signal !== null) {
    throw new Error(
      `headless shutdown failed code=${result.code} signal=${result.signal}: ${output.stderr}`,
    );
  }
  if (output.stdout.includes(password) || output.stderr.includes(password)) {
    throw new Error("headless output exposed the Basic password");
  }
  if (!output.stderr.includes("headless stopped")) {
    throw new Error("headless shutdown did not report joined completion");
  }
}

async function tlsRequest(context, requestPath, options) {
  return new Promise((resolve, reject) => {
    const headers = { Host: options.host ?? context.publicHost };
    if (options.authorization) headers.Authorization = options.authorization;
    if (options.origin) headers.Origin = options.origin;
    if (options.owner) headers["X-RSTorrent-Owner"] = options.owner;
    const request = https.request(
      {
        hostname: "127.0.0.1",
        port: context.proxyPort,
        path: requestPath,
        method: "GET",
        rejectUnauthorized: false,
        headers,
      },
      (response) => {
        const chunks = [];
        let bytes = 0;
        response.on("data", (chunk) => {
          bytes += chunk.length;
          if (bytes > 1024 * 1024) {
            request.destroy(new Error("TLS response exceeded fixture bound"));
            return;
          }
          chunks.push(chunk);
        });
        response.on("end", () =>
          resolve({
            status: response.statusCode,
            headers: response.headers,
            body: Buffer.concat(chunks),
          }),
        );
      },
    );
    request.setTimeout(5000, () => request.destroy(new Error("TLS request timed out")));
    request.once("error", reject);
    request.end();
  });
}

async function startProxy(certificate, backendPort, proxyPort) {
  const server = https.createServer(
    {
      key: await fsp.readFile(certificate.key),
      cert: await fsp.readFile(certificate.cert),
    },
    (request, response) => {
      const upstream = http.request(
        {
          host: "127.0.0.1",
          port: backendPort,
          method: request.method,
          path: request.url,
          headers: request.headers,
        },
        (upstreamResponse) => {
          response.writeHead(upstreamResponse.statusCode, upstreamResponse.headers);
          upstreamResponse.pipe(response);
        },
      );
      upstream.once("error", () => {
        if (!response.headersSent) response.writeHead(502);
        response.end();
      });
      request.pipe(upstream);
    },
  );
  server.fixtureSockets = new Set();
  server.on("connection", (socket) => {
    server.fixtureSockets.add(socket);
    socket.once("close", () => server.fixtureSockets.delete(socket));
  });
  server.on("upgrade", (request, socket, head) => {
    const upstream = net.connect(backendPort, "127.0.0.1", () => {
      upstream.write(`${request.method} ${request.url} HTTP/${request.httpVersion}\r\n`);
      for (let index = 0; index < request.rawHeaders.length; index += 2) {
        upstream.write(`${request.rawHeaders[index]}: ${request.rawHeaders[index + 1]}\r\n`);
      }
      upstream.write("\r\n");
      if (head.length > 0) upstream.write(head);
      upstream.pipe(socket);
      socket.pipe(upstream);
    });
    server.fixtureSockets.add(upstream);
    upstream.once("close", () => server.fixtureSockets.delete(upstream));
    upstream.once("error", () => socket.destroy());
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(proxyPort, "127.0.0.1", resolve);
  });
  return server;
}

async function createCertificate(root) {
  const key = path.join(root, "proxy-key.pem");
  const cert = path.join(root, "proxy-cert.pem");
  await run("openssl", [
    "req",
    "-x509",
    "-newkey",
    "rsa:2048",
    "-nodes",
    "-keyout",
    key,
    "-out",
    cert,
    "-days",
    "1",
    "-subj",
    "/CN=127.0.0.1",
    "-addext",
    "subjectAltName=IP:127.0.0.1",
  ]);
  await fsp.chmod(key, 0o600);
  return { key, cert };
}

async function reservePort() {
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const port = server.address().port;
  await new Promise((resolve) => server.close(resolve));
  return port;
}

async function retry(callback) {
  let lastError;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const result = await callback();
      if (result !== undefined) return result;
    } catch (error) {
      lastError = error;
    }
    await delay(25);
  }
  throw lastError ?? new Error("headless service did not become healthy");
}

function boundedAppend(value, chunk) {
  const next = value + chunk.toString("utf8");
  return next.length > 1024 * 1024 ? next.slice(-1024 * 1024) : next;
}

async function run(command, arguments_) {
  await new Promise((resolve, reject) => {
    const child = spawn(command, arguments_, { stdio: ["ignore", "ignore", "pipe"] });
    let stderr = "";
    child.stderr.on("data", (chunk) => {
      stderr = boundedAppend(stderr, chunk);
    });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0 && signal === null) resolve();
      else reject(new Error(`${command} failed code=${code} signal=${signal}: ${stderr}`));
    });
  });
}

function parseArchive(arguments_) {
  if (
    arguments_.length !== 2 ||
    arguments_[0] !== "--archive" ||
    !path.isAbsolute(arguments_[1]) ||
    !fs.statSync(arguments_[1]).isFile()
  ) {
    throw new Error("usage: verify-headless-service.mjs --archive ABSOLUTE_TAR_GZ");
  }
  return arguments_[1];
}

async function exists(target) {
  try {
    await fsp.lstat(target);
    return true;
  } catch (error) {
    if (error.code === "ENOENT") return false;
    throw error;
  }
}

async function isFile(target) {
  try {
    return (await fsp.stat(target)).isFile();
  } catch (error) {
    if (error.code === "ENOENT") return false;
    throw error;
  }
}

async function normalizeDirectoryModes(root) {
  const pending = [root];
  while (pending.length > 0) {
    const directory = pending.pop();
    await fsp.chmod(directory, 0o755);
    for (const entry of await fsp.readdir(directory, { withFileTypes: true })) {
      if (entry.isDirectory()) pending.push(path.join(directory, entry.name));
    }
  }
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
