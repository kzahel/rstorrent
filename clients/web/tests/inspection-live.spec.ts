import fs from "node:fs/promises";
import path from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";

const gateway = process.env.RSTORRENT_LIVE_GATEWAY_URL;
const magnet = process.env.RSTORRENT_LIVE_MAGNET;
const torrentId = process.env.RSTORRENT_LIVE_TORRENT_ID;
const torrentName = process.env.RSTORRENT_LIVE_TORRENT_NAME;
const fileCount = process.env.RSTORRENT_LIVE_FILE_COUNT;
const screenshotDirectory = process.env.RSTORRENT_SCREENSHOT_DIR;

test("live peer inspection follows a controlled verified transfer", async ({
  page,
}) => {
  test.setTimeout(60_000);
  test.skip(
    gateway === undefined ||
      magnet === undefined ||
      torrentId === undefined ||
      torrentName === undefined ||
      fileCount === undefined,
    "controlled live gateway is opt-in",
  );
  const viewSetIds: string[] = [];
  let openAttempts = 0;
  let suspendUpdates = false;
  let releaseUpdates = () => {};
  let updatesReleased = Promise.resolve();
  let delayNextCommand = true;
  await page.route(`${gateway!}/api/v1/view-sets`, async (route) => {
    if (route.request().method() === "POST") {
      openAttempts += 1;
      if (openAttempts > 1) await new Promise((resolve) => setTimeout(resolve, 350));
    }
    await route.continue();
  });
  await page.route("**/api/v1/view-sets/*/updates?**", async (route) => {
    if (suspendUpdates) await updatesReleased;
    await route.continue();
  });
  await page.route(`${gateway!}/api/v1/commands`, async (route) => {
    if (delayNextCommand) {
      delayNextCommand = false;
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    await route.continue();
  });
  page.on("response", (response) => {
    if (
      response.request().method() === "POST" &&
      response.url() === `${gateway!}/api/v1/view-sets` &&
      response.status() === 201
    ) {
      void response.json().then((body: { view_set_id?: string }) => {
        if (body.view_set_id !== undefined) viewSetIds.push(body.view_set_id);
      });
    }
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto(`/?live=${encodeURIComponent(gateway!)}&poll_ms=100`);
  await expect(page.getByText("Live engine", { exact: true })).toBeVisible();
  await expect
    .poll(async () => {
      const mainBottom = await page
        .locator("#app main")
        .evaluate((element) => Math.round(element.getBoundingClientRect().bottom));
      const detailBottom = await page
        .locator('section[aria-label="Torrent details"]')
        .evaluate((element) => Math.round(element.getBoundingClientRect().bottom));
      return { mainBottom, detailBottom };
    })
    .toEqual({ mainBottom: 900, detailBottom: 900 });

  const moreButton = page.getByRole("button", { name: "More", exact: true });
  await moreButton.focus();
  await page.keyboard.press("ArrowDown");
  const addTestTorrent = page.getByRole("menuitem", {
    name: "Add test torrent",
  });
  await expect(addTestTorrent).toBeFocused();
  await page.keyboard.press("ArrowRight");
  const testTorrentMenu = page.getByRole("menu", {
    name: "Add test torrent",
  });
  await expect(testTorrentMenu.getByRole("menuitem")).toHaveCount(5);
  await expect(
    testTorrentMenu.getByRole("menuitem", { name: "Big Buck Bunny" }),
  ).toBeFocused();
  await capture(page, "live-test-torrent-menu-wide.png");
  const menuViolations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(menuViolations).toEqual([]);
  await page.keyboard.press("Escape");
  await expect(testTorrentMenu).not.toBeVisible();
  await page.keyboard.press("Escape");
  await expect(moreButton).toBeFocused();

  const addForm = page.getByRole("form", { name: "Add torrent" });
  const torrentInput = addForm.getByRole("textbox", {
    name: "Magnet link or torrent URL",
  });
  const addButton = addForm.getByRole("button");
  await torrentInput.fill("https://example.test/file.torrent");
  await addButton.click();
  await expect(torrentInput).toHaveAttribute("aria-invalid", "true");
  await expect(torrentInput).toHaveValue(
    "https://example.test/file.torrent",
  );
  await expect(
    page.getByText(
      "Remote .torrent URLs are not supported yet. Paste a magnet link instead.",
      { exact: true },
    ),
  ).toBeVisible();

  await torrentInput.fill(magnet!);
  await torrentInput.press("Enter");
  await expect(addButton).toBeDisabled();
  await expect(moreButton).toBeDisabled();
  await expect(page.getByText("Torrent added", { exact: true })).toBeVisible();
  await expect(torrentInput).toHaveValue("");

  const library = page.getByRole("grid", { name: "Torrent library" });
  const torrentRow = library.locator(`[data-row-id="${torrentId!}"]`);
  await expect(torrentRow).toBeVisible({ timeout: 10_000 });

  await page.setViewportSize({ width: 390, height: 844 });
  const menuButton = page.getByRole("button", {
    name: "Toggle library navigation",
  });
  await expect(menuButton).toBeVisible();
  if ((await menuButton.getAttribute("aria-expanded")) === "true") {
    await menuButton.click();
    await page.waitForTimeout(250);
  }
  await expect(menuButton).toHaveAttribute("aria-expanded", "false");
  await expect
    .poll(async () =>
      page
        .getByRole("navigation", { name: "Torrent library" })
        .evaluate((element) => Math.round(element.getBoundingClientRect().right)),
    )
    .toBeLessThanOrEqual(0);
  await expect(torrentInput).toBeVisible();
  await expect(addButton).toBeVisible();
  await capture(page, "live-magnet-phone-library.png");
  await moreButton.click();
  await page.getByRole("menuitem", { name: "Add test torrent" }).click();
  await expect(testTorrentMenu.getByRole("menuitem")).toHaveCount(5);
  await capture(page, "live-test-torrent-menu-phone.png");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");

  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(torrentRow).toContainText(torrentName!, { timeout: 20_000 });
  await torrentRow.click();
  await page.getByRole("tab", { name: "General" }).click();
  await expect(
    page.getByRole("tabpanel").getByRole("heading", { name: torrentName! }),
  ).toBeVisible();

  const transferStartedAt = performance.now();
  await page.getByRole("tab", { name: "Files" }).click();
  const files = page.getByRole("grid", { name: "Torrent files" });
  const expectedFileCount = Number(fileCount!);
  await expect(files).toHaveAttribute("aria-rowcount", String(expectedFileCount + 1), {
    timeout: 20_000,
  });
  await scrollToEnd(files);
  const prefixFile = files.getByRole("row").filter({ hasText: "prefix.bin" });
  const payloadFile = files.getByRole("row").filter({ hasText: "payload.bin" });
  await expect(prefixFile).toBeVisible();
  await expect(payloadFile).toBeVisible();
  const payloadCells = payloadFile.getByRole("gridcell");
  await expect
    .poll(async () => (await payloadCells.nth(5).textContent())?.trim(), {
      timeout: 12_000,
    })
    .not.toBe("0 B");
  const firstDoneMs = Math.round(performance.now() - transferStartedAt);
  await expect
    .poll(async () => (await payloadCells.nth(6).textContent())?.trim(), {
      timeout: 12_000,
    })
    .not.toBe("0 B");
  const firstVerifiedMs = Math.round(performance.now() - transferStartedAt);
  await capture(page, "live-files-progress-wide.png");

  updatesReleased = new Promise<void>((resolve) => {
    releaseUpdates = resolve;
  });
  suspendUpdates = true;
  await new Promise((resolve) => setTimeout(resolve, 1_000));
  suspendUpdates = false;
  releaseUpdates();
  await expect(page.getByText("reconnecting", { exact: true })).toBeVisible();
  await expect(payloadFile).toBeVisible();
  await capture(page, "live-files-reconnecting.png");
  await expect(page.getByText("connected", { exact: true })).toBeVisible();
  await expect.poll(() => new Set(viewSetIds).size).toBeGreaterThan(1);

  await page.getByRole("tab", { name: "Peers" }).click();

  const peers = page.getByRole("grid", { name: "Active peer connections" });
  await expect(peers).toBeVisible();
  await expect
    .poll(async () => Number(await peers.getAttribute("aria-rowcount")))
    .toBeGreaterThan(1);
  await expect(peers.getByText("127.0.0.1", { exact: false }).first()).toBeVisible();
  await capture(page, "live-peer-wide.png");

  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);

  await expect(torrentRow).toContainText("downloading", { timeout: 15_000 });
  const peerCells = peers.getByRole("row").nth(1).getByRole("gridcell");
  await expect
    .poll(async () => {
      const down = (await peerCells.nth(5).textContent())?.trim();
      const requests = (await peerCells.nth(7).textContent())?.trim();
      return down !== "—" || requests !== "—";
    }, { timeout: 12_000 })
    .toBe(true);
  await page.setViewportSize({ width: 920, height: 720 });
  await expect(peers).toBeVisible();
  await capture(page, "live-peer-compact.png");

  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByRole("button", { name: "Torrents", exact: true })).toBeVisible();
  await expect(peers).toBeVisible();
  await page.waitForTimeout(250);
  await capture(page, "live-peer-phone.png");

  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(torrentRow).toContainText("complete", { timeout: 30_000 });
  await expect
    .poll(async () => Number(await peers.getAttribute("aria-rowcount")))
    .toBe(1);
  await page.getByRole("tab", { name: "Files" }).click();
  await expect(files).toHaveAttribute("aria-rowcount", String(expectedFileCount + 1), {
    timeout: 10_000,
  });
  await scrollToEnd(files);
  await expect(prefixFile).toContainText("6.8 KiB");
  await expect(payloadFile).toContainText("39.0 KiB");
  expect(firstVerifiedMs).toBeGreaterThanOrEqual(firstDoneMs);
  console.log(
    `file_live_milestones ${JSON.stringify({ firstDoneMs, firstVerifiedMs, files: expectedFileCount })}`,
  );
});

async function scrollToEnd(grid: Locator) {
  await grid.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
    element.dispatchEvent(new Event("scroll"));
  });
}

async function capture(page: Page, filename: string) {
  if (screenshotDirectory === undefined) return;
  await fs.mkdir(screenshotDirectory, { recursive: true });
  await page.screenshot({
    path: path.join(screenshotDirectory, filename),
    fullPage: false,
  });
}
