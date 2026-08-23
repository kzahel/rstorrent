import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

test("writes sorted checksums without detached signatures", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "rstorrent-checksums-"));
  try {
    const release = path.join(directory, "release.json");
    const output = path.join(directory, "SHA256SUMS");
    fs.writeFileSync(
      release,
      JSON.stringify({
        assets: [
          { name: "z.AppImage.sig", digest: `sha256:${"c".repeat(64)}` },
          { name: "z.AppImage", digest: `sha256:${"b".repeat(64)}` },
          { name: "a.dmg", digest: `sha256:${"a".repeat(64)}` },
        ],
      }),
    );
    const script = fileURLToPath(
      new URL("./write-release-checksums.mjs", import.meta.url),
    );
    const result = spawnSync(process.execPath, [script, release, output], {
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(
      fs.readFileSync(output, "utf8"),
      `${"a".repeat(64)}  a.dmg\n${"b".repeat(64)}  z.AppImage\n`,
    );
  } finally {
    fs.rmSync(directory, { recursive: true });
  }
});
