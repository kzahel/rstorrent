import fs from "node:fs/promises";
import path from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";

const gateway = process.env.RSTORRENT_LIVE_GATEWAY_URL;
const magnet = process.env.RSTORRENT_LIVE_MAGNET;
const torrentId = process.env.RSTORRENT_LIVE_TORRENT_ID;
const torrentName = process.env.RSTORRENT_LIVE_TORRENT_NAME;
const fileCount = process.env.RSTORRENT_LIVE_FILE_COUNT;
const trackerUrl = process.env.RSTORRENT_LIVE_TRACKER_URL;
const screenshotDirectory = process.env.RSTORRENT_SCREENSHOT_DIR;
const expectDiskPressure = process.env.RSTORRENT_LIVE_EXPECT_DISK_PRESSURE === "1";
const expectPieces = process.env.RSTORRENT_LIVE_EXPECT_PIECES === "1";

test("live disk inspection observes pressure and exact recovery", async ({
  page,
}) => {
  test.setTimeout(90_000);
  test.skip(
    !expectDiskPressure ||
      gateway === undefined ||
      magnet === undefined ||
      torrentId === undefined ||
      torrentName === undefined,
    "controlled slow-storage gateway is opt-in",
  );
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto(
    `/?live=${encodeURIComponent(gateway!)}&transport=http&poll_ms=100`,
  );
  await expect(
    page.getByRole("navigation", { name: "Torrent library" }),
  ).toBeVisible();

  const addForm = page.getByRole("form", { name: "Add torrent" });
  const torrentInput = addForm.getByRole("textbox", {
    name: "Magnet link or torrent URL",
  });
  await torrentInput.fill(magnet!);
  await torrentInput.press("Enter");
  await expect(page.getByText("Torrent added", { exact: true })).toBeVisible();

  const torrentRow = page
    .getByRole("grid", { name: "Torrent library" })
    .locator(`[data-row-id="${torrentId!}"]`);
  await expect(torrentRow).toBeVisible({ timeout: 10_000 });
  await page.getByRole("tab", { name: "Disk" }).click();
  const pieces = page.getByRole("grid", { name: "Active storage pieces" });
  await expect(page.getByLabel("Disk pressure Backpressured")).toBeVisible({
    timeout: 20_000,
  });
  await expect(page.getByText("intake paused now", { exact: true })).toBeVisible();
  await expect
    .poll(async () => Number(await pieces.getAttribute("aria-rowcount")))
    .toBeGreaterThan(1);
  await capture(page, "live-disk-backpressured-wide.png");

  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) =>
      violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);

  await expect(torrentRow).toContainText("complete", { timeout: 60_000 });
  await expect(page.getByLabel("Disk pressure Idle")).toBeVisible({
    timeout: 10_000,
  });
  await expect(pieces).toHaveAttribute("aria-rowcount", "1");
  await expect(page.getByText("intake is open", { exact: true })).toBeVisible();
  await capture(page, "live-disk-recovered-wide.png");
  console.log("disk_live_milestones pressure=backpressured completion=verified recovery=idle");
});

