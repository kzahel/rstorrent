import fs from "node:fs/promises";
import path from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

const screenshotDirectory = process.env.RSTORRENT_SCREENSHOT_DIR;

test("wide inspection surface is accessible and drivable", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openScenario(page, "healthy-download", 42_000);

  await expect(page.getByRole("grid", { name: "Torrent library" })).toHaveAttribute(
    "aria-rowcount",
    "4",
  );
  await expect(page.getByRole("grid", { name: "Active peer connections" })).toBeVisible();
  await expect(page.getByText("Big Buck Bunny 1080p surround").first()).toBeVisible();

  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  await capture(page, "rstorrent-demo-wide.png");

  const torrentRows = page
    .getByRole("grid", { name: "Torrent library" })
    .getByRole("row");
  await torrentRows.nth(2).focus();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByText("Sintel 4K open movie").first()).toBeVisible();
});

test("compact tracker recovery remains legible", async ({ page }) => {
  await page.setViewportSize({ width: 920, height: 720 });
  await openScenario(page, "tracker-recovery", 24_000);
  await expect(page.getByRole("grid", { name: "Active peer connections" })).toHaveAttribute(
    "aria-rowcount",
    "15",
  );
  await capture(page, "rstorrent-demo-compact.png");
});

test("removal keeps data by default and exposes destructive intent", async ({
  page,
}) => {
  await page.setViewportSize({ width: 920, height: 720 });
  await openScenario(page, "healthy-download", 42_000);
  const trigger = page.getByRole("button", { name: "Remove", exact: true });
  await trigger.click();
  const dialog = page.getByRole("dialog", { name: "Remove torrent?" });
  const deleteData = dialog.getByRole("checkbox", {
    name: "Also delete downloaded data",
  });
  await expect(deleteData).not.toBeChecked();
  await deleteData.check();
  await expect(dialog.getByRole("alert")).toContainText("cannot be undone");
  const destructive = dialog.getByRole("button", {
    name: "Remove and delete data",
  });
  await destructive.focus();
  await page.keyboard.press("Tab");
  await expect(deleteData).toBeFocused();
  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  await capture(page, "rstorrent-remove-dialog.png");
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await expect(trigger).toBeFocused();

  await trigger.click();
  const retained = page.getByRole("dialog", { name: "Remove torrent?" });
  await expect(retained.getByRole("checkbox")).not.toBeChecked();
  await retained.getByRole("button", { name: "Remove", exact: true }).click();
  await expect(retained).not.toBeVisible();
  await expect(page.getByText("Torrent removed", { exact: true })).toBeVisible();
});

test("phone navigation opens a full detail surface", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openScenario(page, "healthy-download", 42_000);
  const library = page.getByRole("grid", { name: "Torrent library" });
  await expect(library).toBeVisible();
  await library.getByRole("row").filter({ hasText: "Big Buck Bunny" }).click();
  const backButton = page.getByRole("button", { name: "Torrents", exact: true });
  await expect(backButton).toBeVisible();
  await expect(page.getByRole("grid", { name: "Active peer connections" })).toBeVisible();
  await capture(page, "rstorrent-demo-phone.png");
  await backButton.click();
  await expect(library).toBeVisible();
});

test("large collections retain a bounded virtual DOM", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(() => {
    const target = window as Window & { __rstorrentLongTasks?: number[] };
    target.__rstorrentLongTasks = [];
    new PerformanceObserver((entries) => {
      for (const entry of entries.getEntries()) {
        target.__rstorrentLongTasks?.push(entry.duration);
      }
    }).observe({ type: "longtask", buffered: true });
  });
  const openedAt = performance.now();
  await openScenario(page, "large-swarm", 0);
  const initialRenderMs = performance.now() - openedAt;
  const torrents = page.getByRole("grid", { name: "Torrent library" });
  const peers = page.getByRole("grid", { name: "Active peer connections" });
  await expect(torrents).toHaveAttribute("aria-rowcount", "2001");
  await expect(peers).toHaveAttribute("aria-rowcount", "10001");
  expect(await torrents.getByRole("row").count()).toBeLessThanOrEqual(100);
  expect(await peers.getByRole("row").count()).toBeLessThanOrEqual(100);

  const updateStartedAt = performance.now();
  await page.getByRole("button", { name: "+10s" }).click();
  await expect(page.getByLabel("Demo clock 00:10")).toBeVisible();
  const updateRenderMs = performance.now() - updateStartedAt;
  await torrents.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
    element.dispatchEvent(new Event("scroll"));
  });
  await peers.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
    element.dispatchEvent(new Event("scroll"));
  });
  const browserMetrics = await page.evaluate(() => {
    const measuredPerformance = performance as Performance & {
      memory?: { usedJSHeapSize: number };
    };
    const measuredWindow = window as Window & { __rstorrentLongTasks?: number[] };
    const longTasks = measuredWindow.__rstorrentLongTasks ?? [];
    return {
      domElements: document.getElementsByTagName("*").length,
      usedJsHeapBytes: measuredPerformance.memory?.usedJSHeapSize ?? null,
      longTaskCount: longTasks.length,
      longestTaskMs: longTasks.length === 0 ? 0 : Math.max(...longTasks),
      longTaskTotalMs: longTasks.reduce((sum, value) => sum + value, 0),
    };
  });
  expect(browserMetrics.domElements).toBeLessThan(2_000);
  if (browserMetrics.usedJsHeapBytes !== null) {
    expect(browserMetrics.usedJsHeapBytes).toBeLessThan(256 * 1024 * 1024);
  }
  expect(initialRenderMs).toBeLessThan(5_000);
  expect(updateRenderMs).toBeLessThan(5_000);
  console.log(
    `scale_metrics ${JSON.stringify({ initialRenderMs: Math.round(initialRenderMs), updateRenderMs: Math.round(updateRenderMs), ...browserMetrics })}`,
  );
});

async function openScenario(page: Page, scenario: string, at: number) {
  await page.goto(`/?demo=${scenario}&at=${at}&autoplay=0`);
  await expect(page.getByText("RSTorrent", { exact: true })).toBeVisible();
  await expect(page.getByText("Demo data", { exact: true })).toBeVisible();
}

async function capture(page: Page, filename: string) {
  if (screenshotDirectory === undefined) return;
  await fs.mkdir(screenshotDirectory, { recursive: true });
  await page.screenshot({
    path: path.join(screenshotDirectory, filename),
    fullPage: false,
  });
}
