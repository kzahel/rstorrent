import fs from "node:fs/promises";
import path from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

const screenshotDirectory = process.env.RSTORRENT_SCREENSHOT_DIR;

test("primary destinations preserve shared source state", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/?demo=healthy-download&at=42000&autoplay=0");
  await expect(page.getByText("RSTorrent", { exact: true })).toBeVisible();

  const primary = page.getByRole("navigation", { name: "Primary" });
  const transfers = primary.getByRole("button", { name: "Transfers" });
  await expect(transfers).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("grid", { name: "Transfer queue" })).toHaveAttribute(
    "aria-rowcount",
    "4",
  );
  await capture(page, "rstorrent-transfers-wide.png");
  const sintelCheck = page.getByRole("checkbox", {
    name: "Select Sintel 4K open movie",
  });
  await expect(sintelCheck).not.toBeChecked();
  const bunnyRow = page.getByRole("row").filter({
    hasText: "Big Buck Bunny 1080p surround",
  });
  const transferRow = page.getByRole("row").filter({ hasText: "Sintel" });
  await bunnyRow.click();
  const normalColumns = await transferRow.getAttribute("style");
  await transferRow.click({ modifiers: ["Shift"] });
  await expect(page.getByText("2 selected for actions")).toBeVisible();
  await expect(transferRow).toHaveAttribute("style", normalColumns ?? "");
  await page
    .getByRole("navigation", { name: "Transfer filters" })
    .getByRole("button", { name: /Paused/ })
    .click();
  await expect(
    page.getByText("2 selected for actions (1 outside this view)"),
  ).toBeVisible();
  await primary.getByRole("button", { name: "Workbench" }).click();
  await expect(
    page.getByRole("checkbox", {
      name: "Deselect Big Buck Bunny 1080p surround",
    }),
  ).toBeChecked();
  await expect(
    page.getByRole("checkbox", { name: "Deselect Sintel 4K open movie" }),
  ).toBeChecked();

  await primary.getByRole("button", { name: "Library" }).click();
  await expect(page.getByRole("list", { name: "Torrent-backed content" })).toBeVisible();
  await expect(page.getByText(/media details are not connected yet/i)).toBeVisible();
  await expect(page.getByRole("button", { name: /^Play / })).toHaveCount(0);
  await capture(page, "rstorrent-library-wide.png");
  await page
    .getByRole("button", { name: "Activate Sintel 4K open movie in Library" })
    .click();
  await page.getByRole("button", { name: "Open in Workbench" }).click();
  await expect(primary.getByRole("button", { name: "Workbench" })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await page.getByRole("tab", { name: "General" }).click();
  await expect(page.getByText("Current transfer")).toBeVisible();
  await expect(page.getByText("Sintel 4K open movie").first()).toBeVisible();

  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  await capture(page, "rstorrent-destinations-wide.png");
});

test("More copies selected torrents' canonical magnets", async ({
  context,
  page,
}) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.setViewportSize({ width: 1024, height: 720 });
  await page.goto("/?demo=healthy-download&at=42000&autoplay=0");

  await page
    .getByRole("checkbox", { name: "Select Sintel 4K open movie" })
    .click();

  const more = page.getByRole("button", { name: "More" });
  await more.click();
  const menu = page.getByRole("menu", { name: "More" });
  const copy = menu.getByRole("menuitem", { name: "Copy magnet links" });
  await expect(copy).toBeEnabled();

  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  await copy.click();

  await expect(
    page.getByText("2 magnet links copied", { exact: true }),
  ).toBeVisible();
  await expect(menu).toHaveCount(0);
  await expect(more).toBeFocused();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
    [
      "magnet:?xt=urn:btih:a962f460b83861cfb5faa1d7ad7da9c3f3cc2fc4",
      "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10",
    ].join("\n"),
  );
});

