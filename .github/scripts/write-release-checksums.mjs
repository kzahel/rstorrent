#!/usr/bin/env node

import fs from "node:fs";

const [releasePath, outputPath] = process.argv.slice(2);
if (!releasePath || !outputPath) {
  console.error("usage: write-release-checksums.mjs RELEASE_JSON OUTPUT");
  process.exit(2);
}

const release = JSON.parse(fs.readFileSync(releasePath, "utf8"));
const lines = release.assets
  .filter((asset) => asset.name !== "SHA256SUMS" && !asset.name.endsWith(".sig"))
  .map((asset) => {
    const match = /^sha256:([0-9a-f]{64})$/i.exec(asset.digest ?? "");
    if (!match) throw new Error(`missing GitHub SHA-256 digest for ${asset.name}`);
    return `${match[1].toLowerCase()}  ${asset.name}`;
  })
  .sort();

fs.writeFileSync(outputPath, `${lines.join("\n")}\n`);
