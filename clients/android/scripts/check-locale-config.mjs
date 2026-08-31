#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const androidRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = path.resolve(androidRoot, "../..");
const localePolicy = JSON.parse(
  fs.readFileSync(path.join(repositoryRoot, "localization/locales.json"), "utf8"),
);
const shippingTranslations = localePolicy.supportedLocales.filter(
  (locale) => locale !== localePolicy.sourceLocale,
);
const manifestPath = path.join(androidRoot, "app/src/main/AndroidManifest.xml");
const configPath = path.join(androidRoot, "app/src/main/res/xml/locales_config.xml");
const manifest = fs.readFileSync(manifestPath, "utf8");

if (process.argv.includes("--generate")) {
  if (shippingTranslations.length === 0) {
    console.error("English-only releases deliberately omit Android's per-app language picker.");
    process.exitCode = 2;
  } else {
    const entries = localePolicy.supportedLocales
      .map((locale) => `    <locale android:name="${locale}" />`)
      .join("\n");
    fs.writeFileSync(
      configPath,
      `<?xml version="1.0" encoding="utf-8"?>\n` +
        `<locale-config xmlns:android="http://schemas.android.com/apk/res/android">\n` +
        `${entries}\n</locale-config>\n`,
    );
    console.log(`Generated ${path.relative(repositoryRoot, configPath)}`);
  }
} else if (shippingTranslations.length === 0) {
  if (manifest.includes("android:localeConfig") || fs.existsSync(configPath)) {
    throw new Error("English-only Android must not advertise an application language picker");
  }
  console.log("Android locale configuration is valid (English-only picker omitted)");
} else {
  if (!manifest.includes('android:localeConfig="@xml/locales_config"')) {
    throw new Error("AndroidManifest.xml must reference @xml/locales_config");
  }
  if (!fs.existsSync(configPath)) {
    throw new Error("run clients/android/scripts/check-locale-config.mjs --generate");
  }
  const config = fs.readFileSync(configPath, "utf8");
  for (const locale of localePolicy.supportedLocales) {
    if (!config.includes(`android:name="${locale}"`)) {
      throw new Error(`Android locale config is missing ${locale}`);
    }
  }
  console.log(`Android locale configuration is valid (${localePolicy.supportedLocales.join(", ")})`);
}
