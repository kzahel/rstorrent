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

test("detail tab geometry and counts do not change with selection", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openScenario(page, "tracker-recovery", 24_000);

  const tabs = page.getByRole("tab");
  await expect(tabs).toHaveCount(10);
  await expect(page.getByRole("tab", { name: "Peers" }).locator("span")).toHaveText("14");
  await expect(page.getByRole("tab", { name: "Trackers" }).locator("span")).toHaveText("2");

  const tabNames = [
    "General",
    "Trackers",
    "Peers",
    "Swarm",
    "Files",
    "Pieces",
    "Disk",
    "Logs",
    "Speed",
    "DHT",
  ];
  for (const width of [1440, 920]) {
    await page.setViewportSize({ width, height: 900 });
    const initialGeometry = await tabGeometry(page);
    for (const name of tabNames) {
      const tab = page.getByRole("tab", { name });
      await tab.click();
      await expect(tab).toHaveAttribute("aria-selected", "true");
      expect(await tabGeometry(page)).toEqual(initialGeometry);
    }
  }
});

test("compact tracker recovery remains legible", async ({ page }) => {
  await page.setViewportSize({ width: 920, height: 720 });
  await openScenario(page, "tracker-recovery", 24_000);
  await expect(page.getByRole("grid", { name: "Active peer connections" })).toHaveAttribute(
    "aria-rowcount",
    "15",
  );
  await page.getByRole("tab", { name: "Trackers" }).click();
  const trackers = page.getByRole("grid", { name: "Torrent trackers" });
  await expect(trackers).toHaveAttribute("aria-rowcount", "3");
  await expect(trackers.getByRole("row").filter({ hasText: "42" })).toContainText(
    "Announce in",
  );
  await expect(trackers.getByText("reannounce wait").first()).toBeVisible();
  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) =>
      violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  await capture(page, "rstorrent-trackers-compact.png");

  await page.setViewportSize({ width: 390, height: 844 });
  await page
    .getByRole("grid", { name: "Torrent library" })
    .getByRole("row")
    .filter({ hasText: "Big Buck Bunny" })
    .click();
  await expect(trackers).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Torrents", exact: true }),
  ).toBeVisible();
  await expect
    .poll(async () =>
      page
        .getByRole("navigation", { name: "Torrent library" })
        .evaluate((element) => Math.round(element.getBoundingClientRect().right)),
    )
    .toBeLessThanOrEqual(0);
  await capture(page, "rstorrent-trackers-phone.png");
});

test("global disk pipeline shows pressure and responsive piece work", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openScenario(page, "slow-disk-pressure", 20_000);
  await page.getByRole("tab", { name: "Disk" }).click();
  await expect(page.getByText("Receive → write → verify")).toBeVisible();
  await expect(page.getByLabel("Disk pressure Backpressured")).toBeVisible();
  await expect(page.getByText("intake paused now")).toBeVisible();
  const pieces = page.getByRole("grid", { name: "Active storage pieces" });
  await expect(pieces).toHaveAttribute("aria-rowcount", "65");
  expect(await pieces.getByRole("row").count()).toBeLessThanOrEqual(100);
  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) =>
      violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  await capture(page, "rstorrent-disk-wide.png");

  await page.setViewportSize({ width: 920, height: 720 });
  await expect(pieces).toBeVisible();
  await capture(page, "rstorrent-disk-compact.png");

  await page.setViewportSize({ width: 390, height: 844 });
  await page
    .getByRole("grid", { name: "Torrent library" })
    .getByRole("row")
    .filter({ hasText: "Big Buck Bunny" })
    .click();
  await expect(
    page.getByRole("button", { name: "Torrents", exact: true }),
  ).toBeVisible();
  await expect(pieces).toBeVisible();
  await expect
    .poll(async () =>
      page
        .getByRole("navigation", { name: "Torrent library" })
        .evaluate((element) => Math.round(element.getBoundingClientRect().right)),
    )
    .toBeLessThanOrEqual(0);
  await capture(page, "rstorrent-disk-phone.png");
});

