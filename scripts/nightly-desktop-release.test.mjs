import assert from "node:assert/strict";
import test from "node:test";

import { isDesktopPackageInput, newestVerifiedMainCommit, nightlyIdentity } from "./nightly-desktop-release.mjs";

test("nightly identity uses a greater numeric release train and bounds native versions", () => {
  assert.deepEqual(nightlyIdentity("0.1.4", 12, 2, "a".repeat(40)), {
    version: "0.2.1202",
    tag: "desktop-latest-v0.2.1202",
    sha: "a".repeat(40),
  });
  assert.throws(() => nightlyIdentity("0.1.4", 656, 1, "a".repeat(40)), /bounds/);
  assert.throws(() => nightlyIdentity("0.255.0", 1, 1, "a".repeat(40)), /bounds/);
});

test("selects the newest successful ancestor and skips failed reruns", () => {
  const runs = [
    { head_sha: "new", status: "completed", conclusion: "failure" },
    { head_sha: "new", status: "completed", conclusion: "success" },
    { head_sha: "old", status: "completed", conclusion: "success" },
  ];
  assert.equal(newestVerifiedMainCommit(runs, () => true).head_sha, "old");
});

test("nightly package input filter includes runtime and release inputs", () => {
  assert.equal(isDesktopPackageInput("crates/rstorrent-engine/src/lib.rs"), true);
  assert.equal(isDesktopPackageInput("clients/web/src/inspection/App.tsx"), true);
  assert.equal(isDesktopPackageInput("clients/web/src/inspection/App.test.tsx"), false);
  assert.equal(isDesktopPackageInput("docs/topics/web-ui-design.md"), false);
});
