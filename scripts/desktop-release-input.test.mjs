import assert from "node:assert/strict";
import test from "node:test";

import { resolveDesktopReleaseInput } from "./desktop-release-input.mjs";

test("a scheduled reusable call resolves the exact verified Latest identity", () => {
  const sha = "a".repeat(40);
  assert.deepEqual(resolveDesktopReleaseInput({
    event: "schedule", ref: "refs/heads/main", sha: "b".repeat(40),
    sourceSha: sha, version: "0.2.101", tag: "desktop-latest-v0.2.101",
  }), {
    sourceSha: sha, version: "0.2.101", tag: "desktop-latest-v0.2.101", channel: "latest", publish: true,
  });
});

test("stable tags publish while manual rehearsals do not", () => {
  const sha = "a".repeat(40);
  assert.deepEqual(resolveDesktopReleaseInput({ event: "push", ref: "refs/tags/desktop-v0.1.4", sha }), {
    sourceSha: sha, version: "0.1.4", tag: "desktop-v0.1.4", channel: "stable", publish: true,
  });
  assert.equal(resolveDesktopReleaseInput({ event: "workflow_dispatch", ref: "refs/heads/main", sha }).publish, false);
  assert.throws(() => resolveDesktopReleaseInput({ event: "push", ref: "refs/tags/desktop-vbad", sha }), /Invalid Stable/);
  assert.throws(() => resolveDesktopReleaseInput({ event: "schedule", ref: "refs/heads/main", sha, sourceSha: sha, version: "0.2.101", tag: "desktop-v0.2.101" }), /matching Latest tag/);
});