test("torrent and file rows expose exact accessible context actions", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1024, height: 720 });
  await page.goto("/?demo=healthy-download&at=42000&autoplay=0");
  const transferGrid = page.getByRole("grid", { name: "Transfer queue" });
  const sintelRow = transferGrid.getByRole("row").filter({
    hasText: "Sintel 4K open movie",
  });

  await sintelRow.click({ button: "right", position: { x: 300, y: 18 } });
  let menu = page.getByRole("menu", { name: "Torrent actions" });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("menuitem")).toHaveCount(7);
  await expect(
    menu.getByRole("menuitem", { name: "Copy magnet link" }),
  ).toBeVisible();
  await expect(menu.getByRole("group", { name: "Transfer" })).toBeVisible();
  await expect(menu.getByRole("group", { name: "Sharing" })).toBeVisible();
  await expect(
    menu.getByRole("group", { name: "Organization" }),
  ).toBeVisible();
  await expect(
    menu.getByRole("group", { name: "Destructive" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(
    transferGrid.getByRole("checkbox", {
      name: "Deselect Sintel 4K open movie",
    }),
  ).toBeChecked();
  await expect(sintelRow).toBeFocused();

  await transferGrid
    .getByRole("checkbox", { name: "Select Big Buck Bunny 1080p surround" })
    .click();
  await sintelRow.click({ button: "right", position: { x: 1, y: 18 } });
  menu = page.getByRole("menu", { name: "Torrent actions" });
  await expect(
    menu.getByRole("menuitem", { name: "Copy magnet links" }),
  ).toBeVisible();
  const menuBounds = await menu.boundingBox();
  expect(menuBounds).not.toBeNull();
  expect(menuBounds!.x).toBeGreaterThanOrEqual(0);
  expect(menuBounds!.x + menuBounds!.width).toBeLessThanOrEqual(1024);
  const menuViolations = (
    await new AxeBuilder({ page }).include('[role="menu"]').analyze()
  ).violations.filter(
    (violation) =>
      violation.impact === "serious" || violation.impact === "critical",
  );
  expect(menuViolations).toEqual([]);
  await menu.getByRole("menuitem", { name: "Remove" }).click();
  const removal = page.getByRole("dialog", { name: "Remove 2 torrents?" });
  await expect(
    removal.getByRole("checkbox", { name: "Also delete downloaded data" }),
  ).not.toBeChecked();
  await expect(
    removal.getByRole("list", { name: "Torrents to remove" }),
  ).toBeVisible();
  await removal.getByRole("button", { name: "Cancel" }).click();
  await expect(sintelRow).toBeFocused();

  await sintelRow.focus();
  await page.keyboard.press("Shift+F10");
  await expect(
    page
      .getByRole("menu", { name: "Torrent actions" })
      .getByRole("menuitem", { name: "Copy magnet links" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");

  await page
    .getByRole("navigation", { name: "Primary" })
    .getByRole("button", { name: "Workbench" })
    .click();
  const workbenchGrid = page.getByRole("grid", { name: "Torrent library" });
  const workbenchSintel = workbenchGrid.getByRole("row").filter({
    hasText: "Sintel 4K open movie",
  });
  await workbenchSintel.click({ button: "right" });
  await expect(
    page.getByRole("menu", { name: "Torrent actions" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");

  await page.goto("/?demo=file-progress&at=24000&autoplay=0");
  await page
    .getByRole("navigation", { name: "Primary" })
    .getByRole("button", { name: "Workbench" })
    .click();
  await page.getByRole("tab", { name: "Files" }).click();
  const files = page.getByRole("grid", { name: "Torrent files" });
  await expect(files).toHaveAttribute("aria-rowcount", "4096");
  const fileRow = files.getByRole("row").filter({ hasText: "asset-001.mkv" });
  await fileRow.click({ button: "right" });
  const fileMenu = page.getByRole("menu", { name: "File actions" });
  await expect(fileMenu.getByRole("menuitem", { name: "Normal" })).toBeDisabled();
  await expect(fileMenu.getByRole("menuitem", { name: "Skip" })).toBeDisabled();
  expect(await files.getByRole("row").count()).toBeLessThanOrEqual(100);
  const fileViolations = (
    await new AxeBuilder({ page }).include('[role="menu"]').analyze()
  ).violations.filter(
    (violation) =>
      violation.impact === "serious" || violation.impact === "critical",
  );
  expect(fileViolations).toEqual([]);
});

test("phone destinations and contextual filters remain reachable", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/?demo=healthy-download&at=42000&autoplay=0");
  const transferGrid = page.getByRole("grid", { name: "Transfer queue" });
  await expect(transferGrid).toBeVisible();
  const heldRow = transferGrid.getByRole("row").filter({ hasText: "Sintel" });
  await heldRow.dispatchEvent("pointerdown", {
    button: 0,
    clientX: 20,
    clientY: 20,
    pointerId: 41,
    pointerType: "touch",
  });
  await page.waitForTimeout(550);
  await expect(
    page.getByRole("checkbox", { name: "Deselect Sintel 4K open movie" }),
  ).toBeChecked();
  await page
    .getByRole("button", {
      name: "Done selecting rows in Transfer queue",
    })
    .click();
  await expect(
    page.getByRole("checkbox", { name: "Select Sintel 4K open movie" }),
  ).not.toBeChecked();
  const primary = page.getByRole("navigation", { name: "Primary" });
  await primary.getByRole("button", { name: "Library" }).click();
  await expect(page.getByRole("list", { name: "Torrent-backed content" })).toBeVisible();
  await page.getByRole("button", { name: "Toggle Library filters" }).click();
  await expect(page.getByRole("navigation", { name: "Library filters" })).toBeVisible();
  await page.getByRole("button", { name: "Toggle Library filters" }).click();
  await capture(page, "rstorrent-library-phone.png");

  await primary.getByRole("button", { name: "Workbench" }).click();
  await expect(page.getByRole("grid", { name: "Torrent library" })).toBeVisible();
  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
});

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
  const detail = page.getByRole("region", { name: "Torrent details" });
  await page.getByRole("tab", { name: "General" }).click();
  await torrentRows.nth(2).focus();
  await page.keyboard.press("ArrowDown");
  await expect(torrentRows.nth(3)).toHaveAttribute("aria-current", "true");
  await expect(
    detail.getByRole("heading", { name: "Sintel 4K open movie" }),
  ).toBeVisible();

  await page.keyboard.press("Shift+ArrowUp");
  await expect(torrentRows.nth(2)).toHaveAttribute("aria-current", "true");
  await expect(page.getByText("2 selected for actions")).toBeVisible();
  await expect(
    detail.getByRole("heading", {
      name: "Big Buck Bunny 1080p surround",
    }),
  ).toBeVisible();

  await page.keyboard.press("Control+a");
  await expect(page.getByText("3 selected for actions")).toBeVisible();
  await page.keyboard.press("ArrowUp");
  await expect(torrentRows.nth(1)).toHaveAttribute("aria-current", "true");
  await expect(page.getByText("3 selected for actions")).toHaveCount(0);
  await expect(
    page.getByRole("checkbox", {
      name: "Deselect Arch Linux 2026.08.01 x86_64",
    }),
  ).toBeChecked();
  await expect(
    page.getByRole("checkbox", {
      name: "Select Big Buck Bunny 1080p surround",
    }),
  ).not.toBeChecked();
  await expect(
    detail.getByRole("heading", { name: "Arch Linux 2026.08.01 x86_64" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByText("3 selected for actions")).toHaveCount(0);
  await expect(torrentRows.nth(1)).toHaveAttribute("aria-current", "true");
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

test("peer flags expose a complete accessible legend without sorting", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1024, height: 720 });
  await openScenario(page, "healthy-download", 42_000);

  const peers = page.getByRole("grid", { name: "Active peer connections" });
  await expect(peers.getByText("libtorrent 2.0.13").first()).toBeVisible();
  const header = page.getByRole("columnheader", { name: "Flags" });
  const help = page.getByRole("button", { name: "Explain Flags" });
  await expect(page.getByLabel(/^Peer flags:/).first()).toBeVisible();
  await expect(header).not.toHaveAttribute("aria-sort");
  await help.click();
  const legend = page.getByRole("dialog", { name: "Flags column help" });
  await expect(legend).toBeVisible();
  await expect(legend).toBeFocused();
  await expect(legend.locator("dt code")).toHaveCount(16);
  await expect(legend.getByText("Incoming", { exact: true })).toBeVisible();
  await expect(legend.getByText("Encrypted", { exact: true })).toBeVisible();
  await expect(
    legend.getByText("Peer flag legend", { exact: true }),
  ).toHaveCount(0);
  await expect(legend.getByText(/case-sensitive/)).toHaveCount(0);
  await expect(legend.getByText(/remote peer initiated/)).toHaveCount(0);
  const legendType = await legend.evaluate((element) => {
    const content = element.firstElementChild;
    const nodes = [
      content?.querySelector("h3"),
      content?.querySelector("dt code"),
      content?.querySelector("dd"),
    ].filter((node): node is Element => node !== null && node !== undefined);
    return {
      fontSizes: nodes.map((node) =>
        Number.parseFloat(globalThis.getComputedStyle(node).fontSize),
      ),
      fontWeights: nodes.map(
        (node) => globalThis.getComputedStyle(node).fontWeight,
      ),
    };
  });
  expect(Math.max(...legendType.fontSizes)).toBeLessThanOrEqual(11);
  expect(legendType.fontWeights).toEqual(["400", "400", "400"]);
  const compactLegendBounds = await legend.boundingBox();
  expect(compactLegendBounds).not.toBeNull();
  expect(compactLegendBounds!.width).toBeLessThanOrEqual(260);
  expect(compactLegendBounds!.height).toBeLessThanOrEqual(360);
  await expect(header).not.toHaveAttribute("aria-sort");
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (violation) =>
        violation.impact === "serious" || violation.impact === "critical",
    ),
  ).toEqual([]);
  await capture(page, "rstorrent-peer-flags-light.png");

  await page.keyboard.press("Escape");
  await expect(legend).not.toBeVisible();
  await expect(help).toBeFocused();

  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("radio", { name: /Dark/ }).check();
  await settings.getByRole("radio", { name: /Compact/ }).check();
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 920, height: 720 });
  await expect(peers.getByText("libtorrent 2.0.13").first()).toBeVisible();
  await help.scrollIntoViewIfNeeded();
  await expect
    .poll(async () => Math.round((await help.boundingBox())?.width ?? 0))
    .toBeGreaterThanOrEqual(24);
  await help.click();
  await expect(legend).toBeVisible();
  const legendBounds = await legend.boundingBox();
  expect(legendBounds).not.toBeNull();
  expect(legendBounds!.x).toBeGreaterThanOrEqual(0);
  expect(legendBounds!.x + legendBounds!.width).toBeLessThanOrEqual(920);
  expect(legendBounds!.y).toBeGreaterThanOrEqual(0);
  expect(legendBounds!.y + legendBounds!.height).toBeLessThanOrEqual(720);
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (violation) =>
        violation.impact === "serious" || violation.impact === "critical",
    ),
  ).toEqual([]);
  await capture(page, "rstorrent-peer-flags-dark.png");
  await page.mouse.click(4, 4);
  await expect(legend).not.toBeVisible();
});

