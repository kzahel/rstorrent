import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";

import { applyPresentation, presentationForPlatform } from "../popup/platform.js";
import { extensionRoot } from "./validate.mjs";

const popup = readFileSync(path.join(extensionRoot, "popup/popup.html"), "utf8");

test("ChromeOS presents Android and Crostini without desktop bootstrap", () => {
  assert.deepEqual(presentationForPlatform("cros"), {
    desktop: false,
    chromeos: true,
  });
});

test("desktop platforms present only native desktop bootstrap", () => {
  for (const os of ["linux", "mac", "openbsd", "win"]) {
    assert.deepEqual(presentationForPlatform(os), {
      desktop: true,
      chromeos: false,
    });
  }
});

test("unknown or unavailable platform information retains both recovery surfaces", () => {
  for (const os of [undefined, null, "android", "future-os"]) {
    assert.deepEqual(presentationForPlatform(os), {
      desktop: true,
      chromeos: true,
    });
  }
});

test("presentation hides irrelevant controls", () => {
  const desktop = { hidden: true };
  const chromeos = { hidden: true };
  applyPresentation(presentationForPlatform("cros"), { desktop, chromeos });
  assert.equal(desktop.hidden, true);
  assert.equal(chromeos.hidden, false);

  applyPresentation(presentationForPlatform("mac"), { desktop, chromeos });
  assert.equal(desktop.hidden, false);
  assert.equal(chromeos.hidden, true);
});

test("ChromeOS copy uses only the exact published Android listing", () => {
  assert.match(
    popup,
    /href="https:\/\/play\.google\.com\/store\/apps\/details\?id=com\.jstorrent\.app"/u,
  );
  assert.match(popup, /separate torrent libraries, settings,/u);
  assert.doesNotMatch(popup, /installed|Play (?:is|appears) available/iu);
});
