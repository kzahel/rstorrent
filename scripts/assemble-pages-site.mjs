#!/usr/bin/env node

import { createHash } from "node:crypto";
import { cp, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "..");
const websiteRoot = join(repositoryRoot, "website/dist");
const sourceRoot = join(repositoryRoot, "clients/web/dist/remote");
const targetRoot = join(websiteRoot, "remote");
const buildId = process.argv[2];
const relayUrl = "wss://relay.rstorrent.com/client";

if (buildId === undefined || buildId.length < 1 || buildId.length > 160) {
  throw new Error("usage: assemble-pages-site.mjs BUILD_ID");
}
if (!(await stat(join(websiteRoot, "index.html"))).isFile()) {
  throw new Error("website must be built before assembling Pages output");
}
if (!(await stat(join(sourceRoot, "index.html"))).isFile()) {
  throw new Error("remote client must be built before assembling Pages output");
}

await rm(targetRoot, { recursive: true, force: true });
await mkdir(targetRoot, { recursive: true });
await cp(sourceRoot, targetRoot, { recursive: true, force: false });

const html = await readFile(join(targetRoot, "index.html"), "utf8");
const references = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map(
  (match) => match[1],
);
if (
  references.length === 0 ||
  references.some((reference) => !reference.startsWith("/remote/assets/"))
) {
  throw new Error("assembled remote HTML has an unexpected asset origin");
}

const files = [];
for (const path of await filesBelow(targetRoot)) {
  const relativePath = relative(targetRoot, path).replaceAll("\\", "/");
  if (relativePath === "build-manifest.json") continue;
  const body = await readFile(path);
  if (/^(?:sw|service-worker)(?:[.-]|$)/i.test(relativePath.split("/").at(-1))) {
    throw new Error(`remote output contains a service worker: ${relativePath}`);
  }
  if (relativePath.endsWith(".js") && /navigator\s*\.\s*serviceWorker/.test(body)) {
    throw new Error(`remote output registers a service worker: ${relativePath}`);
  }
  if (
    relativePath.startsWith("assets/") &&
    !/[.-][A-Za-z0-9_-]{8,}\.(?:css|js|wasm)$/.test(relativePath)
  ) {
    throw new Error(`remote asset is not content-hashed: ${relativePath}`);
  }
  files.push({
    path: relativePath,
    bytes: body.byteLength,
    sha256: createHash("sha256").update(body).digest("hex"),
  });
}
files.sort((left, right) => left.path.localeCompare(right.path));

const javascript = (
  await Promise.all(
    files
      .filter((file) => file.path.endsWith(".js"))
      .map((file) => readFile(join(targetRoot, file.path), "utf8")),
  )
).join("\n");
if (!javascript.includes(buildId) || !javascript.includes(relayUrl)) {
  throw new Error("remote JavaScript does not contain its exact build and relay identity");
}

await writeFile(
  join(targetRoot, "build-manifest.json"),
  `${JSON.stringify(
    {
      schema_version: 1,
      build_id: buildId,
      relay_url: relayUrl,
      files,
    },
    null,
    2,
  )}\n`,
  "utf8",
);
process.stdout.write(
  `Assembled website and remote client ${buildId} with ${files.length} hashed records.\n`,
);

async function filesBelow(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map(async (entry) => {
      const path = join(directory, entry.name);
      return entry.isDirectory() ? filesBelow(path) : [path];
    }),
  );
  return nested.flat();
}
