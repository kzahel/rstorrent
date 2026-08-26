import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  createHeadlessReleaseManifest,
  INSTALL_PROTOCOL_VERSION,
  MANIFEST_NAME,
  REPOSITORY,
  RUNTIME,
  SIGNATURE_NAME,
} from "./write-headless-release-manifest.mjs";

function fixture(version = "0.1.0") {
  const directory = mkdtempSync(join(tmpdir(), "rstorrent-headless-manifest-"));
  const x86_64Path = join(
    directory,
    `rstorrent-headless-${version}-linux-x86_64.tar.gz`,
  );
  const aarch64Path = join(
    directory,
    `rstorrent-headless-${version}-linux-aarch64.tar.gz`,
  );
  writeFileSync(x86_64Path, "x86 package bytes\n");
  writeFileSync(aarch64Path, "arm package bytes\n");
  return { aarch64Path, directory, x86_64Path };
}

test("writes the canonical strict two-architecture manifest", (context) => {
  const files = fixture();
  context.after(() => rmSync(files.directory, { recursive: true, force: true }));
  const manifest = createHeadlessReleaseManifest({
    version: "0.1.0",
    sourceCommit: "0123456789abcdef0123456789abcdef01234567",
    ...files,
  });
  assert.match(manifest, /^rstorrent-headless-release-v1\n/u);
  assert.match(manifest, /tag=headless-v0\.1\.0\n/u);
  assert.match(manifest, new RegExp(`repository=${REPOSITORY}\\n`, "u"));
  assert.match(
    manifest,
    new RegExp(`install_protocol=${INSTALL_PROTOCOL_VERSION}\\n`, "u"),
  );
  assert.match(manifest, new RegExp(`runtime=${RUNTIME}\\n`, "u"));
  assert.match(manifest, /x86_64_sha256=[0-9a-f]{64}\n/u);
  assert.match(manifest, /aarch64_sha256=[0-9a-f]{64}\n/u);
  assert.match(
    manifest,
    new RegExp(`manifest_asset=${MANIFEST_NAME}\\n`, "u"),
  );
  assert.match(
    manifest,
    new RegExp(`signature_asset=${SIGNATURE_NAME}\\n$`, "u"),
  );
});

test("rejects prereleases, malformed commits, and misleading assets", (context) => {
  const files = fixture();
  context.after(() => rmSync(files.directory, { recursive: true, force: true }));
  const valid = {
    version: "0.1.0",
    sourceCommit: "0123456789abcdef0123456789abcdef01234567",
    ...files,
  };
  assert.throws(() =>
    createHeadlessReleaseManifest({ ...valid, version: "0.1.0-dev.1" }),
  );
  assert.throws(() =>
    createHeadlessReleaseManifest({ ...valid, version: "0.01.0" }),
  );
  assert.throws(() =>
    createHeadlessReleaseManifest({ ...valid, sourceCommit: "not-a-commit" }),
  );
  assert.throws(() =>
    createHeadlessReleaseManifest({
      ...valid,
      x86_64Path: files.aarch64Path,
    }),
  );
});

test("pins the existing RSTorrent updater trust root", () => {
  const tauri = JSON.parse(
    readFileSync(
      new URL("../../clients/desktop/src-tauri/tauri.conf.json", import.meta.url),
      "utf8",
    ),
  );
  const decodedUpdaterKey = Buffer.from(
    tauri.plugins.updater.pubkey,
    "base64",
  ).toString("utf8");
  const updaterKey = decodedUpdaterKey.trim().split("\n").at(-1);
  const installer = readFileSync(
    new URL("../../website/public/install-headless.sh", import.meta.url),
    "utf8",
  );
  const embeddedKey = installer.match(/^MINISIGN_PUBLIC_KEY="([^"]+)"$/mu)?.[1];
  assert.equal(embeddedKey, updaterKey);
});
