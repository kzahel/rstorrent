import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";

import {
  extensionIdFromPublicKey,
  extensionRoot,
  storeExtensionId,
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
