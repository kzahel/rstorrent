#!/usr/bin/env node
// Controlled packaged-product presentation evidence, never a Play/ARC pass.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";
import { chromium } from "../clients/web/node_modules/playwright-core/index.mjs";

const archive = process.argv[2];
if (!archive) throw new Error("Usage: node scripts/verify-chromeos-onboarding.mjs EXTENSION.zip");
const staging = mkdtempSync(path.join(tmpdir(), "rstorrent-253-browser-"));
let browser;
try {
  execFileSync("unzip", ["-q", path.resolve(archive), "-d", staging]);
  const manifest = JSON.parse(readFileSync(path.join(staging, "manifest.json")));
  const id = createHash("sha256").update(Buffer.from(manifest.key, "base64")).digest("hex").slice(0, 32).replace(/[0-9a-f]/gu, nibble => String.fromCharCode(97 + parseInt(nibble, 16)));
  const origin = `chrome-extension://${id}`;
  for (const scenario of ["denied", "offline", "legacy", "incompatible", "rejected", "expired"]) {
    const profile = mkdtempSync(path.join(tmpdir(), "rstorrent-253-profile-"));
    const context = await chromium.launchPersistentContext(profile, { channel: "chromium", headless: true, args: [`--disable-extensions-except=${staging}`, `--load-extension=${staging}`] });
    browser = context;
    try {
      const page = await context.newPage();
      await page.addInitScript(({ scenario }) => {
        globalThis.chrome = {
          runtime: { getManifest: () => ({ version: "1.1.2" }) },
          permissions: { contains: async () => scenario !== "denied" },
          storage: { local: { get: async () => ({}), set: async () => {} } },
        };
        globalThis.__probes = 0;
        globalThis.fetch = async (url) => {
          globalThis.__probes += 1;
          const parsed = new URL(url);
          if (scenario === "legacy" && parsed.port === "7800") return new Response(JSON.stringify({ port: 7800, protocolVersion: 1, behaviorVersion: 1, version: "1.0.24", paired: true, capabilities: { ioWebSocket: true, controlEvents: true, rootsRead: true, fileOps: true } }));
          if (["incompatible", "rejected", "expired"].includes(scenario) && parsed.port === "3030") {
            if (parsed.pathname.endsWith("/hello")) return new Response(JSON.stringify({ product: "rstorrent", backend: "android", port: 3030, protocol_min: scenario === "incompatible" ? 2 : 1, protocol_max: scenario === "incompatible" ? 2 : 1, nonce: "abcdefghijklmnop", paired: false }));
            if (parsed.pathname.endsWith("/pairing/request")) return new Response(JSON.stringify({ request_id: "abcdefghijklmnop", expires_in_seconds: 120 }));
            if (parsed.pathname.endsWith("/pairing/poll")) return new Response(JSON.stringify({ status: scenario }));
          }
          throw new TypeError("controlled offline fixture");
        };
      }, { scenario });
      await page.goto(`${origin}/companion/companion.html`);
      const retry = page.getByRole("button", { name: "Retry", exact: true });
      if (scenario === "offline") {
        await page.getByRole("button", { name: "Cancel", exact: true }).click();
        await retry.waitFor();
        const probes = await page.evaluate(() => globalThis.__probes);
        await page.waitForTimeout(2200);
        assert.equal(await page.evaluate(() => globalThis.__probes), probes);
        await retry.click();
        await page.getByText("The Android service did not respond within 20 seconds.", { exact: false }).waitFor({ timeout: 25_000 });
      }
      await retry.waitFor({ timeout: 5000 });
      await page.getByRole("button", { name: "Preview connection support context" }).click();
      const report = JSON.parse(await page.locator("#companion-context").inputValue());
      assert.equal(report.app_presence, "unknown"); assert.equal(report.play_availability, "unknown");
      assert.equal(report.category, { denied: "permission_denied", offline: "service_unreachable", legacy: "app_update_required", incompatible: "extension_update_required", rejected: "pairing_rejected", expired: "pairing_expired" }[scenario]);
      if (scenario === "denied") assert.equal(await page.evaluate(() => globalThis.__probes), 0);
      assert.equal(await page.locator("#app").isVisible(), false);
      const popupPromise = context.waitForEvent("page");
      await page.getByRole("link", { name: "Use Linux instead" }).click();
      const help = await popupPromise;
      await help.waitForLoadState();
      assert.match(help.url(), /setup\.html#linux$/u);
      assert.match(await help.locator("#linux").innerText(), /separate Linux library/u);
      assert.match(await help.locator("#linux").innerText(), /unsupported/u);
      await help.getByText("Play is not enabled or setup is unfinished", { exact: true }).click();
      assert.match(await help.locator("details").first().innerText(), /permitted/u);
      await help.getByText("Installation failed, or the app is unavailable", { exact: true }).click();
      assert.match(await help.locator("details").nth(2).innerText(), /cannot determine its cause/u);
      for (const width of [320, 1440]) {
        await help.setViewportSize({ width, height: 900 });
        assert.equal(await help.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
      }
      console.log(`PASS injected packaged journey: ${scenario}, separate Linux guidance, support preview`);
    } finally { await context.close(); browser = undefined; rmSync(profile, { recursive: true, force: true }); }
  }
} finally {
  await browser?.close();
  rmSync(staging, { recursive: true, force: true });
}
