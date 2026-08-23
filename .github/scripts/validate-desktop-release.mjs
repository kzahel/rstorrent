#!/usr/bin/env node

import fs from "node:fs";
import { fileURLToPath } from "node:url";

function fail(message) {
  throw new Error(message);
}

function requireAsset(assetNames, name) {
  if (!assetNames.has(name)) fail(`missing required release asset: ${name}`);
}

function requireMatchingAsset(assetNames, pattern, label) {
  const matches = [...assetNames].filter((name) => pattern.test(name));
  if (matches.length !== 1) {
    fail(`expected exactly one ${label}, found ${matches.length}: ${matches.join(", ")}`);
  }
  return matches[0];
}

export function validateDesktopRelease({ release, latest, tag, repository }) {
  if (!/^desktop-v\d+\.\d+\.\d+$/.test(tag)) {
    fail(`unexpected desktop tag: ${tag}`);
  }
  const version = tag.slice("desktop-v".length);
  if (release.tagName !== tag) {
    fail(`release tag ${release.tagName} does not match ${tag}`);
  }
  if (!release.isDraft) fail("release must remain a draft until validation succeeds");
  if (!Array.isArray(release.assets)) fail("release assets are missing");

  const assetNames = new Set();
  for (const asset of release.assets) {
    if (!asset.name || assetNames.has(asset.name)) {
      fail(`missing or duplicate release asset name: ${asset.name ?? "<empty>"}`);
    }
    assetNames.add(asset.name);
    if (!/^sha256:[0-9a-f]{64}$/i.test(asset.digest ?? "")) {
      fail(`release asset ${asset.name} is missing a GitHub SHA-256 digest`);
    }
  }
  requireAsset(assetNames, "latest.json");

  for (const [pattern, label] of [
    [/_\d+\.\d+\.\d+_aarch64\.dmg$/, "macOS Apple-silicon DMG"],
    [/_\d+\.\d+\.\d+_x64\.dmg$/, "macOS Intel DMG"],
    [/_\d+\.\d+\.\d+_x64-setup\.exe$/, "Windows NSIS installer"],
    [/_\d+\.\d+\.\d+_x64(?:_en-US)?\.msi$/, "Windows MSI installer"],
    [/_\d+\.\d+\.\d+_amd64\.AppImage$/, "Linux x86_64 AppImage"],
    [/_\d+\.\d+\.\d+_amd64\.deb$/, "Linux x86_64 DEB"],
    [/-\d+\.\d+\.\d+-1\.x86_64\.rpm$/, "Linux x86_64 RPM"],
    [/_\d+\.\d+\.\d+_aarch64\.AppImage$/, "Linux ARM64 AppImage"],
    [/_\d+\.\d+\.\d+_arm64\.deb$/, "Linux ARM64 DEB"],
    [/-\d+\.\d+\.\d+-1\.aarch64\.rpm$/, "Linux ARM64 RPM"],
  ]) {
    requireMatchingAsset(assetNames, pattern, label);
  }

  if (latest.version !== version) {
    fail(`latest.json version ${latest.version} does not match ${version}`);
  }
  if (!latest.platforms || typeof latest.platforms !== "object") {
    fail("latest.json platforms are missing");
  }
  const requiredPlatforms = [
    "darwin-aarch64",
    "darwin-x86_64",
    "linux-aarch64",
    "linux-x86_64",
    "windows-x86_64",
  ];
  const expectedUrlPrefix =
    `https://github.com/${repository}/releases/download/${tag}/`;
  for (const platform of requiredPlatforms) {
    const metadata = latest.platforms[platform];
    if (!metadata) fail(`latest.json is missing platform ${platform}`);
    if (typeof metadata.signature !== "string" || metadata.signature.length < 32) {
      fail(`latest.json platform ${platform} has no usable signature`);
    }
    if (
      typeof metadata.url !== "string" ||
      !metadata.url.startsWith(expectedUrlPrefix)
    ) {
      fail(`latest.json platform ${platform} has an unexpected URL: ${metadata.url}`);
    }
    const assetName = decodeURIComponent(metadata.url.slice(expectedUrlPrefix.length));
    requireAsset(assetNames, assetName);
    requireAsset(assetNames, `${assetName}.sig`);
    const expectedSuffix = platform.startsWith("darwin-")
      ? ".app.tar.gz"
      : platform.startsWith("linux-")
        ? ".AppImage"
        : "-setup.exe";
    if (!assetName.endsWith(expectedSuffix)) {
      fail(`updater for ${platform} must use ${expectedSuffix}: ${assetName}`);
    }
  }
  return { version, platforms: requiredPlatforms };
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function parseArguments(argv) {
  const result = {};
  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index];
    const value = argv[index + 1];
    if (!name?.startsWith("--") || value === undefined) {
      fail(`invalid argument near ${name ?? "<end>"}`);
    }
    result[name.slice(2)] = value;
  }
  for (const name of ["release", "latest", "tag", "repository"]) {
    if (!result[name]) fail(`missing --${name}`);
  }
  return result;
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    const args = parseArguments(process.argv.slice(2));
    const result = validateDesktopRelease({
      release: readJson(args.release),
      latest: readJson(args.latest),
      tag: args.tag,
      repository: args.repository,
    });
    console.log(`Validated complete RSTorrent desktop release ${result.version}`);
  } catch (error) {
    console.error(`Desktop release validation failed: ${error.message}`);
    process.exitCode = 1;
  }
}