test("interface size settings persist and keep geometry coherent", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1024, height: 720 });
  await openScenario(page, "healthy-download", 42_000);

  const app = page.locator("#app > [data-interface-size]");
  const library = page.getByRole("grid", { name: "Torrent library" });
  const firstRow = library.locator("[data-row-id]").first();
  const more = page.getByRole("button", { name: "More", exact: true });
  await expect(app).toHaveAttribute("data-interface-size", "standard");
  await expect.poll(() => elementHeight(firstRow)).toBe(36);
  await expect.poll(() => elementHeight(more)).toBe(36);
  await expect
    .poll(async () =>
      Math.round((await more.locator("svg").boundingBox())?.width ?? 0),
    )
    .toBeGreaterThanOrEqual(17);

  const settings = page.getByRole("button", { name: "Settings", exact: true });
  await settings.click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  const close = dialog.getByRole("button", { name: "Close settings" });
  await expect(close).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(dialog.getByRole("radio", { name: /Spacious/ })).toBeFocused();
  await dialog.getByRole("radio", { name: /Standard/ }).check();
  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) =>
      violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  await capture(page, "rstorrent-settings-standard.png");

  await dialog.getByRole("radio", { name: /Compact/ }).check();
  await expect(app).toHaveAttribute("data-interface-size", "compact");
  await expect.poll(() => elementHeight(firstRow)).toBe(32);
  await expect.poll(() => elementHeight(more)).toBe(30);
  await expect
    .poll(async () =>
      Math.round((await more.locator("svg").boundingBox())?.width ?? 0),
    )
    .toBeGreaterThanOrEqual(16);
  await capture(page, "rstorrent-settings-compact.png");

  await dialog.getByRole("radio", { name: /Spacious/ }).check();
  await expect(app).toHaveAttribute("data-interface-size", "spacious");
  await expect.poll(() => elementHeight(firstRow)).toBe(42);
  await expect.poll(() => elementHeight(more)).toBe(44);
  await capture(page, "rstorrent-settings-spacious.png");
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await expect(settings).toBeFocused();

  await page.reload();
  await expect(app).toHaveAttribute(
    "data-interface-size",
    "spacious",
  );
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const phoneDialog = page.getByRole("dialog", { name: "Settings" });
  await expect(phoneDialog).toBeVisible();
  await expect
    .poll(async () =>
      Math.round((await phoneDialog.boundingBox())?.width ?? 0),
    )
    .toBe(390);
  const phoneViolations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) =>
      violation.impact === "serious" || violation.impact === "critical",
  );
  expect(phoneViolations).toEqual([]);
  await capture(page, "rstorrent-settings-phone.png");
});

