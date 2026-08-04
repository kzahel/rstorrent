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
const gatewayToken = process.env.RSTORRENT_LIVE_GATEWAY_TOKEN;
const storagePath = process.env.RSTORRENT_LIVE_STORAGE_PATH;
const screenshotDirectory = process.env.RSTORRENT_SCREENSHOT_DIR;
const expectDiskPressure = process.env.RSTORRENT_LIVE_EXPECT_DISK_PRESSURE === "1";
const expectPieces = process.env.RSTORRENT_LIVE_EXPECT_PIECES === "1";
const transportBenchmark =
  process.env.RSTORRENT_LIVE_TRANSPORT_BENCHMARK === "1";
const benchmarkTransport = process.env.RSTORRENT_LIVE_TRANSPORT;
const expectFileSelection = process.env.RSTORRENT_LIVE_FILE_SELECTION === "1";

test("paired application transport throughput", async ({ page }) => {
  test.setTimeout(240_000);
  test.skip(
    !transportBenchmark ||
      gateway === undefined ||
      magnet === undefined ||
      torrentId === undefined ||
      (benchmarkTransport !== "http" && benchmarkTransport !== "websocket"),
    "paired transport benchmark is opt-in",
  );
  let applicationUpgrades = 0;
  let semanticHttpRequests = 0;
  const expectedSocket = `${gateway!.replace(/^http/, "ws")}/api/v1/connect`;
  page.on("websocket", (socket) => {
    if (socket.url() === expectedSocket) applicationUpgrades += 1;
  });
  page.on("request", (request) => {
    if (!request.url().startsWith(gateway!)) return;
    const pathname = new URL(request.url()).pathname;
    if (
      pathname === "/api/v1/hello" ||
      pathname === "/api/v1/commands" ||
      pathname.startsWith("/api/v1/view-sets")
    ) {
      semanticHttpRequests += 1;
    }
  });
  const query =
    benchmarkTransport === "http"
      ? `/?live=${encodeURIComponent(gateway!)}&transport=http&poll_ms=100`
      : `/?live=${encodeURIComponent(gateway!)}`;
  await page.goto(withGatewayToken(query));
  const transfers = page.getByRole("grid", { name: "Transfer queue" });
  await expect(transfers).toBeVisible();
  const input = page
    .getByRole("form", { name: "Add torrent" })
    .getByRole("textbox", { name: "Magnet link or torrent URL" });
  const started = performance.now();
  await input.fill(magnet!);
  await input.press("Enter");
  await confirmDefaultAddOptions(page);
  await expect(page.getByText("Torrent added", { exact: true })).toBeVisible();
  const row = transfers.locator(`[data-row-id="${torrentId!}"]`);
  await expect(row).toContainText(/complete/i, { timeout: 180_000 });
  const transferSeconds = (performance.now() - started) / 1_000;
  if (benchmarkTransport === "websocket") {
    expect(applicationUpgrades).toBe(1);
    expect(semanticHttpRequests).toBe(0);
  } else {
    expect(applicationUpgrades).toBe(0);
    expect(semanticHttpRequests).toBeGreaterThan(0);
  }
  console.log(
    `transport_benchmark_result ${JSON.stringify({ transport: benchmarkTransport, transferSeconds, applicationUpgrades, semanticHttpRequests })}`,
  );
});

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
  await page.goto(liveUrl());
  const torrentRow = await addAndOpenInWorkbench(page, magnet!, torrentId!);
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
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto(liveUrl());
  const startedAt = performance.now();
  const torrentRow = await addAndOpenInWorkbench(page, magnet!, torrentId!);
  await page.getByRole("tab", { name: "Pieces" }).click();
  const pieceMap = page.getByRole("img", { name: /pieces:/ });
  await expect(pieceMap).toBeVisible({ timeout: 20_000 });
  await expect
    .poll(async () => pieceMap.getAttribute("aria-label"), { timeout: 20_000 })
    .toMatch(/pieces: [\d,]+ verified, [1-9][\d,]* active/);
  const firstActiveMs = Math.round(performance.now() - startedAt);
  await capture(page, "live-pieces-active-wide.png");

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
  await page.goto(liveUrl());
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
  await confirmDefaultAddOptions(page);
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

  await page.getByRole("tab", { name: "Swarm" }).click();
  const swarm = page.getByRole("grid", { name: "Known swarm peers" });
  await expect(swarm).toHaveAttribute("aria-rowcount", "2", { timeout: 20_000 });
  await expect(swarm.getByText("127.0.0.1", { exact: false }).first()).toBeVisible();
  await expect(swarm.getByText(/TRACKER · Magnet/).first()).toBeVisible({
    timeout: 20_000,
  });
  await capture(page, "live-swarm-wide.png");

  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);

  await expect(torrentRow).toContainText("complete", { timeout: 30_000 });
  await expect(swarm).toHaveAttribute("aria-rowcount", "1", { timeout: 10_000 });
  await expect(page.getByText("The peer registry is inactive.")).toBeVisible();
  await page.getByRole("tab", { name: "Peers" }).click();
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
    `file_live_milestones ${JSON.stringify({ firstDoneMs, firstVerifiedMs, files: expectedFileCount, swarmSourceMerge: true, swarmTerminalCleanup: true, applicationUpgrades, semanticHttpRequests: semanticHttpRequests.length })}`,
  );
});

