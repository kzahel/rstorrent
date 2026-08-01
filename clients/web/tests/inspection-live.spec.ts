import fs from "node:fs/promises";
import path from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

const gateway = process.env.RSTORRENT_LIVE_GATEWAY_URL;
const magnet = process.env.RSTORRENT_LIVE_MAGNET;
const torrentId = process.env.RSTORRENT_LIVE_TORRENT_ID;
const screenshotDirectory = process.env.RSTORRENT_SCREENSHOT_DIR;

test("live peer inspection follows a controlled verified transfer", async ({
  page,
}) => {
  test.skip(
    gateway === undefined || magnet === undefined || torrentId === undefined,
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
  await expect(page.getByText("Torrent added", { exact: true })).toBeVisible();
  await expect(torrentInput).toHaveValue("");

  const library = page.getByRole("grid", { name: "Torrent library" });
  const torrentRow = library
    .getByRole("row")
    .filter({ hasText: torrentId!.slice(0, 12) });
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

  await page.setViewportSize({ width: 1440, height: 900 });
  await torrentRow.click();

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
  updatesReleased = new Promise<void>((resolve) => {
    releaseUpdates = resolve;
  });
  suspendUpdates = true;
  await new Promise((resolve) => setTimeout(resolve, 1_000));
  suspendUpdates = false;
  releaseUpdates();
  await expect(page.getByText("reconnecting", { exact: true })).toBeVisible();
  await expect
    .poll(async () => Number(await peers.getAttribute("aria-rowcount")))
    .toBeGreaterThan(1);
  await capture(page, "live-peer-reconnecting.png");
  await expect(page.getByText("connected", { exact: true })).toBeVisible();
  await expect.poll(() => new Set(viewSetIds).size).toBeGreaterThan(1);
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
});

async function capture(page: Page, filename: string) {
  if (screenshotDirectory === undefined) return;
  await fs.mkdir(screenshotDirectory, { recursive: true });
  await page.screenshot({
    path: path.join(screenshotDirectory, filename),
    fullPage: false,
  });
}