test("color themes follow or override system appearance and persist", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1024, height: 720 });
  await page.emulateMedia({ colorScheme: "dark" });
  await openScenario(page, "healthy-download", 42_000);

  const root = page.locator("html");
  await expect(page.locator('meta[name="color-scheme"]')).toHaveAttribute(
    "content",
    "light dark",
  );
  await expect(root).toHaveAttribute("data-color-theme", "auto");
  await expect.poll(() => themeMetrics(page)).toEqual({
    colorScheme: "dark",
    background: "rgb(21, 27, 34)",
  });

  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(
    dialog
      .getByRole("group", { name: "Color theme" })
      .getByRole("radio", { name: /Auto/ }),
  ).toBeChecked();
  await capture(page, "rstorrent-settings-auto-dark.png");

  await dialog.getByRole("radio", { name: /Light/ }).check();
  await expect(root).toHaveAttribute("data-color-theme", "light");
  await expect.poll(() => themeMetrics(page)).toEqual({
    colorScheme: "light",
    background: "rgb(255, 255, 255)",
  });
  const lightViolations = (
    await new AxeBuilder({ page }).analyze()
  ).violations.filter(
    (violation) =>
      violation.impact === "serious" || violation.impact === "critical",
  );
  expect(lightViolations).toEqual([]);
  await capture(page, "rstorrent-settings-explicit-light.png");

  await dialog.getByRole("radio", { name: /Dark/ }).check();
  await page.emulateMedia({ colorScheme: "light" });
  await expect(root).toHaveAttribute("data-color-theme", "dark");
  await expect.poll(() => themeMetrics(page)).toEqual({
    colorScheme: "dark",
    background: "rgb(21, 27, 34)",
  });
  const darkViolations = (
    await new AxeBuilder({ page }).analyze()
  ).violations.filter(
    (violation) =>
      violation.impact === "serious" || violation.impact === "critical",
  );
  expect(darkViolations).toEqual([]);
  await capture(page, "rstorrent-settings-explicit-dark.png");

  await dialog
    .getByRole("group", { name: "Color theme" })
    .getByRole("radio", { name: /Auto/ })
    .check();
  await expect.poll(() => themeMetrics(page)).toEqual({
    colorScheme: "light",
    background: "rgb(255, 255, 255)",
  });
  await page.emulateMedia({ colorScheme: "dark" });
  await expect.poll(() => themeMetrics(page)).toEqual({
    colorScheme: "dark",
    background: "rgb(21, 27, 34)",
  });

  await dialog.getByRole("radio", { name: /Dark/ }).check();
  expect(
    await page.evaluate(() =>
      JSON.parse(
        localStorage.getItem("rstorrent.presentation.appearance") ?? "null",
      ),
    ),
  ).toEqual({
    version: 2,
    interfaceSize: "standard",
    colorTheme: "dark",
  });
  await page.addInitScript(() => {
    const observer = new MutationObserver(() => {
      if (document.querySelector("#app")?.firstElementChild) {
        sessionStorage.setItem(
          "rstorrent.theme-at-first-content",
          document.documentElement.dataset.colorTheme ?? "",
        );
        observer.disconnect();
      }
    });
    document.addEventListener(
      "readystatechange",
      () =>
        observer.observe(document.documentElement, {
          childList: true,
          subtree: true,
        }),
      { once: true },
    );
  });
  await page.emulateMedia({ colorScheme: "light" });
  await page.reload();
  await expect(page.getByText("RSTorrent", { exact: true })).toBeVisible();
  await expect(root).toHaveAttribute("data-color-theme", "dark");
  await expect(page.locator('meta[name="color-scheme"]')).toHaveAttribute(
    "content",
    "dark",
  );
  await expect.poll(() => themeMetrics(page)).toEqual({
    colorScheme: "dark",
    background: "rgb(21, 27, 34)",
  });
  expect(
    await page.evaluate(() =>
      sessionStorage.getItem("rstorrent.theme-at-first-content"),
    ),
  ).toBe("dark");
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
        .getByRole("navigation", { name: "Workbench torrent filters" })
        .evaluate((element) => Math.round(element.getBoundingClientRect().right)),
    )
    .toBeLessThanOrEqual(0);
  await capture(page, "rstorrent-trackers-phone.png");
});