test("live metadata-only add and file selection", async ({ page }) => {
  test.setTimeout(90_000);
  test.skip(
    !expectFileSelection ||
      gateway === undefined ||
      gatewayToken === undefined ||
      magnet === undefined ||
      torrentId === undefined ||
      torrentName === undefined ||
      fileCount === undefined ||
      storagePath === undefined,
    "controlled live file-selection gateway is opt-in",
  );

  const storage = path.resolve(storagePath!);
  const output = path.join(storage, torrentName!);
  const staging = path.join(storage, `.${torrentId!}.rstorrent-staging`);
  const part = path.join(storage, `.${torrentId!}.rstorrent-parts`);
  const prefix = path.join(output, "nested", "prefix.bin");
  const payload = path.join(output, "payload.bin");

  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto(liveUrl());
  const addForm = page.getByRole("form", { name: "Add torrent" });
  await addForm
    .getByRole("textbox", { name: "Magnet link or torrent URL" })
    .fill(magnet!);
  await addForm.getByRole("button", { name: "Add" }).click();

  const addDialog = page.getByRole("dialog", {
    name: "Choose download options",
  });
  await expect(addDialog).toBeVisible();
  const startContent = addDialog.getByRole("checkbox", {
    name: /Start downloading files when metadata is available/,
  });
  await expect(startContent).toBeChecked();
  await startContent.uncheck();
  await addDialog.getByRole("button", { name: "Add torrent" }).click();
  await expect(page.getByText("Torrent added", { exact: true })).toBeVisible();

  const transfers = page.getByRole("grid", { name: "Transfer queue" });
  const transferRow = transfers.locator(`[data-row-id="${torrentId!}"]`);
  await expect(transferRow).toBeVisible({ timeout: 10_000 });
  await transferRow.click();
  await page
    .getByRole("navigation", { name: "Primary" })
    .getByRole("button", { name: "Workbench" })
    .click();
  const torrentRow = page
    .getByRole("grid", { name: "Torrent library" })
    .locator(`[data-row-id="${torrentId!}"]`);
  await expect(torrentRow).toBeVisible();
  await torrentRow.click();
  await page.getByRole("tab", { name: "Files" }).click();
  const files = page.getByRole("grid", { name: "Torrent files" });
  await expect(files).toHaveAttribute(
    "aria-rowcount",
    String(Number(fileCount!) + 1),
    { timeout: 20_000 },
  );
  expect(await fs.readdir(storage)).toEqual([]);

  await scrollToEnd(files);
  const prefixRow = files.getByRole("row").filter({ hasText: "prefix.bin" });
  await expect(prefixRow).toBeVisible();
  await prefixRow.click();
  await page.getByRole("button", { name: "More file actions" }).click();
  const fileActions = page.getByRole("menu", { name: "More file actions" });
  await expect(fileActions.getByRole("menuitem")).toHaveCount(2);
  await fileActions
    .getByRole("menuitem", { name: "Skip", exact: true })
    .click();
  await expect(prefixRow.getByText("Skip", { exact: true })).toBeVisible();
  expect(await fs.readdir(storage)).toEqual([]);

  await page.getByRole("button", { name: "Start", exact: true }).click();
  await expect(torrentRow).toContainText("complete", { timeout: 60_000 });
  await expect.poll(() => pathExists(payload)).toBe(true);
  await expect.poll(() => pathExists(part)).toBe(true);
  expect(await pathExists(prefix)).toBe(false);
  expect(await pathExists(staging)).toBe(false);

  await prefixRow.click();
  await page.getByRole("button", { name: "More file actions" }).click();
  await page
    .getByRole("menu", { name: "More file actions" })
    .getByRole("menuitem", { name: "Normal", exact: true })
    .click();
  await expect(prefixRow.getByText("Normal", { exact: true })).toBeVisible();
  await expect.poll(() => pathExists(prefix), { timeout: 20_000 }).toBe(true);
  await expect.poll(() => pathExists(part), { timeout: 20_000 }).toBe(false);
  await expect(torrentRow).toContainText("complete", { timeout: 20_000 });
  console.log(
    "file_selection_live_milestones metadata_only=no_artifacts skip=published_part normal=materialized_part_removed",
  );
});

