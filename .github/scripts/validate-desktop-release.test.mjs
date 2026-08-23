import assert from "node:assert/strict";
import test from "node:test";

import { validateDesktopRelease } from "./validate-desktop-release.mjs";

const tag = "desktop-v1.2.3";
const repository = "kzahel/rstorrent";
const version = "1.2.3";
const digest = `sha256:${"a".repeat(64)}`;

test("accepts a complete five-target draft", () => {
  assert.equal(validateDesktopRelease({ ...fixture(), tag, repository }).version, version);
});

test("rejects missing platform coverage and external updater URLs", () => {
  const missing = fixture();
  delete missing.latest.platforms["linux-aarch64"];
  assert.throws(
    () => validateDesktopRelease({ ...missing, tag, repository }),
    /missing platform linux-aarch64/,
  );

  const external = fixture();
  external.latest.platforms["windows-x86_64"].url = "https://example.test/update.exe";
  assert.throws(
    () => validateDesktopRelease({ ...external, tag, repository }),
    /unexpected URL/,
  );
});

test("rejects public or unsigned release input", () => {
  const published = fixture();
  published.release.isDraft = false;
  assert.throws(
    () => validateDesktopRelease({ ...published, tag, repository }),
    /remain a draft/,
  );

  const unsigned = fixture();
  unsigned.release.assets[0].digest = null;
  assert.throws(
    () => validateDesktopRelease({ ...unsigned, tag, repository }),
    /missing a GitHub SHA-256 digest/,
  );
});

function fixture() {
  const updaterAssets = {
    "darwin-aarch64": "RSTorrent_aarch64.app.tar.gz",
    "darwin-x86_64": "RSTorrent_x64.app.tar.gz",
    "linux-aarch64": `rstorrent-desktop_${version}_aarch64.AppImage`,
    "linux-x86_64": `rstorrent-desktop_${version}_amd64.AppImage`,
    "windows-x86_64": `RSTorrent_${version}_x64-setup.exe`,
  };
  const names = new Set([
    `RSTorrent_${version}_aarch64.dmg`,
    `RSTorrent_${version}_x64.dmg`,
    `RSTorrent_${version}_x64-setup.exe`,
    `RSTorrent_${version}_x64_en-US.msi`,
    `rstorrent-desktop_${version}_amd64.AppImage`,
    `rstorrent-desktop_${version}_amd64.deb`,
    `rstorrent-desktop-${version}-1.x86_64.rpm`,
    `rstorrent-desktop_${version}_aarch64.AppImage`,
    `rstorrent-desktop_${version}_arm64.deb`,
    `rstorrent-desktop-${version}-1.aarch64.rpm`,
    "latest.json",
  ]);
  for (const name of Object.values(updaterAssets)) {
    names.add(name);
    names.add(`${name}.sig`);
  }
  return {
    release: {
      tagName: tag,
      isDraft: true,
      assets: [...names].map((name) => ({ name, digest })),
    },
    latest: {
      version,
      platforms: Object.fromEntries(
        Object.entries(updaterAssets).map(([platform, name]) => [
          platform,
          {
            signature: "signed-updater-metadata-that-is-long-enough",
            url: `https://github.com/${repository}/releases/download/${tag}/${name}`,
          },
        ]),
      ),
    },
  };
}
