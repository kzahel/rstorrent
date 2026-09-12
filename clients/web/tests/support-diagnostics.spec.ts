import fs from "node:fs/promises";
import path from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

for (const width of [320, 1440]) {
  for (const theme of ["light", "dark"]) {
    test(`support preview, copy and download at ${width}px ${theme}`, async ({ page, context }) => {
      await page.setViewportSize({ width, height: 900 });
      await context.grantPermissions(["clipboard-read", "clipboard-write"]);
      await page.addInitScript(() => {
        const active = new Set<string>();
        const create = URL.createObjectURL.bind(URL);
        const revoke = URL.revokeObjectURL.bind(URL);
        URL.createObjectURL = (value) => { const url = create(value); active.add(url); return url; };
        URL.revokeObjectURL = (url) => { active.delete(url); revoke(url); };
        Object.defineProperty(window, "supportObjectUrlCount", { get: () => active.size });
      });
      const external: string[] = [];
      page.on("request", (request) => {
        const url = new URL(request.url());
        if (url.protocol.startsWith("http") && url.hostname !== "127.0.0.1") external.push(url.origin);
      });
      await page.goto(`/tests/fixtures/support-harness.html?theme=${theme}`);
      await page.getByRole("button", { name: "Prepare diagnostics" }).click();
      const preview = page.getByRole("textbox", { name: "Exact report to copy or download" });
      const bytes = await preview.inputValue();
      expect(JSON.parse(bytes).version).toBe("0.1.3");
      expect(Buffer.byteLength(bytes)).toBeLessThan(2048);
      await page.getByRole("button", { name: "Copy diagnostics" }).click();
      await expect(page.getByRole("status")).toHaveText("Diagnostics copied.");
      expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(bytes);
      const pending = page.waitForEvent("download");
      await page.getByRole("button", { name: "Download diagnostics" }).click();
      const download = await pending;
      expect(download.suggestedFilename()).toBe("rstorrent-diagnostics.json");
      const file = await download.path();
      expect(file).not.toBeNull();
      expect(await fs.readFile(file!, "utf8")).toBe(bytes);
      await download.delete();
      await expect.poll(() => page.evaluate(() =>
        (window as unknown as { supportObjectUrlCount: number }).supportObjectUrlCount,
      )).toBe(0);
      expect(external).toEqual([]);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
      expect(overflow).toBe(false);
      for (const name of ["Copy diagnostics", "Download diagnostics"]) {
        const bounds = await page.getByRole("button", { name }).boundingBox();
        expect(bounds!.x).toBeGreaterThanOrEqual(0);
        expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(width);
      }
      expect((await new AxeBuilder({ page }).analyze()).violations.filter(
        (violation) => violation.impact === "serious" || violation.impact === "critical",
      )).toEqual([]);
      if (process.env.RSTORRENT_SCREENSHOT_DIR) {
        await fs.mkdir(process.env.RSTORRENT_SCREENSHOT_DIR, { recursive: true });
        await page.screenshot({ path: path.join(process.env.RSTORRENT_SCREENSHOT_DIR, `support-${width}-${theme}.png`), fullPage: true });
      }
    });
  }
}