async function addAndOpenInWorkbench(
  page: Page,
  liveMagnet: string,
  liveTorrentId: string,
): Promise<Locator> {
  const primary = page.getByRole("navigation", { name: "Primary" });
  const transfers = page.getByRole("grid", { name: "Transfer queue" });
  await expect(primary).toBeVisible();
  await expect(transfers).toBeVisible();
  const input = page
    .getByRole("form", { name: "Add torrent" })
    .getByRole("textbox", { name: "Magnet link or torrent URL" });
  await input.fill(liveMagnet);
  await input.press("Enter");
  await confirmDefaultAddOptions(page);
  await expect(page.getByText("Torrent added", { exact: true })).toBeVisible();
  const transferRow = transfers.locator(`[data-row-id="${liveTorrentId}"]`);
  await expect(transferRow).toBeVisible({ timeout: 10_000 });
  await transferRow.click();
  await primary.getByRole("button", { name: "Workbench" }).click();
  const torrentRow = page
    .getByRole("grid", { name: "Torrent library" })
    .locator(`[data-row-id="${liveTorrentId}"]`);
  await expect(torrentRow).toBeVisible({ timeout: 10_000 });
  await torrentRow.click();
  return torrentRow;
}

async function scrollToEnd(grid: Locator) {
  await grid.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
    element.dispatchEvent(new Event("scroll"));
  });
}

async function confirmDefaultAddOptions(page: Page) {
  const dialog = page.getByRole("dialog", { name: "Choose download options" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Add torrent" }).click();
}

async function capture(page: Page, filename: string) {
  if (screenshotDirectory === undefined) return;
  await fs.mkdir(screenshotDirectory, { recursive: true });
  await page.screenshot({
    path: path.join(screenshotDirectory, filename),
    fullPage: false,
  });
}

function liveUrl(): string {
  return withGatewayToken(`/?live=${encodeURIComponent(gateway!)}`);
}

function withGatewayToken(url: string): string {
  if (gatewayToken === undefined) return url;
  const separator = url.includes("?") ? "&" : "?";
  return `${url}${separator}token=${encodeURIComponent(gatewayToken)}`;
}

async function pathExists(candidate: string): Promise<boolean> {
  try {
    await fs.stat(candidate);
    return true;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return false;
    throw error;
  }
}
