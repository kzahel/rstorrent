#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";

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

function walkFiles(relativeRoot) {
  const result = [];
  const pending = [path.join(repositoryRoot, relativeRoot)];
  while (pending.length > 0) {
    const current = pending.pop();
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const absolute = path.join(current, entry.name);
      if (entry.isDirectory()) pending.push(absolute);
      else result.push(absolute);
    }
  }
  return result;
}

async function checkWebCatalog() {
  const catalogPath = "clients/web/src/localization/messages/en.json";
  if (!fs.existsSync(path.join(repositoryRoot, catalogPath))) return;
  const catalog = readJson(catalogPath);
  const compiled = readJson("clients/web/src/localization/generated/en.json");
  const ids = Object.keys(catalog);
  requireUnique(ids, "web message identifiers");
  const parserUrl = pathToFileURL(
    path.join(
      repositoryRoot,
      "clients/web/node_modules/@formatjs/icu-messageformat-parser/index.js",
    ),
  );
  if (!fs.existsSync(fileURLToPath(parserUrl))) {
    fail("web localization dependencies are not installed; run npm install --prefix clients/web");
  }
  const { parse: parseIcu } = await import(parserUrl.href);
  const babelParserUrl = pathToFileURL(
    path.join(repositoryRoot, "clients/web/node_modules/@babel/parser/lib/index.js"),
  );
  const { parse: parseTypescript } = await import(babelParserUrl.href);
  for (const [id, entry] of Object.entries(catalog)) {
    if (!/^[a-z0-9]+(?:[.-][a-z0-9]+)*$/.test(id)) fail(`invalid web message ID ${id}`);
    if (
      typeof entry !== "object" ||
      entry === null ||
      Object.keys(entry).sort().join(",") !== "defaultMessage,description" ||
      typeof entry.defaultMessage !== "string" ||
      entry.defaultMessage.trim() === "" ||
      typeof entry.description !== "string" ||
      entry.description.trim() === ""
    ) {
      fail(`web message ${id} must have nonempty defaultMessage and description`);
    }
    try {
      const ast = parseIcu(entry.defaultMessage);
      if (JSON.stringify(compiled[id]) !== JSON.stringify(ast)) {
        fail(`compiled web message ${id} is stale; run npm run generate:localization --prefix clients/web`);
      }
    } catch (error) {
      fail(`web message ${id} has invalid ICU syntax: ${error.message}`);
    }
  }
  const compiledOrphans = Object.keys(compiled).filter((id) => !(id in catalog));
  if (compiledOrphans.length > 0) fail(`compiled web catalog has orphaned IDs: ${compiledOrphans.join(", ")}`);

  const referenced = new Set();
  const sourceFiles = walkFiles("clients/web/src").filter(
    (file) => /\.tsx?$/.test(file) && !/\.test\.tsx?$/.test(file),
  );
  for (const file of sourceFiles) {
    const source = fs.readFileSync(file, "utf8");
    for (const match of source.matchAll(/(?:localizedMessage|message)\("([^"]+)"/g)) {
      referenced.add(match[1]);
    }
    const relative = path.relative(repositoryRoot, file);
    if (file.endsWith(".tsx")) {
      const inline = findInlineWebCopy(source, parseTypescript);
      if (inline !== undefined) fail(`unclassified JSX product copy in ${relative}: ${inline}`);
    }
  }
  for (const htmlFile of ["index.html", "companion.html", "remote.html"]) {
    const source = fs.readFileSync(path.join(repositoryRoot, "clients/web", htmlFile), "utf8");
    for (const match of source.matchAll(/data-l10n-id="([^"]+)"[^>]*>([^<]*)</g)) {
      const [, id, english] = match;
      referenced.add(id);
      if (catalog[id]?.defaultMessage !== english.trim()) {
        fail(`${htmlFile} fallback for ${id} differs from the English catalog`);
      }
    }
    for (const match of source.matchAll(/data-l10n-failure-id="([^"]+)"/g)) {
      referenced.add(match[1]);
      const fallback = catalog[match[1]]?.defaultMessage;
      if (
        typeof fallback !== "string" ||
        !fs.readFileSync(path.join(repositoryRoot, "clients/web/public/rstorrent-boot.js"), "utf8")
          .includes(JSON.stringify(fallback).slice(1, -1))
      ) {
        fail(`${htmlFile} boot failure for ${match[1]} differs from the English catalog`);
      }
    }
  }
  for (const id of referenced) {
    if (!(id in catalog)) fail(`web source references missing message ${id}`);
  }
  const orphaned = ids.filter((id) => !referenced.has(id));
  if (orphaned.length > 0) fail(`orphaned web messages: ${orphaned.slice(0, 12).join(", ")}`);

  const packageJson = readJson("clients/web/package.json");
  const requiredPins = {
    "react-intl": "10.1.25",
    "@formatjs/icu-messageformat-parser": "3.5.17",
  };
  for (const [dependency, version] of Object.entries(requiredPins)) {
    if (packageJson.dependencies?.[dependency] !== version) {
      fail(`web localization dependency ${dependency} must be pinned to ${version}`);
    }
  }
  if (packageJson.devDependencies?.["@formatjs/cli"] !== "6.16.22") {
    fail("@formatjs/cli must be pinned to 6.16.22");
  }
  console.log(`Web localization catalog is valid (${ids.length} English messages)`);
}

