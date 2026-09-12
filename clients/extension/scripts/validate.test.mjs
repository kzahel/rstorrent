import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  extensionIdFromPublicKey,
  extensionRoot,
  storeExtensionId,
  validateCompanionBuild,
} from "./validate.mjs";

test("the dashboard public key derives the pinned store item ID", () => {
  const manifest = JSON.parse(
    readFileSync(path.join(extensionRoot, "manifest.json"), "utf8"),
  );
  assert.equal(extensionIdFromPublicKey(manifest.key), storeExtensionId);
  assert.equal(storeExtensionId, "gcgoepclopkgijmclmlheafaglmbjlcc");
});

test("a different public key cannot impersonate the pinned store item", () => {
  const replacementKey = Buffer.from("not the dashboard key").toString("base64");
  assert.notEqual(extensionIdFromPublicKey(replacementKey), storeExtensionId);
});

function companionFixture(t, extraSource) {
  const root = mkdtempSync(path.join(os.tmpdir(), "rstorrent-companion-validation-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(path.join(root, "assets"));
  writeFileSync(path.join(root, "companion.html"), '<script src="assets/companion.js"></script>');
  writeFileSync(path.join(root, "assets/companion.css"), "");
  writeFileSync(
    path.join(root, "assets/companion.js"),
    'const endpoint = "http://100.115.92.2:3030/rstorrent/companion/v1";\n' + extraSource,
  );
  return root;
}

test("companion accepts reviewed FormatJS diagnostic links and the ARC socket", (t) => {
  const root = companionFixture(t, `
    const diagnostic = "See https://formatjs.github.io/docs/tooling/babel-plugin";
    const socket = "ws://100.115.92.2:3034/rstorrent/companion/v1";
  `);
  assert.doesNotThrow(() => validateCompanionBuild(root));
});

for (const url of [
  "https://unreviewed.example/resource",
  "https://formatjs.github.io.attacker.example/docs",
  "https://github.com.attacker.example/docs",
  "http://100.115.92.2.attacker.example:3030/",
  "http://evil-100.115.92.2:3030/",
  "wss://formatjs.github.io/socket",
]) {
  test(`companion rejects unreviewed or lookalike host ${url}`, (t) => {
    const root = companionFixture(t, `const unexpected = ${JSON.stringify(url)};`);
    assert.throws(() => validateCompanionBuild(root), /unexpected remote URL:/u);
  });
}

test("reviewed diagnostic links do not permit dynamic code", (t) => {
  const root = companionFixture(t, 'eval("https://formatjs.github.io/docs");');
  assert.throws(() => validateCompanionBuild(root), /dynamic-code syntax/u);
});
