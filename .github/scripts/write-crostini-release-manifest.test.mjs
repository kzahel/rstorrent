import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  createCrostiniReleaseManifest,
  EXTENSION_ID,
  LAUNCH_PROTOCOL_VERSION,
  MANIFEST_NAME,
  REPOSITORY,
  RUNTIME,
  SIGNATURE_NAME,
} from "./write-crostini-release-manifest.mjs";

function fixture(version = "0.1.0") {
  const directory = mkdtempSync(join(tmpdir(), "rstorrent-crostini-manifest-"));
  const x86_64Path = join(
    directory,
    `rstorrent-crostini-${version}-x86_64.tar.gz`,
  );
  const aarch64Path = join(
    directory,
    `rstorrent-crostini-${version}-aarch64.tar.gz`,
  );
  writeFileSync(x86_64Path, "x86 package bytes\n");
  writeFileSync(aarch64Path, "arm package bytes\n");
  return { aarch64Path, directory, x86_64Path };
}

test("writes the canonical strict two-architecture manifest", (context) => {
  const files = fixture();
  context.after(() => rmSync(files.directory, { recursive: true, force: true }));
  const manifest = createCrostiniReleaseManifest({
    version: "0.1.0",
    sourceCommit: "0123456789abcdef0123456789abcdef01234567",
    ...files,
  });
  assert.match(manifest, /^rstorrent-crostini-release-v1\n/u);
  assert.match(manifest, /tag=crostini-v0\.1\.0\n/u);
  assert.match(manifest, new RegExp(`repository=${REPOSITORY}\\n`, "u"));
  assert.match(
    manifest,
    new RegExp(`launch_protocol=${LAUNCH_PROTOCOL_VERSION}\\n`, "u"),
  );
  assert.match(manifest, new RegExp(`extension_id=${EXTENSION_ID}\\n`, "u"));
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
  assert.throws(() => createCrostiniReleaseManifest({ ...valid, version: "0.1.0-dev.1" }));
  assert.throws(() => createCrostiniReleaseManifest({ ...valid, version: "0.01.0" }));
  assert.throws(() => createCrostiniReleaseManifest({ ...valid, sourceCommit: "not-a-commit" }));
  assert.throws(() =>
    createCrostiniReleaseManifest({ ...valid, x86_64Path: files.aarch64Path }),
  );
});

test("pins the exact updater trust root and store extension identity", () => {
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
    new URL("../../website/public/install-crostini.sh", import.meta.url),
    "utf8",
  );
  const embeddedKey = installer.match(/^MINISIGN_PUBLIC_KEY="([^"]+)"$/mu)?.[1];
  assert.equal(embeddedKey, updaterKey);

  const extension = JSON.parse(
    readFileSync(
      new URL("../../clients/extension/manifest.json", import.meta.url),
      "utf8",
    ),
  );
  const digest = createHash("sha256")
    .update(Buffer.from(extension.key, "base64"))
    .digest()
    .subarray(0, 16);
  const derivedId = [...digest]
    .flatMap((byte) => [byte >> 4, byte & 0x0f])
    .map((nibble) => String.fromCharCode("a".charCodeAt(0) + nibble))
    .join("");
  assert.equal(derivedId, EXTENSION_ID);
});
