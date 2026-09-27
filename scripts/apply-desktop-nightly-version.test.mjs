import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { applyNightlyVersion } from "./apply-desktop-nightly-version.mjs";

test("applies the same numeric identity to every packaged version source", (t) => {
  const root = mkdtempSync(join(tmpdir(), "rstorrent-nightly-version-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "clients/web"), { recursive: true });
  mkdirSync(join(root, "clients/desktop/src-tauri"), { recursive: true });
  writeFileSync(join(root, "clients/web/package.json"), '{"version":"0.1.4"}\n');
  writeFileSync(join(root, "clients/web/package-lock.json"), '{"version":"0.1.4","packages":{"":{"version":"0.1.4"}}}\n');
  writeFileSync(join(root, "clients/desktop/src-tauri/tauri.conf.json"), '{"version":"0.1.4"}\n');
  writeFileSync(join(root, "clients/desktop/src-tauri/Cargo.toml"), '[package]\nname = "rstorrent-desktop"\nversion = "0.1.4"\n');
  writeFileSync(join(root, "Cargo.lock"), 'name = "rstorrent-desktop"\nversion = "0.1.4"\n');
  writeFileSync(join(root, "CHANGELOG.md"), '# Changelog\n\n## [0.1.4]\n\n- Stable.\n');
  applyNightlyVersion(root, "0.2.101");
  for (const name of ["clients/web/package.json", "clients/web/package-lock.json", "clients/desktop/src-tauri/tauri.conf.json", "clients/desktop/src-tauri/Cargo.toml", "Cargo.lock", "CHANGELOG.md"]) {
    assert.match(readFileSync(join(root, name), "utf8"), /0\.2\.101/);
  }
  assert.throws(() => applyNightlyVersion(root, "0.2.101-beta"), /Numeric/);
});