test("piece canvas shows retry truth and bounds a large torrent", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openScenario(page, "piece-retry", 10_000);
  await page.getByRole("tab", { name: "Pieces" }).click();
  const retryMap = page.getByRole("img", {
    name: /1,055 pieces: 450 verified, 1 active/i,
  });
  await expect(retryMap).toBeVisible();
  await expect(page.getByLabel("Piece state legend")).toBeVisible();
  const retryCanvas = await retryMap.evaluate((element) => {
    const canvas = element as HTMLCanvasElement;
    return {
      width: canvas.width,
      height: canvas.height,
      cssHeight: Math.round(canvas.getBoundingClientRect().height),
    };
  });
  expect(retryCanvas.height).toBeGreaterThan(0);
  expect(retryCanvas.cssHeight).toBeLessThanOrEqual(1_024);
  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) =>
      violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  await capture(page, "rstorrent-pieces-retry-wide.png");

  await page.setViewportSize({ width: 920, height: 720 });
  await expect(retryMap).toBeVisible();
  await capture(page, "rstorrent-pieces-retry-compact.png");

  await openScenario(page, "large-swarm", 0);
  await page.getByRole("tab", { name: "Pieces" }).click();
  const largeMap = page.getByRole("img", {
    name: /250,000 pieces: 135,000 verified, 6 active/i,
  });
  await expect(largeMap).toBeVisible();
  const largeMetrics = await page.evaluate(() => ({
    domElements: document.getElementsByTagName("*").length,
    canvas: [...document.querySelectorAll("canvas")].map((canvas) => ({
      width: canvas.width,
      height: canvas.height,
      cssHeight: Math.round(canvas.getBoundingClientRect().height),
    })),
  }));
  expect(largeMetrics.domElements).toBeLessThan(1_500);
  expect(largeMetrics.canvas).toHaveLength(1);
  expect(largeMetrics.canvas[0]?.cssHeight).toBeLessThanOrEqual(1_024);
  expect(largeMetrics.canvas[0]?.width).toBeLessThanOrEqual(920 * 3);
  await capture(page, "rstorrent-pieces-large.png");

  await page.setViewportSize({ width: 390, height: 844 });
  await page
    .getByRole("grid", { name: "Torrent library" })
    .getByRole("row")
    .nth(1)
    .click();
  await expect(page.getByRole("button", { name: "Torrents", exact: true })).toBeVisible();
  await expect
    .poll(async () =>
      page
        .getByRole("navigation", { name: "Torrent library" })
        .evaluate((element) => Math.round(element.getBoundingClientRect().right)),
    )
    .toBeLessThanOrEqual(0);
  await expect(page.getByRole("tab", { name: "Pieces" })).toBeInViewport();
  await expect(largeMap).toBeVisible();
  await capture(page, "rstorrent-pieces-phone.png");
  console.log(`piece_scale_metrics ${JSON.stringify(largeMetrics)}`);
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

test("full file catalog stays virtualized across wide compact and phone layouts", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const openedAt = performance.now();
  await openScenario(page, "file-progress", 24_000);
  await page.getByRole("tab", { name: "Files" }).click();
  const files = page.getByRole("grid", { name: "Torrent files" });
  await expect(files).toHaveAttribute("aria-rowcount", "4096");
  await expect(page.getByText("1 padding hidden")).toBeVisible();
  expect(await files.getByRole("row").count()).toBeLessThanOrEqual(100);

  const columns = page.getByRole("button", { name: "Columns" }).last();
  await columns.click();
  await page.getByRole("checkbox", { name: "Storage Path" }).check();
  await expect(files.getByRole("columnheader", { name: "Storage Path" })).toBeVisible();
  const nameResize = files.getByRole("separator", { name: "Resize Name column" });
  const initialWidth = Number(await nameResize.getAttribute("aria-valuenow"));
  await nameResize.focus();
  await page.keyboard.press("ArrowRight");
  await expect(nameResize).toHaveAttribute("aria-valuenow", String(initialWidth + 12));
  await columns.click();

  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  const metrics = await page.evaluate(() => {
    const measuredPerformance = performance as Performance & {
      memory?: { usedJSHeapSize: number };
    };
    return {
      domElements: document.getElementsByTagName("*").length,
      usedJsHeapBytes: measuredPerformance.memory?.usedJSHeapSize ?? null,
    };
  });
  const updateStartedAt = performance.now();
  await page.getByRole("button", { name: "+10s" }).click();
  await expect(page.getByLabel("Demo clock 00:34")).toBeVisible();
  const updateRenderMs = Math.round(performance.now() - updateStartedAt);
  expect(metrics.domElements).toBeLessThan(1_500);
  if (metrics.usedJsHeapBytes !== null) {
    expect(metrics.usedJsHeapBytes).toBeLessThan(256 * 1024 * 1024);
  }
  expect(updateRenderMs).toBeLessThan(5_000);
  expect(performance.now() - openedAt).toBeLessThan(5_000);
  await capture(page, "rstorrent-files-wide.png");

  await page.setViewportSize({ width: 920, height: 720 });
  await expect(files).toBeVisible();
  await capture(page, "rstorrent-files-compact.png");

  await page
    .getByRole("grid", { name: "Torrent library" })
    .getByRole("row")
    .filter({ hasText: "Open Movies production archive" })
    .click();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByRole("button", { name: "Torrents", exact: true })).toBeVisible();
  await expect(files).toBeVisible();
  await expect
    .poll(async () =>
      page
        .getByRole("navigation", { name: "Torrent library" })
        .evaluate((element) => Math.round(element.getBoundingClientRect().right)),
    )
    .toBeLessThanOrEqual(0);
  await capture(page, "rstorrent-files-phone.png");
  console.log(`file_scale_metrics ${JSON.stringify({ ...metrics, updateRenderMs })}`);
});

async function openScenario(page: Page, scenario: string, at: number) {
  await page.goto(`/?demo=${scenario}&at=${at}&autoplay=0`);
  await expect(page.getByText("RSTorrent", { exact: true })).toBeVisible();
  await expect(page.getByText("Demo data", { exact: true })).toBeVisible();
}

async function tabGeometry(page: Page) {
  return page.getByRole("tab").evaluateAll((elements) =>
    elements.map((element) => ({
      left: (element as HTMLElement).offsetLeft,
      width: (element as HTMLElement).offsetWidth,
    })),
  );
}

async function capture(page: Page, filename: string) {
  if (screenshotDirectory === undefined) return;
  await fs.mkdir(screenshotDirectory, { recursive: true });
  await page.screenshot({
    path: path.join(screenshotDirectory, filename),
    fullPage: false,
  });
}