test("swarm lifecycle remains readable and accessible across layouts", async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 760 });
  await openScenario(page, "swarm-lifecycle", 24_000);
  await page.getByRole("tab", { name: "Swarm" }).click();
  const swarm = page.getByRole("grid", { name: "Known swarm peers" });
  await expect(swarm).toHaveAttribute("aria-rowcount", "9");
  await expect(page.getByLabel("Swarm registry summary")).toContainText(/8.*known/);
  await expect(swarm.getByText("backed off").first()).toBeVisible();
  await expect(swarm.getByText(/TRACKER · DHT/).first()).toBeVisible();
  let violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  await capture(page, "rstorrent-swarm-wide.png");

  await page.setViewportSize({ width: 390, height: 844 });
  await page
    .getByRole("grid", { name: "Torrent library" })
    .getByRole("row")
    .filter({ hasText: "Big Buck Bunny" })
    .click();
  await expect(swarm).toBeVisible();
  await expect(page.getByRole("button", { name: "Torrents", exact: true })).toBeVisible();
  violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  await capture(page, "rstorrent-swarm-phone.png");
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
        .getByRole("navigation", { name: "Workbench torrent filters" })
        .evaluate((element) => Math.round(element.getBoundingClientRect().right)),
    )
    .toBeLessThanOrEqual(0);
  await capture(page, "rstorrent-disk-phone.png");
});

