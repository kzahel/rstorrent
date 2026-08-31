#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function fail(message) {
  throw new Error(message);
}

function readJson(relativePath) {
  return JSON.parse(fs.readFileSync(path.join(repositoryRoot, relativePath), "utf8"));
}

function requireUnique(values, label) {
  if (new Set(values).size !== values.length) fail(`${label} contains duplicates`);
}

function checkPolicy() {
  const locales = readJson("localization/locales.json");
  if (locales.sourceLocale !== "en" || locales.fallbackLocale !== "en") {
    fail("English must remain the source and ultimate fallback locale");
  }
  if (JSON.stringify(locales.supportedLocales) !== JSON.stringify(["en"])) {
    fail("the localization-ready slice may advertise only English");
  }
  const testTags = locales.testLocales.map(({ tag }) => tag);
  requireUnique(testTags, "test locale tags");
  for (const required of ["en-XA", "ar-XB"]) {
    if (!testTags.includes(required)) fail(`missing test pseudo-locale ${required}`);
    if (locales.supportedLocales.includes(required)) {
      fail(`test pseudo-locale ${required} must not be a supported locale`);
    }
  }
  for (const [platform, catalog] of Object.entries(locales.catalogs)) {
    if (!fs.existsSync(path.join(repositoryRoot, catalog))) {
      // Catalogs are added surface by surface while Tactical 204 is active.
      console.warn(`Localization catalog pending for ${platform}: ${catalog}`);
    }
  }

  const glossary = readJson("localization/glossary.json");
  const terms = glossary.terms.map(({ term }) => term);
  requireUnique(terms, "glossary terms");
  for (const required of [
    "torrent",
    "tracker",
    "peer",
    "seeding",
    "download root",
    "Normal",
    "Skip",
    "High",
    "checking",
    "paused",
    "completed",
    "remove",
    "delete data",
  ]) {
    if (!terms.includes(required)) fail(`glossary is missing ${required}`);
  }

  const provenance = readJson("localization/provenance.json");
  const sourceRows = provenance.catalogs.filter(({ locale }) => locale === "en");
  requireUnique(sourceRows.map(({ platform }) => platform), "English provenance platforms");
  for (const platform of ["web", "android", "ios"]) {
    const row = sourceRows.find((candidate) => candidate.platform === platform);
    if (!row || !row.source || !row.license || row.reviewState !== "source") {
      fail(`English ${platform} catalog has incomplete provenance`);
    }
  }

  const classification = readJson("localization/source-classification.json");
  requireUnique(classification.rules.map(({ path }) => path), "classification paths");
  for (const rule of classification.rules) {
    if (!fs.existsSync(path.join(repositoryRoot, rule.path))) {
      fail(`classified source path does not exist: ${rule.path}`);
    }
  }
}

export function main() {
  checkPolicy();
  console.log("Localization policy is valid (shipping locales: en; test: en-XA, ar-XB)");
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    main();
  } catch (error) {
    console.error(`Localization validation failed: ${error.message}`);
    process.exitCode = 1;
  }
}