function placeholders(message) {
  return [...message.matchAll(/\{([a-z][a-z0-9_]*)\}/g)].map((match) => match[1]).sort();
}

function checkDesktopCatalog() {
  const catalog = readJson("clients/desktop/src-tauri/locales/en.json");
  const comments = readJson("clients/desktop/src-tauri/locales/en.comments.json");
  const ids = Object.keys(catalog).sort();
  if (JSON.stringify(ids) !== JSON.stringify(Object.keys(comments).sort())) {
    fail("desktop English messages and translator comments have different keys");
  }
  const referenced = new Map();
  for (const file of walkFiles("clients/desktop/src-tauri/src").filter((file) => file.endsWith(".rs"))) {
    const source = fs.readFileSync(file, "utf8");
    for (const match of source.matchAll(/desktop_localization::(text|format)\(\s*"([^"]+)"/g)) {
      const [, kind, id] = match;
      referenced.set(id, kind);
    }
  }
  for (const id of ids) {
    if (!/^[a-z0-9]+(?:[.-][a-z0-9]+)*$/.test(id)) fail(`invalid desktop message ID ${id}`);
    if (typeof catalog[id] !== "string" || catalog[id].trim() === "") {
      fail(`desktop message ${id} is empty`);
    }
    if (typeof comments[id] !== "string" || comments[id].trim() === "") {
      fail(`desktop translator comment ${id} is empty`);
    }
    if (!referenced.has(id)) fail(`orphaned desktop message ${id}`);
    const names = placeholders(catalog[id]);
    if (names.length > 0 && referenced.get(id) !== "format") {
      fail(`desktop message ${id} has placeholders but is not formatted`);
    }
    if (names.length === 0 && referenced.get(id) === "format") {
      fail(`desktop message ${id} is formatted without placeholders`);
    }
  }
  for (const id of referenced.keys()) {
    if (!(id in catalog)) fail(`desktop source references missing message ${id}`);
  }
  console.log(`Desktop native localization catalog is valid (${ids.length} English messages)`);
}

function findInlineWebCopy(source, parseTypescript) {
  const attributeNames = new Set([
    "alt", "aria-description", "aria-label", "data-label", "emptyMessage",
    "label", "placeholder", "title",
  ]);
  const tree = parseTypescript(source, {
    sourceType: "module",
    plugins: ["typescript", "jsx"],
  });
  let found;
  visit(tree);
  return found;

  function visit(node) {
    if (found !== undefined) return;
    if (node.type === "JSXText") {
      const value = node.value.replace(/\s+/g, " ").trim();
      if (value.length > 1 && /[A-Za-z]/.test(value)) found = value;
    } else if (
      node.type === "JSXAttribute" &&
      node.name.type === "JSXIdentifier" &&
      attributeNames.has(node.name.name) &&
      node.value?.type === "StringLiteral" &&
      /[A-Za-z]/.test(node.value.value)
    ) {
      found = node.value.value;
    }
    for (const value of Object.values(node)) {
      if (found !== undefined) return;
      if (Array.isArray(value)) {
        for (const child of value) {
          if (child && typeof child === "object" && typeof child.type === "string") visit(child);
        }
      } else if (value && typeof value === "object" && typeof value.type === "string") {
        visit(value);
      }
    }
  }
}

export async function main() {
  checkPolicy();
  await checkWebCatalog();
  checkDesktopCatalog();
  console.log("Localization policy is valid (shipping locales: en; test: en-XA, ar-XB)");
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    await main();
  } catch (error) {
    console.error(`Localization validation failed: ${error.message}`);
    process.exitCode = 1;
  }
}
