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
  await expect(page.getByRole("grid", { name: "Connected and candidate peers" })).toBeVisible();
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
  await expect(page.getByRole("grid", { name: "Connected and candidate peers" })).toHaveAttribute(
    "aria-rowcount",
    "15",
  );
  await capture(page, "rstorrent-demo-compact.png");
});

test("phone navigation opens a full detail surface", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openScenario(page, "healthy-download", 42_000);
  const library = page.getByRole("grid", { name: "Torrent library" });
  await expect(library).toBeVisible();
  await library.getByRole("row").filter({ hasText: "Big Buck Bunny" }).click();
  const backButton = page.getByRole("button", { name: "Torrents", exact: true });
  await expect(backButton).toBeVisible();
  await expect(page.getByRole("grid", { name: "Connected and candidate peers" })).toBeVisible();
  await capture(page, "rstorrent-demo-phone.png");
  await backButton.click();
  await expect(library).toBeVisible();
});

test("large collections retain a bounded virtual DOM", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openScenario(page, "large-swarm", 0);
  const torrents = page.getByRole("grid", { name: "Torrent library" });
  const peers = page.getByRole("grid", { name: "Connected and candidate peers" });
  await expect(torrents).toHaveAttribute("aria-rowcount", "2001");
  await expect(peers).toHaveAttribute("aria-rowcount", "10001");
  expect(await torrents.getByRole("row").count()).toBeLessThanOrEqual(100);
  expect(await peers.getByRole("row").count()).toBeLessThanOrEqual(100);
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