test("live piece inspection follows active work through verification", async ({
  page,
}) => {
  test.setTimeout(90_000);
  test.skip(
    !expectPieces ||
      gateway === undefined ||
      magnet === undefined ||
      torrentId === undefined ||
      torrentName === undefined,
    "controlled piece-map gateway is opt-in",
  );
  let suspendUpdates = false;
  let releaseUpdates = () => {};
  let updatesReleased = Promise.resolve();
  await page.route("**/api/v1/view-sets/*/updates?**", async (route) => {
    if (suspendUpdates) await updatesReleased;
    await route.continue();
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto(
    `/?live=${encodeURIComponent(gateway!)}&transport=http&poll_ms=100`,
  );
  await expect(
    page.getByRole("navigation", { name: "Torrent library" }),
  ).toBeVisible();

  const input = page
    .getByRole("form", { name: "Add torrent" })
    .getByRole("textbox", { name: "Magnet link or torrent URL" });
  const startedAt = performance.now();
  await input.fill(magnet!);
  await input.press("Enter");
  await expect(page.getByText("Torrent added", { exact: true })).toBeVisible();

  const torrentRow = page
    .getByRole("grid", { name: "Torrent library" })
    .locator(`[data-row-id="${torrentId!}"]`);
  await expect(torrentRow).toBeVisible({ timeout: 10_000 });
  await torrentRow.click();
  await page.getByRole("tab", { name: "Pieces" }).click();
  const pieceMap = page.getByRole("img", { name: /pieces:/ });
  await expect(pieceMap).toBeVisible({ timeout: 20_000 });
  await expect
    .poll(async () => pieceMap.getAttribute("aria-label"), { timeout: 20_000 })
    .toMatch(/pieces: [\d,]+ verified, [1-9][\d,]* active/);
  const firstActiveMs = Math.round(performance.now() - startedAt);
  await capture(page, "live-pieces-active-wide.png");

  updatesReleased = new Promise<void>((resolve) => {
    releaseUpdates = resolve;
  });
  suspendUpdates = true;
  await new Promise((resolve) => setTimeout(resolve, 1_000));
  suspendUpdates = false;
  releaseUpdates();
  await expect(page.getByText("reconnecting", { exact: true })).toBeVisible();
  await expect(page.getByText("connected", { exact: true })).toBeVisible();
  await expect(pieceMap).toBeVisible();

  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) =>
      violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);

  await expect(torrentRow).toContainText("complete", { timeout: 60_000 });
  await expect
    .poll(async () => pieceMap.getAttribute("aria-label"), { timeout: 10_000 })
    .toMatch(/^17 pieces: 17 verified, 0 active$/);
  const completeMs = Math.round(performance.now() - startedAt);
  await capture(page, "live-pieces-complete-wide.png");
  console.log(
    `piece_live_milestones ${JSON.stringify({ firstActiveMs, completeMs, pieces: 17 })}`,
  );
});

test("live peer inspection follows a controlled verified transfer", async ({
  page,
}) => {
  test.setTimeout(60_000);
  test.skip(
    gateway === undefined ||
      magnet === undefined ||
      torrentId === undefined ||
      torrentName === undefined ||
      fileCount === undefined ||
      trackerUrl === undefined,
    "controlled live gateway is opt-in",
  );
  let applicationUpgrades = 0;
  const semanticHttpRequests: string[] = [];
  const semanticPaths = [
    "/api/v1/hello",
    "/api/v1/commands",
    "/api/v1/view-sets",
  ];
  page.on("websocket", (socket) => {
    if (socket.url() === `${gateway!.replace(/^http/, "ws")}/api/v1/connect`) {
      applicationUpgrades += 1;
    }
  });
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (
      request.url().startsWith(gateway!) &&
      semanticPaths.some(
        (path) => url.pathname === path || url.pathname.startsWith(`${path}/`),
      )
    ) {
      semanticHttpRequests.push(`${request.method()} ${url.pathname}`);
    }
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto(`/?live=${encodeURIComponent(gateway!)}`);
  const primary = page.getByRole("navigation", { name: "Primary" });
  const transferGrid = page.getByRole("grid", { name: "Transfer queue" });
  await expect(primary).toBeVisible();
  await expect(transferGrid).toBeVisible();
  await expect.poll(() => applicationUpgrades).toBe(1);

  const addForm = page.getByRole("form", { name: "Add torrent" });
  const torrentInput = addForm.getByRole("textbox", {
    name: "Magnet link or torrent URL",
  });
  const transferStartedAt = performance.now();
  await torrentInput.fill(magnet!);
  await torrentInput.press("Enter");
  await expect(page.getByText("Torrent added", { exact: true })).toBeVisible();
  await expect(torrentInput).toHaveValue("");

  const transferRow = transferGrid.locator(`[data-row-id="${torrentId!}"]`);
  await expect(transferRow).toContainText(torrentName!, { timeout: 20_000 });
  await transferRow.click();
  await primary.getByRole("button", { name: "Workbench" }).click();
  const library = page.getByRole("grid", { name: "Torrent library" });
  await expect(library).toBeVisible();
  const torrentRow = library.locator(`[data-row-id="${torrentId!}"]`);
  await expect(torrentRow).toBeVisible();
  await torrentRow.click();
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
  expect(applicationUpgrades).toBe(1);
  expect(semanticHttpRequests).toEqual([]);
  console.log(
    `file_live_milestones ${JSON.stringify({ firstDoneMs, firstVerifiedMs, files: expectedFileCount, applicationUpgrades, semanticHttpRequests: semanticHttpRequests.length })}`,
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