test("speed history stays exact, selectable, and accessible", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 820 });
  await openScenario(page, "speed-bursty", 42_000);
  await page.getByRole("tab", { name: "Speed" }).click();

  const panel = page.getByLabel("Session speed history");
  const chart = page.getByRole("img", { name: /Speed history chart/ });
  await expect(panel).toContainText("Session · All torrents");
  await expect(chart).toBeVisible();
  await expect(page.getByLabel("Selected speed window summaries")).toContainText(
    "Received",
  );

  await chart.focus();
  await page.keyboard.press("ArrowLeft");
  await expect(panel.getByRole("status")).toContainText(/Received.*\/s/);

  await panel.getByRole("button", { name: "DHT in" }).click();
  await expect(panel.getByText("4 of 8")).toBeVisible();
  const historyRange = panel.getByRole("combobox", { name: "History" });
  await historyRange.selectOption("hours24");
  await expect(historyRange).toHaveValue("hours24");

  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  await capture(page, "rstorrent-speed-wide.png");

  await page.setViewportSize({ width: 390, height: 844 });
  await page
    .getByRole("grid", { name: "Torrent library" })
    .getByRole("row")
    .filter({ hasText: "Big Buck Bunny" })
    .click();
  await expect(chart).toBeVisible();
  await capture(page, "rstorrent-speed-phone.png");

  await page.setViewportSize({ width: 1280, height: 820 });
  await openScenario(page, "speed-stale", 42_000);
  await page.getByRole("tab", { name: "Speed" }).click();
  await expect(page.getByText("Frozen · stale")).toBeVisible();
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
        .getByRole("navigation", { name: "Workbench torrent filters" })
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
  await expect(
    page.getByText("Removed Big Buck Bunny 1080p surround", { exact: true }),
  ).toBeVisible();
});

