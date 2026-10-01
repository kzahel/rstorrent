import assert from "node:assert/strict";
import { cpSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

import {
  RELEASE_LANES,
  assembleDesktopRelease,
  stageDesktopReleaseLeg,
  verifyUploadedAssets,
} from "./desktop-release-artifacts.mjs";
import { validateDesktopRelease } from "./validate-desktop-release.mjs";

const version = "1.2.3";
const sourceSha = "a".repeat(40);
const runId = "77";
const attempt = "1";
const repository = "kzahel/rstorrent";
const signature = Buffer.from("untrusted comment: fixture signature\nfixture\n").toString("base64");

function fixture(product = "RSTorrent") {
  const root = mkdtempSync(join(tmpdir(), "rstorrent-parallel-release-"));
  const input = join(root, "input");
  const output = join(root, "output");
  mkdirSync(input);
  const config = join(root, "clients/desktop/src-tauri/tauri.conf.json");
  mkdirSync(dirname(config), { recursive: true });
  writeFileSync(config, JSON.stringify({ version }));
  writeFileSync(join(root, "CHANGELOG.md"), `# Changelog\n\n## [${version}]\n\n- Fixed test release.\n\n## [1.2.2]\n\n- Older.\n`);
  const packages = {
    "macos-aarch64": [
      [`target/aarch64-apple-darwin/release/bundle/dmg/RSTorrent_${version}_aarch64.dmg`, false],
      ["target/aarch64-apple-darwin/release/bundle/macos/RSTorrent.app.tar.gz", true],
    ],
    "macos-x86_64": [
      [`target/x86_64-apple-darwin/release/bundle/dmg/RSTorrent_${version}_x64.dmg`, false],
      ["target/x86_64-apple-darwin/release/bundle/macos/RSTorrent.app.tar.gz", true],
    ],
    "linux-aarch64": [
      [`target/release/bundle/appimage/RSTorrent_${version}_aarch64.AppImage`, true],
      [`target/release/bundle/deb/RSTorrent_${version}_arm64.deb`, true],
      [`target/release/bundle/rpm/RSTorrent-${version}-1.aarch64.rpm`, true],
    ],
    "linux-x86_64": [
      [`target/release/bundle/appimage/RSTorrent_${version}_amd64.AppImage`, true],
      [`target/release/bundle/deb/RSTorrent_${version}_amd64.deb`, true],
      [`target/release/bundle/rpm/RSTorrent-${version}-1.x86_64.rpm`, true],
    ],
    "windows-x86_64": [
      [`target/release/bundle/nsis/RSTorrent_${version}_x64-setup.exe`, true],
      [`target/release/bundle/msi/RSTorrent_${version}_x64_en-US.msi`, true],
    ],
  };
  async function stage() {
    for (const lane of RELEASE_LANES) {
      for (const [originalPath, signed] of packages[lane]) {
        const path = originalPath.replaceAll("RSTorrent", product);
        const file = join(root, path);
        mkdirSync(dirname(file), { recursive: true });
        writeFileSync(file, `package ${lane} ${path}`);
        if (signed) writeFileSync(`${file}.sig`, signature);
      }
      await stageDesktopReleaseLeg({
        root, lane, sourceSha, runId, attempt, expectedVersion: version, product,
        output: join(input, `desktop-release-${lane}-${runId}-${attempt}`),
      });
    }
  }
  const assemble = (overrides = {}) => assembleDesktopRelease({
    root, input, output, sourceSha, runId, attempt, repository,
    channel: "latest", version, tag: `desktop-latest-v${version}`,
    now: new Date("2026-09-27T12:00:00.000Z"), ...overrides,
  });
  return { root, input, output, stage, assemble };
}

test("collects independent signed legs into one complete Latest manifest", async () => {
  const data = fixture();
  try {
    await data.stage();
    const result = await data.assemble();
    assert.equal(result.release.assets.length, 23);
    assert.equal(Object.keys(result.latest.platforms).length, 15);
    assert.equal(result.latest.platforms["windows-x86_64"].signature, signature);
    assert.equal(result.latest.pub_date, "2026-09-27T12:00:00.000Z");
    assert.equal(validateDesktopRelease({ ...result, tag: `desktop-latest-v${version}`, repository }).version, version);
    assert.equal(readFileSync(join(data.output, "assets/latest.json"), "utf8").includes("desktop-latest-v1.2.3"), true);
    verifyUploadedAssets(result.release, structuredClone(result.release));
  } finally {
    rmSync(data.root, { recursive: true, force: true });
  }
});

test("rehearsal assembles the same matrix without a published tag", async () => {
  const data = fixture();
  try {
    await data.stage();
    const result = await data.assemble({ channel: "stable", version: "", tag: "" });
    assert.equal(result.release.tagName, `desktop-v${version}`);
    assert.equal(result.release.isPrerelease, false);
    assert.equal(result.latest.notes, "Signed RSTorrent desktop release rehearsal.");
    assert.equal(validateDesktopRelease({ ...result, tag: `desktop-v${version}`, repository }).version, version);
  } finally {
    rmSync(data.root, { recursive: true, force: true });
  }
});

test("rejects stale, incomplete, duplicated, and altered release legs", async () => {
  for (const mutation of [
    (data) => writeFileSync(join(data.input, `desktop-release-linux-x86_64-${runId}-${attempt}`, `RSTorrent_${version}_amd64.AppImage`), "altered"),
    (data) => rmSync(join(data.input, `desktop-release-linux-x86_64-${runId}-${attempt}`), { recursive: true }),
    (data) => {
      const old = join(data.input, `desktop-release-linux-x86_64-${runId}-${attempt}`);
      const duplicate = join(data.input, `desktop-release-windows-x86_64-${runId}-${attempt}`);
      rmSync(duplicate, { recursive: true });
      cpSync(old, duplicate, { recursive: true });
    },
    (data) => {
      const file = join(data.input, `desktop-release-windows-x86_64-${runId}-${attempt}`, "meta.json");
      const meta = JSON.parse(readFileSync(file, "utf8"));
      meta.sourceSha = "b".repeat(40);
      writeFileSync(file, JSON.stringify(meta));
    },
  ]) {
    const data = fixture();
    try {
      await data.stage();
      mutation(data);
      await assert.rejects(data.assemble(), /release leg|release lane|expected exactly/);
    } finally {
      rmSync(data.root, { recursive: true, force: true });
    }
  }
});

test("rejects mismatched GitHub release assets before publication", async () => {
  const data = fixture();
  try {
    await data.stage();
    const { release } = await data.assemble();
    const remote = structuredClone(release);
    remote.assets[0].digest = `sha256:${"f".repeat(64)}`;
    assert.throws(() => verifyUploadedAssets(release, remote), /differs from assembled bytes/);
    remote.assets = remote.assets.slice(1);
    assert.throws(() => verifyUploadedAssets(release, remote), /asset count mismatch/);
  } finally {
    rmSync(data.root, { recursive: true, force: true });
  }
});

test("refuses a leg with an absent updater signature or wrong version", async () => {
  const data = fixture();
  try {
    mkdirSync(join(data.root, "target/release/bundle/nsis"), { recursive: true });
    writeFileSync(join(data.root, `target/release/bundle/nsis/RSTorrent_${version}_x64-setup.exe`), "installer");
    await assert.rejects(
      stageDesktopReleaseLeg({
        root: data.root, lane: "windows-x86_64", sourceSha, runId, attempt,
        expectedVersion: "1.2.4", output: join(data.input, "wrong-version"),
      }),
      /package version .* does not match/,
    );
    await assert.rejects(
      stageDesktopReleaseLeg({
        root: data.root, lane: "windows-x86_64", sourceSha, runId, attempt,
        expectedVersion: version, output: join(data.input, "missing-signature"),
      }),
      /missing release file/,
    );
  } finally {
    rmSync(data.root, { recursive: true, force: true });
  }
});

test("JSTorrent candidates use exact product receipts and cannot publish", async () => {
  const data=fixture("JSTorrent");
  try {
    await data.stage();
    const options={root:data.root,input:data.input,output:data.output,sourceSha,runId,attempt,repository,channel:"stable",version,product:"JSTorrent"};
    const result=await assembleDesktopRelease(options);
    validateDesktopRelease({...result,tag:result.release.tagName,repository});
    assert(Object.values(result.latest.platforms).every(p=>p.url.includes("/JSTorrent")));
    assert(Object.values(result.latest.platforms).every(p=>!p.url.includes("RSTorrent")));
    await assert.rejects(assembleDesktopRelease({...options,tag:`desktop-v${version}`,output:join(data.root,"forbidden")}),/publication is not enabled/u);
    const directory=join(data.input,`desktop-release-${RELEASE_LANES[0]}-${runId}-${attempt}`);
    const meta=JSON.parse(readFileSync(join(directory,"meta.json"),"utf8"));
    meta.product="RSTorrent";writeFileSync(join(directory,"meta.json"),JSON.stringify(meta));
    await assert.rejects(assembleDesktopRelease({...options,output:join(data.root,"mixed")}),/mixed release products/u);
  } finally {rmSync(data.root,{recursive:true,force:true});}
});