test("diagnostic console stays ordered, filtered, and virtualized", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openScenario(page, "diagnostic-console", 45_000);
  await page.getByRole("tab", { name: "Logs" }).click();
  const feed = page.getByRole("log", {
    name: "Chronological diagnostic events",
  });
  await expect(feed).toBeVisible();
  await expect(page.getByText("2,048 retained", { exact: true })).toBeVisible();
  expect(await feed.locator("article").count()).toBeLessThan(60);

  await page
    .getByLabel("Diagnostic capture profile")
    .selectOption("trace");
  await expect(page.getByText("High-volume producer capture")).toBeVisible();
  await page.getByLabel("Minimum displayed severity").selectOption("warning");
  await page.getByLabel("Displayed torrent scope").selectOption("all");
  await expect(page.getByText("410 shown", { exact: true })).toBeVisible();
  await page.getByRole("searchbox", { name: "Search diagnostics" }).fill("watermark");
  await expect(page.getByText("59 shown", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: /^Expand/ }).first().click();
  await expect(page.getByText("event index").first()).toBeVisible();

  const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (violation) => violation.impact === "serious" || violation.impact === "critical",
  );
  expect(violations).toEqual([]);
  const metrics = await page.evaluate(() => {
    const measuredPerformance = performance as Performance & {
      memory?: { usedJSHeapSize: number };
    };
    return {
      domElements: document.querySelectorAll("*").length,
      renderedRecords: document.querySelectorAll('[role="log"] article').length,
      usedJsHeapBytes: measuredPerformance.memory?.usedJSHeapSize ?? null,
    };
  });
  expect(metrics.domElements).toBeLessThan(1_500);
  expect(metrics.renderedRecords).toBeLessThan(60);
  if (metrics.usedJsHeapBytes !== null) {
    expect(metrics.usedJsHeapBytes).toBeLessThan(256 * 1024 * 1024);
  }
  await capture(page, "rstorrent-diagnostic-console-wide.png");

  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("row", { name: /Big Buck Bunny/ }).click();
  await page.getByRole("tab", { name: "Logs" }).click();
  const phoneFeed = page.getByRole("log", {
    name: "Chronological diagnostic events",
  });
  await expect(phoneFeed).toBeVisible();
  expect(await phoneFeed.locator("article").count()).toBeLessThan(60);
  await capture(page, "rstorrent-diagnostic-console-phone.png");
  console.log(`diagnostic_scale_metrics ${JSON.stringify(metrics)}`);
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
  await page.getByRole("tab", { name: "Swarm" }).click();
  const swarm = page.getByRole("grid", { name: "Known swarm peers" });
  await expect(swarm).toHaveAttribute("aria-rowcount", "1001");
  expect(await swarm.getByRole("row").count()).toBeLessThanOrEqual(100);
  expect(await page.locator("*").count()).toBeLessThan(2_000);
  const primary = page.getByRole("navigation", { name: "Primary" });
  await primary.getByRole("button", { name: "Transfers" }).click();
  const transferQueue = page.getByRole("grid", { name: "Transfer queue" });
  await expect(transferQueue).toHaveAttribute("aria-rowcount", "2001");
  expect(await transferQueue.getByRole("row").count()).toBeLessThanOrEqual(100);
  await primary.getByRole("button", { name: "Library" }).click();
  const content = page.getByRole("list", { name: "Torrent-backed content" });
  const contentCards = content.getByRole("listitem");
  expect(await contentCards.count()).toBeLessThanOrEqual(100);
  await expect(contentCards.first()).toHaveAttribute("aria-setsize", "2000");
  expect(await page.locator("*").count()).toBeLessThan(2_000);
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
  await expect(files.getByRole("checkbox").first()).toBeVisible();
  await files.getByRole("row").nth(1).click();
  await page.getByRole("button", { name: "More file actions" }).click();
  const fileActions = page.getByRole("menu", { name: "More file actions" });
  await expect(
    fileActions.getByRole("menuitem", { name: "Normal", exact: true }),
  ).toBeDisabled();
  await expect(
    fileActions.getByRole("menuitem", { name: "Skip", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByText(
      "File priority changes are unavailable in demo scenarios.",
    ),
  ).toBeVisible();
  await capture(page, "rstorrent-file-actions-wide.png");
  await page.keyboard.press("Escape");
  await files.getByRole("row").nth(1).focus();
  await page.keyboard.press("ArrowDown");
  await expect(files.getByRole("row").nth(2)).toHaveAttribute(
    "aria-current",
    "true",
  );
  await page.keyboard.press("Shift+ArrowDown");
  await expect(files).toHaveAttribute("aria-multiselectable", "true");
  await expect(page.getByText("2 selected for actions")).toBeVisible();
  await expect(files.getByRole("row").nth(3)).toHaveAttribute(
    "aria-current",
    "true",
  );
  await page.keyboard.press("Control+a");
  await expect(page.getByText("4,095 selected for actions")).toBeVisible();
  expect(await files.getByRole("row").count()).toBeLessThanOrEqual(100);
  await page.keyboard.press("Escape");
  await expect(
    files.getByRole("checkbox", { name: "Select asset-001.mkv" }),
  ).not.toBeChecked();
  await expect(
    files.getByRole("checkbox", { name: "Deselect asset-003.mp4" }),
  ).toBeChecked();

  const columns = page.getByRole("button", { name: "Columns" }).last();
  await columns.click();
  await page.getByRole("checkbox", { name: "Storage Path" }).check();
  await capture(page, "rstorrent-columns-wide.png");
  await page.keyboard.press("Escape");
  await expect(files.getByRole("columnheader", { name: "Storage Path" })).toBeVisible();
  const nameResize = files.getByRole("separator", { name: "Resize Name column" });
  const initialWidth = Number(await nameResize.getAttribute("aria-valuenow"));
  await nameResize.focus();
  await page.keyboard.press("ArrowRight");
  await expect(nameResize).toHaveAttribute("aria-valuenow", String(initialWidth + 12));
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
        .getByRole("navigation", { name: "Workbench torrent filters" })
        .evaluate((element) => Math.round(element.getBoundingClientRect().right)),
    )
    .toBeLessThanOrEqual(0);
  await columns.click();
  await expect(
    page.getByRole("dialog", { name: "Table column settings" }),
  ).toBeVisible();
  await capture(page, "rstorrent-columns-phone.png");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "More file actions" }).click();
  const phoneFileActions = page.getByRole("menu", {
    name: "More file actions",
  });
  await expect(
    phoneFileActions.getByRole("menuitem", { name: "Normal", exact: true }),
  ).toBeVisible();
  await expect(
    phoneFileActions.getByRole("menuitem", { name: "Skip", exact: true }),
  ).toBeVisible();
  const phoneMenuBounds = await phoneFileActions
    .locator("..")
    .locator("..")
    .boundingBox();
  expect(phoneMenuBounds).not.toBeNull();
  expect(phoneMenuBounds!.x).toBeGreaterThanOrEqual(7);
  expect(phoneMenuBounds!.y).toBeGreaterThanOrEqual(7);
  expect(phoneMenuBounds!.x + phoneMenuBounds!.width).toBeLessThanOrEqual(383);
  expect(phoneMenuBounds!.y + phoneMenuBounds!.height).toBeLessThanOrEqual(837);
  await capture(page, "rstorrent-file-actions-phone.png");
  await page.keyboard.press("Escape");
  await capture(page, "rstorrent-files-phone.png");
  console.log(`file_scale_metrics ${JSON.stringify({ ...metrics, updateRenderMs })}`);
});

async function openScenario(page: Page, scenario: string, at: number) {
  await page.goto(`/?demo=${scenario}&at=${at}&autoplay=0`);
  await expect(page.getByText("RSTorrent", { exact: true })).toBeVisible();
  await expect(page.getByText("Demo data", { exact: true })).toBeVisible();
  await page
    .getByRole("navigation", { name: "Primary" })
    .getByRole("button", { name: "Workbench" })
    .click();
}

async function tabGeometry(page: Page) {
  return page.getByRole("tab").evaluateAll((elements) =>
    elements.map((element) => ({
      left: (element as HTMLElement).offsetLeft,
      width: (element as HTMLElement).offsetWidth,
    })),
  );
}

async function elementHeight(locator: ReturnType<Page["locator"]>) {
  return Math.round((await locator.boundingBox())?.height ?? 0);
}

async function themeMetrics(page: Page) {
  return page.evaluate(() => {
    const rootStyle = getComputedStyle(document.documentElement);
    return {
      colorScheme: rootStyle.colorScheme,
      background: getComputedStyle(document.body).backgroundColor,
    };
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
