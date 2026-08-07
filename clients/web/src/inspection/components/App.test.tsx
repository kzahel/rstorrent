// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { createRef } from "react";

import type { ClientSettingsRuntimeView } from "../../api";
import { clientSettingsRuntimeFixture } from "../../test-support/client-settings";
import { APPEARANCE_STORAGE_KEY, type AppearanceStorage } from "../appearance";
import type { InspectionApplication } from "../application";
import { InspectionProvider } from "../context";
import { InspectionController } from "../controller";
import { DemoApplication } from "../demo/DemoApplication";
import {
  DEMO_PRIMARY_TORRENT_ID,
  buildScenarioSnapshot,
} from "../demo/catalog";
import type {
  CommandResult,
  DownloadStorageSettings,
  DesiredInspectionViews,
  InspectionCommand,
  InspectionUpdate,
} from "../model";
import { WEBTORRENT_TEST_TORRENTS } from "../testTorrents";
import { App } from "./App";
import { RemoveTorrentDialog } from "./RemoveTorrentDialog";

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  globalThis.requestAnimationFrame = (callback) => {
    return window.setTimeout(() => callback(0), 0);
  };
  globalThis.cancelAnimationFrame = (handle) => window.clearTimeout(handle);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(
    () =>
      ({
        setTransform: vi.fn(),
        clearRect: vi.fn(),
        fillRect: vi.fn(),
        beginPath: vi.fn(),
        moveTo: vi.fn(),
        lineTo: vi.fn(),
        stroke: vi.fn(),
      }) as unknown as CanvasRenderingContext2D,
  );
});

const controllers: InspectionController[] = [];

afterEach(async () => {
  cleanup();
  Reflect.deleteProperty(navigator, "clipboard");
  document.documentElement.removeAttribute("data-color-theme");
  document.documentElement.removeAttribute("data-interface-size");
  if (typeof globalThis.localStorage?.clear === "function") {
    globalThis.localStorage.clear();
  }
  await Promise.all(
    controllers.splice(0).map((controller) => controller.close()),
  );
});

describe("inspection application", () => {
  it("renders the responsive hierarchy and changes detail tabs", async () => {
    const user = userEvent.setup();
    renderScenario("healthy-download", 42_000);
    const header = screen.getByRole("banner");
    expect(within(header).queryByText("Inspection")).not.toBeInTheDocument();
    expect(within(header).queryByText(/peers/i)).not.toBeInTheDocument();
    const primary = screen.getByRole("navigation", { name: "Primary" });
    expect(
      within(primary).getByRole("button", { name: "Transfers" }),
    ).toHaveAttribute("aria-current", "page");
    expect(
      screen.getByRole("navigation", { name: "Transfer filters" }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Add demo" })).toBeVisible();
    expect(
      screen.queryByRole("textbox", { name: "Magnet link or torrent URL" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("grid", { name: "Transfer queue" }),
    ).toHaveAttribute("aria-rowcount", "4");
    await user.click(
      within(primary).getByRole("button", { name: "Workbench" }),
    );
    expect(
      screen.getByRole("navigation", { name: "Workbench torrent filters" }),
    ).toBeVisible();
    expect(
      screen.getByRole("grid", { name: "Torrent library" }),
    ).toHaveAttribute("aria-rowcount", "4");
    expect(
      screen.getByRole("grid", { name: "Active peer connections" }),
    ).toBeVisible();
    const peersTab = screen.getByRole("tab", { name: "Peers" });
    expect(peersTab).toHaveTextContent(/^Peers$/);
    expect(screen.getByRole("tab", { name: "Trackers" })).toHaveTextContent(
      /^Trackers$/,
    );
    await user.click(screen.getByRole("button", { name: "Explain Flags" }));
    const flagLegend = screen.getByRole("dialog", {
      name: "Flags column help",
    });
    expect(within(flagLegend).getByText("Incoming")).toBeVisible();
    expect(within(flagLegend).getByText("Encrypted")).toBeVisible();
    expect(
      within(flagLegend).queryByText(/case-sensitive/),
    ).not.toBeInTheDocument();
    expect(
      within(flagLegend).queryByText(/remote peer initiated/),
    ).not.toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(flagLegend).not.toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "General" }));
    expect(screen.getByText("Current transfer")).toBeVisible();
    expect(peersTab).toHaveTextContent(/^Peers$/);
    await user.click(screen.getByRole("tab", { name: "Logs" }));
    expect(
      screen.getByRole("log", { name: "Chronological diagnostic events" }),
    ).toBeVisible();
  });

  it("explains and opens torrent errors from status", async () => {
    const user = userEvent.setup();
    renderScenario("disk-error", 8_000);

    const transfers = screen.getByRole("grid", { name: "Transfer queue" });
    const status = within(transfers).getByRole("button", {
      name: "Error: Write failed: destination has no free space. Open General details",
    });
    expect(status).toHaveAttribute(
      "title",
      "Write failed: destination has no free space\nOpen General details.",
    );
    expect(
      within(transfers).getByRole("checkbox", {
        name: "Deselect Big Buck Bunny — storage failure",
      }),
    ).toBeChecked();
    expect(
      within(transfers).queryByRole("button", { name: "Complete" }),
    ).not.toBeInTheDocument();

    status.focus();
    await user.keyboard("{Enter}");

    expect(screen.getByRole("button", { name: "Workbench" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByRole("tab", { name: "General" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    const detail = screen.getByRole("region", { name: "Torrent details" });
    const error = within(detail).getByRole("alert");
    expect(error).toHaveTextContent(
      "Storage needs attentionWrite failed: destination has no free space",
    );
    expect(error).toHaveFocus();
    expect(
      within(screen.getByRole("grid", { name: "Torrent library" })).getByRole(
        "button",
        {
          name: "error: Write failed: destination has no free space. Open General details",
        },
      ),
    ).toBeVisible();
  });

  it("renders the bounded swarm registry independently of active connections", async () => {
    const user = userEvent.setup();
    renderScenario("swarm-lifecycle", 24_000);
    await user.click(screen.getByRole("button", { name: "Workbench" }));
    await user.click(screen.getByRole("tab", { name: "Swarm" }));

    const grid = await screen.findByRole("grid", { name: "Known swarm peers" });
    expect(grid).toHaveAttribute("aria-rowcount", "9");
    expect(screen.getByLabelText("Swarm registry summary")).toHaveTextContent(
      "8known",
    );
    expect(within(grid).getAllByText(/backed off/i).length).toBeGreaterThan(0);
    expect(
      within(grid).getAllByText(/TRK|DHT|TRACKER/i).length,
    ).toBeGreaterThan(0);
  });

  it("drives an ordered diagnostic console with separate capture controls", async () => {
    const user = userEvent.setup();
    const rendered = renderScenario("diagnostic-console", 45_000);
    await user.click(screen.getByRole("button", { name: "Workbench" }));
    await user.click(screen.getByRole("tab", { name: "Logs" }));

    const feed = screen.getByRole("log", {
      name: "Chronological diagnostic events",
    });
    expect(feed).toBeVisible();
    expect(rendered.container.querySelectorAll("article").length).toBeLessThan(
      60,
    );
    expect(screen.getByText(/retained$/)).toHaveTextContent("2,048 retained");

    const captureScope = screen.getByLabelText("Diagnostic capture scope");
    await user.selectOptions(captureScope, DEMO_PRIMARY_TORRENT_ID);
    await user.click(screen.getByRole("row", { name: /Sintel 4K open movie/ }));
    expect(captureScope).toHaveValue(DEMO_PRIMARY_TORRENT_ID);

    await user.selectOptions(
      screen.getByLabelText("Diagnostic capture profile"),
      "trace",
    );
    expect(screen.getByText("High-volume producer capture")).toBeVisible();
    await user.selectOptions(
      screen.getByLabelText("Minimum displayed severity"),
      "warning",
    );
    await user.selectOptions(
      screen.getByLabelText("Displayed torrent scope"),
      "all",
    );
    expect(screen.getByText(/shown$/)).toHaveTextContent("410 shown");

    await user.type(
      screen.getByRole("searchbox", { name: "Search diagnostics" }),
      "watermark",
    );
    expect(screen.getByText(/shown$/)).toHaveTextContent("59 shown");
  });

  it("changes and restores complete appearance settings", async () => {
    const user = userEvent.setup();
    let storedAppearance: string | null = null;
    const appearanceStorage: AppearanceStorage = {
      getItem: (key) =>
        key === APPEARANCE_STORAGE_KEY ? storedAppearance : null,
      setItem: (key, value) => {
        if (key === APPEARANCE_STORAGE_KEY) storedAppearance = value;
      },
    };
    const first = renderScenario("healthy-download", 42_000, appearanceStorage);
    const app = first.container.firstElementChild;
    expect(app).toHaveAttribute("data-interface-size", "standard");
    expect(document.documentElement).toHaveAttribute(
      "data-color-theme",
      "auto",
    );

    for (const name of ["Start", "Pause", "Remove"]) {
      expect(
        screen.getByRole("button", { name }).querySelector("svg"),
      ).not.toBeNull();
    }
    await user.click(screen.getByRole("button", { name: "More" }));
    expect(
      screen.getByRole("menuitem", { name: "Archive" }).querySelector("svg"),
    ).not.toBeNull();
    await user.keyboard("{Escape}");

    const settings = screen.getByRole("button", {
      name: "Settings",
    });
    await user.click(settings);
    const dialog = screen.getByRole("dialog", { name: "Settings" });
    const close = within(dialog).getByRole("button", {
      name: "Close settings",
    });
    expect(close).toHaveFocus();
    const colorThemeGroup = within(dialog).getByRole("group", {
      name: "Color theme",
    });
    expect(
      within(colorThemeGroup).getByRole("radio", { name: /Auto/ }),
    ).toBeChecked();
    const dataUnitsGroup = within(dialog).getByRole("group", {
      name: "Data units",
    });
    expect(
      within(dataUnitsGroup).getByRole("radio", { name: /Decimal/ }),
    ).toBeChecked();
    expect(screen.getAllByText(/(?:kB|MB)(?:\/s)?$/).length).toBeGreaterThan(0);

    await user.tab({ shift: true });
    expect(
      within(dataUnitsGroup).getByRole("radio", { name: /Binary/ }),
    ).toHaveFocus();
    await user.click(
      within(dataUnitsGroup).getByRole("radio", { name: /Binary/ }),
    );
    expect(screen.getAllByText(/(?:KiB|MiB)(?:\/s)?$/).length).toBeGreaterThan(
      0,
    );
    await user.click(within(dialog).getByRole("radio", { name: /Dark/ }));
    expect(document.documentElement).toHaveAttribute(
      "data-color-theme",
      "dark",
    );
    await user.click(within(dialog).getByRole("radio", { name: /Spacious/ }));
    expect(app).toHaveAttribute("data-interface-size", "spacious");
    expect(JSON.parse(storedAppearance ?? "null")).toEqual({
      version: 3,
      interfaceSize: "spacious",
      colorTheme: "dark",
      dataUnits: "binary",
    });

    await user.keyboard("{Escape}");
    expect(
      screen.queryByRole("dialog", { name: "Settings" }),
    ).not.toBeInTheDocument();
    expect(settings).toHaveFocus();

    first.unmount();
    const restored = renderScenario(
      "healthy-download",
      42_000,
      appearanceStorage,
    );
    expect(restored.container.firstElementChild).toHaveAttribute(
      "data-interface-size",
      "spacious",
    );
    expect(document.documentElement).toHaveAttribute(
      "data-color-theme",
      "dark",
    );
    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect(
      within(screen.getByRole("group", { name: "Data units" })).getByRole(
        "radio",
        { name: /Binary/ },
      ),
    ).toBeChecked();
  });

  it("switches named scenarios and advances the frozen clock", async () => {
    const user = userEvent.setup();
    renderScenario("tracker-recovery", 0);
    await user.click(screen.getByRole("button", { name: "Workbench" }));
    await user.click(screen.getByRole("tab", { name: "General" }));
    expect(screen.getByText(/retry scheduled in 22 seconds/i)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "+10s" }));
    await user.click(screen.getByRole("button", { name: "+10s" }));
    await user.click(screen.getByRole("button", { name: "+10s" }));
    expect(screen.getByText(/recovered tracker cohort/i)).toBeVisible();
    await user.selectOptions(
      screen.getByLabelText("Demo scenario"),
      "disk-error",
    );
    expect(screen.getAllByText(/storage failure/i)[0]).toBeVisible();
  });

  it("keeps rendered rows and cards bounded for large logical collections", async () => {
    const user = userEvent.setup();
    renderScenario("large-swarm", 0);
    const transferGrid = screen.getByRole("grid", { name: "Transfer queue" });
    expect(transferGrid).toHaveAttribute("aria-rowcount", "2001");
    expect(within(transferGrid).getAllByRole("row").length).toBeLessThanOrEqual(
      100,
    );

    await user.click(screen.getByRole("button", { name: "Library" }));
    const library = screen.getByRole("list", {
      name: "Torrent-backed content",
    });
    expect(within(library).getAllByRole("listitem").length).toBeLessThan(100);

    await user.click(screen.getByRole("button", { name: "Workbench" }));
    const torrentGrid = screen.getByRole("grid", { name: "Torrent library" });
    const peerGrid = screen.getByRole("grid", {
      name: "Active peer connections",
    });
    expect(torrentGrid).toHaveAttribute("aria-rowcount", "2001");
    expect(peerGrid).toHaveAttribute("aria-rowcount", "10001");
    expect(within(torrentGrid).getAllByRole("row").length).toBeLessThanOrEqual(
      100,
    );
    expect(within(peerGrid).getAllByRole("row").length).toBeLessThanOrEqual(
      100,
    );
  });

  it("materializes a full file catalog only on the Files tab", async () => {
    const user = userEvent.setup();
    renderScenario("file-progress", 24_000);
    await user.click(screen.getByRole("button", { name: "Workbench" }));
    expect(
      screen.queryByRole("grid", { name: "Torrent files" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Files" }));
    const files = screen.getByRole("grid", { name: "Torrent files" });
    expect(files).toHaveAttribute("aria-rowcount", "4096");
    expect(within(files).getAllByRole("row").length).toBeLessThanOrEqual(100);
    expect(screen.getByText("1 padding hidden")).toBeVisible();
    expect(within(files).getAllByRole("checkbox").length).toBeGreaterThan(1);
    expect(
      within(files).getByRole("checkbox", { name: "Select asset-001.mkv" }),
    ).not.toBeChecked();
    const firstFile = within(files).getAllByRole("row")[1]!;
    await user.click(firstFile);
    await user.click(screen.getByRole("button", { name: "More file actions" }));
    const fileActions = screen.getByRole("menu", {
      name: "More file actions",
    });
    expect(
      within(fileActions).getByRole("menuitem", { name: "Normal" }),
    ).toHaveAttribute("aria-disabled", "true");
    expect(
      within(fileActions).getByRole("menuitem", { name: "Skip" }),
    ).toHaveAttribute("aria-disabled", "true");
    expect(
      screen.getByText(
        "File priority changes are unavailable in demo scenarios.",
      ),
    ).toBeVisible();
    await user.keyboard("{Escape}");

    firstFile.focus();
    await user.keyboard("{ArrowDown}");
    const secondFile = within(files).getAllByRole("row")[2]!;
    expect(secondFile).toHaveAttribute("aria-current", "true");
    await user.keyboard("{Shift>}{ArrowDown}{/Shift}");
    expect(files).toHaveAttribute("aria-multiselectable", "true");
    expect(screen.getByText("2 selected for actions")).toBeVisible();
    expect(within(files).getAllByRole("row")[3]).toHaveAttribute(
      "aria-current",
      "true",
    );
    await user.keyboard("{Control>}a{/Control}");
    expect(screen.getByText("4,095 selected for actions")).toBeVisible();
    expect(within(files).getAllByRole("row").length).toBeLessThanOrEqual(100);
    await user.keyboard("{Escape}");
    expect(
      within(files).getByRole("checkbox", { name: "Select asset-001.mkv" }),
    ).not.toBeChecked();
    expect(
      within(files).getByRole("checkbox", { name: "Deselect asset-003.mp4" }),
    ).toBeChecked();

    await user.click(
      screen.getAllByRole("button", { name: "Columns" }).at(-1)!,
    );
    await user.click(screen.getByRole("checkbox", { name: "Storage Path" }));
    await user.keyboard("{Escape}");
    expect(
      within(files).getByRole("columnheader", { name: /Storage Path/ }),
    ).toBeVisible();
  });

  it("sends Skip and Normal for the active live torrent files", async () => {
    const user = userEvent.setup();
    const snapshot = buildScenarioSnapshot("file-progress", 24_000, false, 1);
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: { ...snapshot, demo: null },
    });
    const firstFileSet = snapshot.filesByTorrent[DEMO_PRIMARY_TORRENT_ID]!;
    const firstFile = firstFileSet.order
      .map((id) => firstFileSet.rows[id])
      .find((row) => row?.padding === false)!;
    renderApplication(application);

    await user.click(screen.getByRole("button", { name: "Workbench" }));
    await user.click(screen.getByRole("tab", { name: "Files" }));
    const files = screen.getByRole("grid", { name: "Torrent files" });
    await user.click(within(files).getAllByRole("row")[1]!);
    await user.click(screen.getByRole("button", { name: "More file actions" }));
    const skip = screen.getByRole("menuitem", { name: "Skip" });
    expect(skip).not.toHaveAttribute("aria-disabled");
    await user.click(skip);

    await waitFor(() =>
      expect(application.commands.at(-1)).toEqual({
        type: "set_file_priority",
        torrentId: DEMO_PRIMARY_TORRENT_ID,
        fileIndices: [firstFile.index],
        priority: "skip",
      }),
    );

    await user.click(screen.getByRole("button", { name: "More file actions" }));
    const normal = screen.getByRole("menuitem", { name: "Normal" });
    expect(normal).not.toHaveAttribute("aria-disabled");
    await user.click(normal);
    await waitFor(() =>
      expect(application.commands.at(-1)).toEqual({
        type: "set_file_priority",
        torrentId: DEMO_PRIMARY_TORRENT_ID,
        fileIndices: [firstFile.index],
        priority: "normal",
      }),
    );
  });

  it("shares file priority actions between context and More menus", async () => {
    const user = userEvent.setup();
    const snapshot = buildScenarioSnapshot("file-progress", 24_000, false, 1);
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: { ...snapshot, demo: null },
    });
    const fileSet = snapshot.filesByTorrent[DEMO_PRIMARY_TORRENT_ID]!;
    const ordinaryFiles = fileSet.order
      .map((id) => fileSet.rows[id])
      .filter((row) => row?.padding === false)
      .slice(0, 2);
    renderApplication(application);

    await user.click(screen.getByRole("button", { name: "Workbench" }));
    await user.click(screen.getByRole("tab", { name: "Files" }));
    const grid = screen.getByRole("grid", { name: "Torrent files" });
    const firstRow = within(grid).getAllByRole("row")[1]!;
    fireEvent.contextMenu(firstRow, { clientX: 120, clientY: 160 });
    const contextMenu = await screen.findByRole("menu");
    expect(
      within(contextMenu).getByRole("group", { name: "Priority" }),
    ).toBeVisible();
    await user.click(
      within(contextMenu).getByRole("menuitem", { name: "Skip" }),
    );
    await waitFor(() =>
      expect(application.commands.at(-1)).toEqual({
        type: "set_file_priority",
        torrentId: DEMO_PRIMARY_TORRENT_ID,
        fileIndices: [ordinaryFiles[0]!.index],
        priority: "skip",
      }),
    );

    await user.click(
      within(grid).getByRole("checkbox", {
        name: `Select ${ordinaryFiles[1]!.name}`,
      }),
    );
    fireEvent.contextMenu(firstRow, { clientX: 120, clientY: 160 });
    await user.click(screen.getByRole("menuitem", { name: "Normal" }));
    await waitFor(() =>
      expect(application.commands.at(-1)).toEqual({
        type: "set_file_priority",
        torrentId: DEMO_PRIMARY_TORRENT_ID,
        fileIndices: ordinaryFiles.map((file) => file!.index),
        priority: "normal",
      }),
    );
  });

  it("materializes the global disk pipeline only on the Disk tab", async () => {
    const user = userEvent.setup();
    renderScenario("slow-disk-pressure", 20_000);
    await user.click(screen.getByRole("button", { name: "Workbench" }));
    expect(
      screen.queryByRole("grid", { name: "Active storage pieces" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Disk" }));
    expect(
      screen.getByText("Receive → write → verify → checkpoint"),
    ).toBeVisible();
    expect(screen.getByLabelText("Disk pressure Backpressured")).toBeVisible();
    const pieces = screen.getByRole("grid", { name: "Active storage pieces" });
    expect(pieces).toHaveAttribute("aria-rowcount", "65");
    expect(within(pieces).getAllByRole("row").length).toBeLessThanOrEqual(100);
    expect(screen.getByText("intake paused now")).toBeVisible();
  });

  it("renders a bounded accessible canvas for a 250,000-piece torrent", async () => {
    const user = userEvent.setup();
    renderScenario("large-swarm", 0);
    await user.click(screen.getByRole("button", { name: "Workbench" }));

    await user.click(screen.getByRole("tab", { name: "Pieces" }));

    const canvas = screen.getByRole("img", {
      name: /250,000 pieces: 135,000 verified, 6 active/i,
    }) as HTMLCanvasElement;
    expect(canvas).toBeVisible();
    expect(canvas.width).toBeLessThanOrEqual(640 * 3);
    expect(canvas.height).toBeLessThanOrEqual(1_024 * 3);
    expect(document.querySelectorAll("*").length).toBeLessThan(1_500);
    expect(screen.getByLabelText("Piece state legend")).toBeVisible();
  });

  it("resizes the detail pane with pointer and keyboard input", async () => {
    const user = userEvent.setup();
    renderScenario("healthy-download", 42_000);
    await user.click(screen.getByRole("button", { name: "Workbench" }));
    const splitter = screen.getByRole("separator", {
      name: "Resize torrent details",
    });
    const main = screen.getByRole("main");
    expect(splitter).toHaveAttribute("aria-orientation", "horizontal");
    expect(splitter).toHaveAttribute("aria-valuemin", "25");
    expect(splitter).toHaveAttribute("aria-valuemax", "80");
    expect(splitter).toHaveAttribute("aria-valuenow", "57");

    vi.spyOn(main, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      top: 0,
      right: 1_000,
      bottom: 700,
      left: 0,
      width: 1_000,
      height: 700,
      toJSON: () => ({}),
    });
    vi.spyOn(splitter, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 300,
      top: 300,
      right: 1_000,
      bottom: 307,
      left: 0,
      width: 1_000,
      height: 7,
      toJSON: () => ({}),
    });
    Object.defineProperties(splitter, {
      setPointerCapture: { configurable: true, value: vi.fn() },
      hasPointerCapture: { configurable: true, value: vi.fn(() => true) },
      releasePointerCapture: { configurable: true, value: vi.fn() },
    });

    fireEvent.pointerDown(splitter, {
      button: 0,
      pointerId: 7,
      clientY: 300,
    });
    fireEvent.pointerMove(splitter, { pointerId: 7, clientY: 200 });
    expect(splitter).toHaveAttribute("aria-valuenow", "72");
    expect(main.style.getPropertyValue("--detail-pane-share")).toBe("72fr");
    fireEvent.pointerUp(splitter, { pointerId: 7, clientY: 200 });
    expect(main).toHaveAttribute("data-resizing", "false");

    splitter.focus();
    await user.keyboard("{ArrowDown}");
    expect(splitter).toHaveAttribute("aria-valuenow", "67");
    await user.keyboard("{Home}");
    expect(splitter).toHaveAttribute("aria-valuenow", "25");
    await user.keyboard("{End}");
    expect(splitter).toHaveAttribute("aria-valuenow", "80");
  });

  it("confirms removal with retained data by default and restores focus", async () => {
    const user = userEvent.setup();
    renderScenario("healthy-download", 42_000);
    const trigger = screen.getByRole("button", { name: "Remove" });

    await user.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Remove torrent?" });
    const deleteData = within(dialog).getByRole("checkbox", {
      name: "Also delete downloaded data",
    });
    expect(deleteData).not.toBeChecked();
    expect(
      within(dialog).getByRole("button", { name: "Cancel" }),
    ).toHaveFocus();
    await user.click(deleteData);
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      /cannot be undone/i,
    );
    const destructive = within(dialog).getByRole("button", {
      name: "Remove and delete data",
    });
    destructive.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(deleteData).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();

    await user.click(trigger);
    const reopened = screen.getByRole("dialog", { name: "Remove torrent?" });
    expect(within(reopened).getByRole("checkbox")).not.toBeChecked();
    await user.click(within(reopened).getByRole("button", { name: "Remove" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(
      screen.getByText(/Removed Big Buck Bunny/, { exact: true }),
    ).toBeVisible();
  });

  it("keeps a failed removal dialog actionable", async () => {
    const user = userEvent.setup();
    const returnFocus = createRef<HTMLButtonElement>();
    const target = buildScenarioSnapshot("healthy-download", 42_000, false, 1)
      .torrents[DEMO_PRIMARY_TORRENT_ID]!;
    render(
      <>
        <button ref={returnFocus}>Remove trigger</button>
        <RemoveTorrentDialog
          targets={[target]}
          deleteDataSupported={true}
          returnFocus={() => returnFocus.current?.focus()}
          onCancel={() => {}}
          onConfirm={async () => {
            throw new Error("Provider permission was revoked");
          }}
        />
      </>,
    );
    const dialog = screen.getByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Remove" }));
    expect(within(dialog).getAllByRole("alert").at(-1)).toHaveTextContent(
      "Provider permission was revoked",
    );
    expect(
      within(dialog).getByRole("button", { name: "Retry failed" }),
    ).toBeEnabled();
    expect(
      within(dialog).getByRole("button", { name: "Retry failed" }),
    ).toHaveFocus();
  });

  it("renders truthful empty states without fabricating media details", async () => {
    const user = userEvent.setup();
    renderScenario("empty-library", 0);
    expect(screen.getByText(/No transfers yet/i)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Library" }));
    expect(screen.getByText("No content sources yet")).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /^Play / }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Workbench" }));
    expect(screen.getByText(/Select a torrent to inspect it/i)).toBeVisible();
  });

  it("shares multi-selection between Transfers and Workbench", async () => {
    const user = userEvent.setup();
    renderScenario("healthy-download", 42_000);
    expect(
      screen.getByRole("checkbox", { name: "Select Sintel 4K open movie" }),
    ).not.toBeChecked();
    fireEvent.click(screen.getByRole("row", { name: /Sintel 4K open movie/ }), {
      shiftKey: true,
    });
    expect(screen.getByText("2 selected for actions")).toBeVisible();
    expect(screen.getByRole("button", { name: "Remove" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: /Paused/ }));
    expect(
      screen.getByText("2 selected for actions (1 outside this view)"),
    ).toBeVisible();

    await user.click(screen.getByRole("button", { name: "Workbench" }));
    expect(
      screen.getByRole("checkbox", {
        name: "Deselect Big Buck Bunny 1080p surround",
      }),
    ).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "Deselect Sintel 4K open movie" }),
    ).toBeChecked();
    expect(screen.getByText("2 selected for actions")).toBeVisible();
  });

  it("uses exact torrent context targets and the shared grouped action set", async () => {
    const user = userEvent.setup();
    const snapshot = {
      ...buildScenarioSnapshot("healthy-download", 42_000, false, 1),
      demo: null,
    };
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot,
    });
    renderApplication(application);
    const grid = screen.getByRole("grid", { name: "Transfer queue" });
    const sintelRow = within(grid).getByRole("row", {
      name: /Sintel 4K open movie/,
    });

    fireEvent.contextMenu(sintelRow, { clientX: 180, clientY: 220 });
    const singletonMenu = await screen.findByRole("menu");
    expect(sintelRow).toHaveAttribute("aria-current", "true");
    expect(
      within(singletonMenu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual([
      "Start",
      "Pause",
      "Force recheck",
      "Copy magnet link",
      "Archive",
      "Restore",
      "Remove",
    ]);
    for (const group of [
      "Transfer",
      "Sharing",
      "Organization",
      "Destructive",
    ]) {
      expect(
        within(singletonMenu).getByRole("group", { name: group }),
      ).toBeVisible();
    }
    await user.keyboard("{Escape}");
    expect(
      within(grid).getByRole("checkbox", {
        name: "Deselect Sintel 4K open movie",
      }),
    ).toBeChecked();

    await user.click(
      within(grid).getByRole("checkbox", {
        name: "Select Big Buck Bunny 1080p surround",
      }),
    );
    fireEvent.contextMenu(sintelRow, { clientX: 180, clientY: 220 });
    expect(
      await screen.findByRole("menuitem", { name: "Copy magnet links" }),
    ).toBeVisible();
    await user.click(screen.getByRole("menuitem", { name: "Remove" }));
    const dialog = screen.getByRole("dialog", { name: "Remove 2 torrents?" });
    expect(
      within(dialog).getByRole("list", { name: "Torrents to remove" }),
    ).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(sintelRow).toHaveFocus();
    expect(application.commands).toEqual([]);
  });

  it("opens a torrent context menu from the keyboard for the checked selection", async () => {
    const user = userEvent.setup();
    renderScenario("healthy-download", 42_000);
    const grid = screen.getByRole("grid", { name: "Transfer queue" });
    await user.click(
      within(grid).getByRole("checkbox", {
        name: "Select Sintel 4K open movie",
      }),
    );
    const currentRow = within(grid).getByRole("row", {
      name: /Big Buck Bunny 1080p surround/,
    });
    currentRow.focus();
    await user.keyboard("{Shift>}{F10}{/Shift}");
    expect(
      await screen.findByRole("menuitem", { name: "Copy magnet links" }),
    ).toBeVisible();
  });

  it("keeps the detail row within the checked selection", async () => {
    const user = userEvent.setup();
    renderScenario("healthy-download", 42_000);
    await user.click(screen.getByRole("button", { name: "Workbench" }));
    await user.click(screen.getByRole("tab", { name: "General" }));

    const grid = screen.getByRole("grid", { name: "Torrent library" });
    const detail = screen.getByRole("region", { name: "Torrent details" });
    const bunny = within(grid).getByRole("row", {
      name: /Big Buck Bunny 1080p surround/,
    });
    bunny.focus();
    await user.keyboard("{ArrowDown}");
    const sintel = within(grid).getByRole("row", {
      name: /Sintel 4K open movie/,
    });
    expect(sintel).toHaveAttribute("aria-current", "true");
    expect(
      within(detail).getByRole("heading", { name: "Sintel 4K open movie" }),
    ).toBeVisible();

    await user.keyboard("{Shift>}{ArrowUp}{/Shift}");
    expect(bunny).toHaveAttribute("aria-current", "true");
    expect(screen.getByText("2 selected for actions")).toBeVisible();
    expect(
      within(detail).getByRole("heading", {
        name: "Big Buck Bunny 1080p surround",
      }),
    ).toBeVisible();

    await user.keyboard("{Control>}a{/Control}");
    expect(screen.getByText("3 selected for actions")).toBeVisible();
    await user.keyboard("{ArrowUp}");
    const arch = within(grid).getByRole("row", {
      name: /Arch Linux 2026\.08\.01 x86_64/,
    });
    expect(arch).toHaveAttribute("aria-current", "true");
    expect(
      screen.queryByText("3 selected for actions"),
    ).not.toBeInTheDocument();
    expect(
      within(grid).getByRole("checkbox", {
        name: "Deselect Arch Linux 2026.08.01 x86_64",
      }),
    ).toBeChecked();
    expect(
      within(grid).getByRole("checkbox", {
        name: "Select Big Buck Bunny 1080p surround",
      }),
    ).not.toBeChecked();
    expect(
      within(detail).getByRole("heading", {
        name: "Arch Linux 2026.08.01 x86_64",
      }),
    ).toBeVisible();

    await user.keyboard("{Escape}");
    expect(
      screen.queryByText("3 selected for actions"),
    ).not.toBeInTheDocument();
    expect(arch).toHaveAttribute("aria-current", "true");
  });

  it("targets an ordinary row action and clears it from table background", async () => {
    const user = userEvent.setup();
    const snapshot = {
      ...buildScenarioSnapshot("healthy-download", 42_000, false, 1),
      demo: null,
    };
    const bunny = Object.values(snapshot.torrents).find(
      (torrent) => torrent.name === "Big Buck Bunny 1080p surround",
    )!;
    const sintel = Object.values(snapshot.torrents).find(
      (torrent) => torrent.name === "Sintel 4K open movie",
    )!;
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot,
    });
    renderApplication(application);

    await user.click(screen.getByRole("row", { name: /Sintel 4K open movie/ }));
    await user.click(screen.getByRole("row", { name: /Big Buck Bunny 1080p/ }));
    await user.click(screen.getByRole("button", { name: "Pause" }));
    await waitFor(() =>
      expect(application.commands).toEqual([
        { type: "pause", torrentId: bunny.id },
      ]),
    );
    expect(application.commands).not.toContainEqual({
      type: "pause",
      torrentId: sintel.id,
    });

    fireEvent.click(screen.getByRole("grid", { name: "Transfer queue" }));
    expect(screen.getByRole("button", { name: "Pause" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove" })).toBeDisabled();
  });

  it("runs multi-row commands sequentially and reports partial failure", async () => {
    const user = userEvent.setup();
    const snapshot = {
      ...buildScenarioSnapshot("healthy-download", 42_000, false, 1),
      demo: null,
    };
    const sintel = Object.values(snapshot.torrents).find(
      (torrent) => torrent.name === "Sintel 4K open movie",
    )!;
    const application = new RecordingLiveApplication(
      { type: "snapshot", snapshot },
      sintel.id,
    );
    renderApplication(application);
    await user.click(
      screen.getByRole("checkbox", { name: "Select Sintel 4K open movie" }),
    );
    await user.click(screen.getByRole("button", { name: "More" }));
    await user.click(screen.getByRole("menuitem", { name: "Archive" }));

    await waitFor(() =>
      expect(application.commands).toEqual([
        { type: "archive", torrentId: DEMO_PRIMARY_TORRENT_ID },
        { type: "archive", torrentId: sintel.id },
      ]),
    );
    expect(
      screen.getByText(
        /Archived 1 of 2; 1 failed: Sintel 4K open movie: rejected for test/,
      ),
    ).toBeVisible();
  });

  it("removes multiple torrents and retries only failed targets", async () => {
    const user = userEvent.setup();
    const snapshot = {
      ...buildScenarioSnapshot("healthy-download", 42_000, false, 1),
      demo: null,
    };
    const sintel = Object.values(snapshot.torrents).find(
      (torrent) => torrent.name === "Sintel 4K open movie",
    )!;
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot,
    });
    application.rejectNextTorrentId = sintel.id;
    renderApplication(application);
    await user.click(
      screen.getByRole("checkbox", { name: "Select Sintel 4K open movie" }),
    );
    await user.click(screen.getByRole("button", { name: "Remove" }));
    let dialog = screen.getByRole("dialog", { name: "Remove 2 torrents?" });
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: "Also delete downloaded data",
      }),
    );
    await user.click(
      within(dialog).getByRole("button", { name: "Remove and delete data" }),
    );

    dialog = await screen.findByRole("dialog", { name: "Remove torrent?" });
    expect(within(dialog).getAllByRole("alert").at(-1)).toHaveTextContent(
      /Removed 1 of 2; 1 failed: Sintel 4K open movie: rejected for test/,
    );
    expect(
      within(dialog).getByRole("button", { name: "Retry failed" }),
    ).toBeEnabled();
    await user.click(
      within(dialog).getByRole("button", { name: "Retry failed" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(
      application.commands.filter((command) => command.type === "remove"),
    ).toEqual([
      {
        type: "remove",
        torrentId: DEMO_PRIMARY_TORRENT_ID,
        deleteData: true,
      },
      { type: "remove", torrentId: sintel.id, deleteData: true },
      { type: "remove", torrentId: sintel.id, deleteData: true },
    ]);
  });

  it("selects truthful Library cards and hands their source to Workbench", async () => {
    const user = userEvent.setup();
    renderScenario("healthy-download", 42_000);
    await user.click(screen.getByRole("button", { name: "Library" }));
    expect(
      screen.getByText(/media details are not connected yet/i),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /^Play / }),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", {
        name: "Activate Sintel 4K open movie in Library",
      }),
    );
    await user.click(screen.getByRole("button", { name: "Open in Workbench" }));
    expect(screen.getByRole("button", { name: "Workbench" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    await user.click(screen.getByRole("tab", { name: "General" }));
    expect(screen.getByText("Current transfer")).toBeVisible();
    expect(screen.getAllByText("Sintel 4K open movie").length).toBeGreaterThan(
      0,
    );
  });

  it("leases detail views only while Workbench needs them", async () => {
    const user = userEvent.setup();
    const snapshot = {
      ...buildScenarioSnapshot("healthy-download", 42_000, false, 1),
      demo: null,
    };
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot,
    });
    renderApplication(application);
    await waitFor(() =>
      expect(application.views.at(-1)).toEqual({
        library: true,
        torrentId: null,
        detail: null,
        logCapture: null,
        speed: null,
      }),
    );

    await user.click(screen.getByRole("button", { name: "Workbench" }));
    await waitFor(() =>
      expect(application.views.at(-1)).toMatchObject({
        library: true,
        torrentId: DEMO_PRIMARY_TORRENT_ID,
        detail: "peers",
      }),
    );
    await user.click(screen.getByRole("button", { name: "Library" }));
    await waitFor(() =>
      expect(application.views.at(-1)).toEqual({
        library: true,
        torrentId: null,
        detail: null,
        logCapture: null,
        speed: null,
      }),
    );
  });

  it("requires a folder on first add and retains the magnet across cancellation", async () => {
    const user = userEvent.setup();
    const application = new RecordingLiveApplication();
    renderApplication(application);
    const draft = screen.getByRole("textbox", {
      name: "Magnet link or torrent URL",
    });
    const magnet =
      "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213";
    await user.type(draft, magnet);
    await user.click(screen.getByRole("button", { name: "Add" }));

    let dialog = screen.getByRole("dialog", {
      name: "Choose download options",
    });
    expect(
      within(dialog).getByText(/download folder is required/i),
    ).toBeVisible();
    expect(
      within(dialog).getByRole("button", { name: "Add torrent" }),
    ).toBeDisabled();
    expect(
      within(dialog).getByRole("button", { name: "Cancel" }),
    ).toHaveFocus();
    expect(draft).toHaveValue(magnet);

    await user.click(
      within(dialog).getByRole("button", { name: "Choose folder…" }),
    );
    await waitFor(() =>
      expect(
        within(dialog).getByRole("radio", { name: /Selected Downloads/ }),
      ).toBeChecked(),
    );
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(
      screen.queryByRole("dialog", { name: "Choose download options" }),
    ).not.toBeInTheDocument();
    expect(draft).toHaveValue(magnet);
    expect(
      application.commands.filter((command) => command.type === "add_magnet"),
    ).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Add" }));
    dialog = screen.getByRole("dialog", { name: "Choose download options" });
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: /Don’t show these options again/,
      }),
    );
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: /Start downloading files when metadata is available/,
      }),
    );
    await user.click(
      within(dialog).getByRole("button", { name: "Add torrent" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Choose download options" }),
      ).not.toBeInTheDocument(),
    );
    expect(draft).toHaveValue("");
    expect(application.commands).toEqual([
      { type: "choose_download_root" },
      {
        type: "add_magnet",
        magnet,
        storageRoot: "root_1",
        startContent: false,
      },
      { type: "set_show_add_options", show: false },
    ]);
  });

  it("opens the hidden torrent chooser only for empty pointer or keyboard Add", async () => {
    const user = userEvent.setup();
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: liveSnapshot({
        roots: [downloadRoot("root_a", "Downloads")],
        defaultRoot: "root_a",
        showAddOptions: false,
      }),
    });
    renderApplication(application);
    const fileInput = torrentFileInput();
    const openChooser = vi.spyOn(fileInput, "click");
    const add = screen.getByRole("button", { name: "Add" });
    const draft = screen.getByRole("textbox", {
      name: "Magnet link or torrent URL",
    });

    expect(fileInput).toHaveAttribute(
      "accept",
      ".torrent,application/x-bittorrent",
    );
    expect(fileInput).not.toHaveAttribute("multiple");
    await user.click(add);
    expect(openChooser).toHaveBeenCalledOnce();
    draft.focus();
    await user.keyboard("{Enter}");
    expect(openChooser).toHaveBeenCalledTimes(2);
    expect(application.commands).toEqual([]);
    expect(screen.getByRole("status")).toHaveTextContent("");

    await user.type(draft, "not a magnet");
    await user.click(add);
    expect(openChooser).toHaveBeenCalledTimes(2);
    expect(
      screen.getByText("Enter a magnet link beginning with magnet:?"),
    ).toBeVisible();
    expect(application.commands).toEqual([]);
  });

  it("uploads one selected file immediately through the usable default root", async () => {
    const user = userEvent.setup();
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: liveSnapshot({
        roots: [downloadRoot("root_a", "Downloads")],
        defaultRoot: "root_a",
        showAddOptions: false,
      }),
    });
    renderApplication(application);
    const bytes = new Uint8Array([100, 52, 58, 105, 110, 102, 111, 101]);
    const source = bytes.buffer.slice(0) as ArrayBuffer;
    const file = new File([bytes], "private-name.torrent", {
      type: "application/octet-stream",
    });
    const read = vi.fn().mockResolvedValue(source);
    Object.defineProperty(file, "arrayBuffer", { value: read });
    const fileInput = torrentFileInput();

    await user.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.change(fileInput, { target: { files: [file] } });

    await waitFor(() =>
      expect(application.commands).toContainEqual({
        type: "add_torrent_bytes",
        source,
        storageRoot: "root_a",
        startContent: true,
      }),
    );
    expect(read).toHaveBeenCalledOnce();
    expect(fileInput).toHaveValue("");
    expect(JSON.stringify(application.commands)).not.toContain("private-name");
    expect(screen.getByText("Torrent added", { exact: true })).toBeVisible();
  });

  it("retains only the File while options are chosen and applies them after read", async () => {
    const user = userEvent.setup();
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: liveSnapshot({
        roots: [
          downloadRoot("root_a", "Default Downloads"),
          downloadRoot("root_b", "External Drive"),
        ],
        defaultRoot: "root_a",
        showAddOptions: true,
      }),
    });
    renderApplication(application);
    const source = new Uint8Array([1, 2, 3, 4]).buffer;
    const file = new File([new Uint8Array(source)], "options.torrent", {
      type: "application/x-bittorrent",
    });
    const read = vi.fn().mockResolvedValue(source);
    Object.defineProperty(file, "arrayBuffer", { value: read });

    await user.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.change(torrentFileInput(), { target: { files: [file] } });
    const dialog = screen.getByRole("dialog", {
      name: "Choose download options",
    });
    expect(read).not.toHaveBeenCalled();
    expect(application.commands).toEqual([]);

    await user.click(
      within(dialog).getByRole("radio", { name: /External Drive/ }),
    );
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: /Start downloading files when metadata is available/,
      }),
    );
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: /Don’t show these options again/,
      }),
    );
    await user.click(
      within(dialog).getByRole("button", { name: "Add torrent" }),
    );

    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Choose download options" }),
      ).not.toBeInTheDocument(),
    );
    expect(read).toHaveBeenCalledOnce();
    expect(application.commands).toEqual([
      {
        type: "add_torrent_bytes",
        source,
        storageRoot: "root_b",
        startContent: false,
      },
      { type: "set_show_add_options", show: false },
    ]);
  });

  it("rejects empty files and keeps a dialog file available after a read failure", async () => {
    const user = userEvent.setup();
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: liveSnapshot({
        roots: [downloadRoot("root_a", "Downloads")],
        defaultRoot: "root_a",
        showAddOptions: true,
      }),
    });
    renderApplication(application);
    const fileInput = torrentFileInput();
    fireEvent.change(fileInput, {
      target: { files: [new File([], "empty.torrent")] },
    });
    expect(
      screen.getByText("Torrent files must contain at least one byte."),
    ).toBeVisible();
    expect(application.commands).toEqual([]);

    const source = new Uint8Array([1, 2, 3]).buffer;
    const file = new File([new Uint8Array(source)], "retry.torrent");
    const read = vi
      .fn()
      .mockRejectedValueOnce(new Error("permission denied"))
      .mockResolvedValueOnce(source);
    Object.defineProperty(file, "arrayBuffer", { value: read });
    fireEvent.change(fileInput, { target: { files: [file] } });
    const dialog = screen.getByRole("dialog", {
      name: "Choose download options",
    });
    await user.click(
      within(dialog).getByRole("button", { name: "Add torrent" }),
    );
    expect(
      await within(dialog).findByText(
        "Could not read the torrent file: permission denied",
      ),
    ).toBeVisible();
    expect(application.commands).toEqual([]);

    await user.click(
      within(dialog).getByRole("button", { name: "Add torrent" }),
    );
    await waitFor(() =>
      expect(application.commands).toContainEqual({
        type: "add_torrent_bytes",
        source,
        storageRoot: "root_a",
        startContent: true,
      }),
    );
    expect(read).toHaveBeenCalledTimes(2);
  });

  it("resets same-file selection and blocks duplicate submission while reading", async () => {
    const user = userEvent.setup();
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: liveSnapshot({
        roots: [downloadRoot("root_a", "Downloads")],
        defaultRoot: "root_a",
        showAddOptions: false,
      }),
    });
    renderApplication(application);
    const source = new Uint8Array([1, 2, 3]).buffer;
    let finishRead: ((value: ArrayBuffer) => void) | undefined;
    const read = vi.fn(
      () =>
        new Promise<ArrayBuffer>((resolve) => {
          finishRead = resolve;
        }),
    );
    const file = new File([new Uint8Array(source)], "same.torrent");
    Object.defineProperty(file, "arrayBuffer", { value: read });
    const fileInput = torrentFileInput();
    const openChooser = vi.spyOn(fileInput, "click");
    const add = screen.getByRole("button", { name: "Add" });

    await user.click(add);
    fireEvent.change(fileInput, { target: { files: [file] } });
    await waitFor(() => expect(add).toHaveTextContent("Adding…"));
    expect(fileInput).toHaveValue("");
    fireEvent.submit(add.closest("form")!);
    expect(openChooser).toHaveBeenCalledOnce();
    expect(read).toHaveBeenCalledOnce();
    finishRead?.(source);
    await waitFor(() => expect(application.commands).toHaveLength(1));

    fireEvent.change(fileInput, { target: { files: [file] } });
    await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
    finishRead?.(source);
    await waitFor(() => expect(application.commands).toHaveLength(2));
  });

  it("uses an alternate root for one add without changing the default", async () => {
    const user = userEvent.setup();
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: liveSnapshot({
        roots: [
          downloadRoot("root_a", "Default Downloads"),
          downloadRoot("root_b", "External Drive"),
        ],
        defaultRoot: "root_a",
        showAddOptions: true,
      }),
    });
    renderApplication(application);
    const magnet =
      "magnet:?xt=urn:btih:111102030405060708090a0b0c0d0e0f10111213";
    await user.type(
      screen.getByRole("textbox", { name: "Magnet link or torrent URL" }),
      magnet,
    );
    await user.click(screen.getByRole("button", { name: "Add" }));
    const dialog = screen.getByRole("dialog", {
      name: "Choose download options",
    });
    expect(
      within(dialog).getByRole("radio", { name: /Default Downloads/ }),
    ).toBeChecked();
    await user.click(
      within(dialog).getByRole("radio", { name: /External Drive/ }),
    );
    await user.click(
      within(dialog).getByRole("button", { name: "Add torrent" }),
    );
    await waitFor(() =>
      expect(application.commands).toContainEqual({
        type: "add_magnet",
        magnet,
        storageRoot: "root_b",
        startContent: true,
      }),
    );
    expect(application.commands).not.toContainEqual({
      type: "set_default_download_root",
      rootId: "root_b",
    });
  });

  it("manages download roots and the add-options preference in Settings", async () => {
    const user = userEvent.setup();
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: liveSnapshot({
        roots: [
          downloadRoot("root_a", "Downloads"),
          downloadRoot("root_missing", "Missing Drive", "unavailable"),
        ],
        defaultRoot: "root_a",
        showAddOptions: true,
      }),
    });
    renderApplication(application);
    await user.click(screen.getByRole("button", { name: "Settings" }));
    const dialog = screen.getByRole("dialog", { name: "Settings" });
    await user.click(within(dialog).getByRole("tab", { name: "Downloads" }));
    expect(within(dialog).getByText("Default download folder")).toBeVisible();
    expect(
      within(dialog).getByRole("checkbox", {
        name: /Show options when adding torrents/,
      }),
    ).toBeChecked();

    await user.click(within(dialog).getByRole("button", { name: "Repair…" }));
    await waitFor(() =>
      expect(application.commands).toContainEqual({
        type: "choose_download_root",
        repairRoot: "root_missing",
      }),
    );
    await user.click(
      within(dialog).getByRole("button", { name: "Make default" }),
    );
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: /Show options when adding torrents/,
      }),
    );
    await waitFor(() =>
      expect(application.commands).toContainEqual({
        type: "set_show_add_options",
        show: false,
      }),
    );
    expect(within(dialog).getByText(/future torrents only/i)).toBeVisible();
  });

  it("validates and atomically saves connection and seeding settings", async () => {
    const user = userEvent.setup();
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: {
        ...liveSnapshot({ roots: [], defaultRoot: null, showAddOptions: true }),
        clientSettings: {
          ...clientSettingsRuntimeFixture(),
          configured: {
            ...clientSettingsRuntimeFixture().configured,
            tracker_https_server_authentication: "disabled",
          },
          effective_tracker_https_server_authentication: "disabled",
          effective_peer_connection_limit: 120,
        },
      },
    });
    renderApplication(application);
    await user.click(screen.getByRole("button", { name: "Settings" }));
    const dialog = screen.getByRole("dialog", { name: "Settings" });

    expect(within(dialog).getByRole("tab", { name: "Appearance" })).toBeVisible();
    expect(within(dialog).getByRole("tab", { name: "Downloads" })).toBeVisible();
    const connectionTab = within(dialog).getByRole("tab", {
      name: "Connection & seeding",
    });
    expect(connectionTab).toBeVisible();
    await user.click(connectionTab);
    expect(
      within(dialog).getByText(/all IPv4 network interfaces/i),
    ).toBeVisible();
    expect(
      within(dialog).getByRole("radio", { name: /^Automatic port/ }),
    ).toBeChecked();
    expect(within(dialog).queryByRole("radio", { name: /Off/ })).toBeNull();
    expect(within(dialog).queryByText(/device-only/i)).toBeNull();
    expect(
      within(dialog).queryByRole("spinbutton", {
        name: "Preferred automatic port",
      }),
    ).toBeNull();
    expect(
      within(dialog).getByText(
        /safely limited to 120 by available file descriptors/i,
      ),
    ).toBeVisible();

    await user.click(
      within(dialog).getByRole("radio", { name: /^Fixed port/ }),
    );
    const port = within(dialog).getByRole("spinbutton", {
      name: "Fixed listener port",
    });
    expect(port).toHaveValue(null);
    const save = within(dialog).getByRole("button", { name: "Save settings" });
    expect(save).toBeDisabled();

    await user.type(port, "1023");
    expect(
      within(dialog).getByText(/whole number from 1024 to 65535/i),
    ).toBeVisible();
    expect(save).toBeDisabled();
    await user.clear(port);
    await user.type(port, "1024");
    const peers = within(dialog).getByRole("spinbutton", {
      name: "Peer connection limit",
    });
    await user.clear(peers);
    await user.type(peers, "2000");
    const slots = within(dialog).getByRole("spinbutton", {
      name: "Payload upload slots",
    });
    await user.clear(slots);
    await user.type(slots, "0");
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: /Map incoming TCP with UPnP/,
      }),
    );
    expect(
      within(dialog).getByText(/keeps interested peers choked/i),
    ).toBeVisible();
    expect(save).toBeEnabled();

    await user.click(save);
    await waitFor(() =>
      expect(application.commands.at(-1)).toEqual({
        type: "set_client_settings",
        settings: {
          listener: { type: "fixed_local_network", port: 1024 },
          preferred_listen_port: 6881,
          port_mapping: "upnp",
          peer_connection_limit: 2000,
          upload_slots: 0,
          tracker_https_server_authentication: "disabled",
        },
      }),
    );
    expect(
      within(dialog).getByText(/Settings accepted and applying/i),
    ).toBeVisible();
    expect(within(dialog).getByText(/Transport: applying/i)).toBeVisible();
    expect(within(dialog).queryByText(/restart/i)).not.toBeInTheDocument();

    await user.clear(peers);
    await user.type(peers, "1999");
    application.emitClientSettings({
      ...clientSettingsRuntimeFixture(),
      configured: {
        ...clientSettingsRuntimeFixture().configured,
        listener: { type: "fixed_local_network", port: 1024 },
        preferred_listen_port: 6881,
        port_mapping: "upnp",
        peer_connection_limit: 2000,
        upload_slots: 0,
        tracker_https_server_authentication: "system_trust",
      },
      effective_tracker_https_server_authentication: "system_trust",
    });
    expect(peers).toHaveValue(1999);
    await user.click(
      within(dialog).getByRole("button", { name: "Cancel changes" }),
    );
    expect(peers).toHaveValue(2000);

    application.rejectNextClientSettings = true;
    await user.clear(slots);
    await user.type(slots, "1");
    await user.click(save);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "settings save rejected",
    );
    expect(slots).toHaveValue(1);
    expect(save).toBeEnabled();

    await user.click(save);
    await waitFor(() =>
      expect(
        application.commands.filter(
          (command) => command.type === "set_client_settings",
        ),
      ).toHaveLength(3),
    );
    expect(within(dialog).queryByRole("alert")).not.toBeInTheDocument();
  });

  it("reports a recoverable listener bind failure without hiding settings", async () => {
    const user = userEvent.setup();
    const active = {
      listener: { type: "fixed_loopback" as const, port: 51_413 },
      preferred_listen_port: 6_881,
      port_mapping: "disabled" as const,
      peer_connection_limit: 200,
      upload_slots: 8,
      tracker_https_server_authentication: "system_trust" as const,
    };
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: {
        ...liveSnapshot({ roots: [], defaultRoot: null, showAddOptions: true }),
        clientSettings: {
          configured: {
            ...active,
            listener: { type: "automatic_local_network" },
          },
          effective_listener: null,
          effective_port_mapping: "disabled",
          effective_peer_connection_limit: 200,
          effective_upload_slots: 8,
          effective_tracker_https_server_authentication: "system_trust",
          transport_application: {
            type: "degraded",
            reason: "transport_bind_failed",
            detail: "port 51413 is already in use.",
          },
          port_mapping_application: { type: "applied" },
          peer_connections_application: { type: "applied" },
          upload_slots_application: { type: "applied" },
          tracker_https_authentication_application: { type: "applied" },
          listener_status: {
            type: "bind_failed",
            reason: "address_in_use",
            detail: "port 51413 is already in use.",
          },
          session_udp_status: {
            type: "bound",
            address: "127.0.0.1",
            port: 51_414,
            coordinated_with_tcp: false,
          },
          port_mapping_status: { type: "disabled" },
          advertised_peer_endpoint: {
            type: "outbound_only",
            generation: "1",
            reason: "listener_bind_failed",
          },
        },
      },
    });
    renderApplication(application);
    await user.click(screen.getByRole("button", { name: "Settings" }));
    const dialog = screen.getByRole("dialog", { name: "Settings" });
    await user.click(
      within(dialog).getByRole("tab", { name: "Connection & seeding" }),
    );

    expect(within(dialog).getByText(/port already in use/i)).toHaveTextContent(
      "port 51413 is already in use",
    );
    expect(within(dialog).getByText(/Transport: degraded/i)).toBeVisible();
    expect(within(dialog).queryByText(/restart/i)).not.toBeInTheDocument();
    expect(
      within(dialog).getByRole("radio", { name: /^Automatic port/ }),
    ).toBeChecked();
    expect(
      within(dialog).getByRole("button", { name: "Save settings" }),
    ).toBeDisabled();
  });

  it("shows configured intent and an uncertain effective mapping without restart copy", async () => {
    const user = userEvent.setup();
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: {
        ...liveSnapshot({ roots: [], defaultRoot: null, showAddOptions: true }),
        clientSettings: {
          ...clientSettingsRuntimeFixture(),
          configured: {
            ...clientSettingsRuntimeFixture().configured,
            listener: { type: "automatic_local_network" },
            port_mapping: "disabled",
          },
          effective_listener: {
            listener: { type: "automatic_local_network" },
            preferred_listen_port: 6_881,
          },
          effective_port_mapping: "upnp",
          port_mapping_application: {
            type: "degraded",
            reason: "port_mapping_cleanup_failed",
            detail: "delete verification failed; the prior lease may remain",
          },
          listener_status: {
            type: "listening",
            address: "192.168.50.12",
            port: 41_234,
          },
          session_udp_status: {
            type: "bound",
            address: "192.168.50.12",
            port: 41_234,
            coordinated_with_tcp: true,
          },
          port_mapping_status: {
            type: "cleanup_failed",
            external_address: "203.0.113.10",
            external_port: 48_001,
            remaining_lease_seconds: 42,
            detail: "delete verification failed",
          },
          advertised_peer_endpoint: {
            type: "local",
            generation: "9",
            address: "192.168.50.12",
            port: 41_234,
            scope: "local_network",
            incoming_observed: false,
          },
        },
      },
    });
    renderApplication(application);
    await user.click(screen.getByRole("button", { name: "Settings" }));
    const dialog = screen.getByRole("dialog", { name: "Settings" });
    await user.click(
      within(dialog).getByRole("tab", { name: "Connection & seeding" }),
    );

    expect(
      within(dialog).getByRole("checkbox", {
        name: /Map incoming TCP with UPnP/,
      }),
    ).not.toBeChecked();
    expect(
      within(dialog).getByText(/Effective gateway mapping policy: UPnP/i),
    ).toBeVisible();
    expect(
      within(dialog).getByText(/may remain for 42 seconds/i),
    ).toBeVisible();
    expect(within(dialog).getByText(/Port mapping: degraded/i)).toBeVisible();
    expect(within(dialog).queryByText(/restart/i)).not.toBeInTheDocument();
  });

  it("copies selected torrents' canonical magnets with truthful feedback", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn<(value: string) => Promise<void>>();
    writeText.mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    const snapshot = {
      ...buildScenarioSnapshot("healthy-download", 42_000, false, 1),
      demo: null,
    };
    const current = snapshot.torrents[snapshot.torrentOrder[0]!]!;
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot,
    });
    renderApplication(application);

    const more = screen.getByRole("button", { name: "More" });
    await user.click(more);
    const copy = screen.getByRole("menuitem", { name: "Copy magnet link" });
    expect(copy).not.toHaveAttribute("aria-disabled");
    await user.click(copy);

    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        `magnet:?xt=urn:btih:${current.infoHash}`,
      ),
    );
    expect(
      screen.getByText("Magnet link copied", { exact: true }),
    ).toBeVisible();
    expect(
      screen.queryByRole("menu", { name: "More" }),
    ).not.toBeInTheDocument();
    await waitFor(() => expect(more).toHaveFocus());

    writeText.mockRejectedValueOnce(new Error("permission denied"));
    await user.click(more);
    await user.click(
      screen.getByRole("menuitem", { name: "Copy magnet link" }),
    );
    expect(
      await screen.findByText(
        "Could not copy magnet links: permission denied",
        { exact: true },
      ),
    ).toBeVisible();
    await waitFor(() => expect(more).toHaveFocus());

    await user.click(
      screen.getByRole("checkbox", { name: "Select Sintel 4K open movie" }),
    );
    await user.click(more);
    const copyMultiple = screen.getByRole("menuitem", {
      name: "Copy magnet links",
    });
    expect(copyMultiple).not.toHaveAttribute("aria-disabled");
    await user.click(copyMultiple);
    const selectedMagnets = snapshot.torrentOrder
      .map((id) => snapshot.torrents[id]!)
      .filter(
        (torrent) =>
          torrent.id === current.id || torrent.name === "Sintel 4K open movie",
      )
      .map((torrent) => `magnet:?xt=urn:btih:${torrent.infoHash}`)
      .join("\n");
    await waitFor(() =>
      expect(writeText).toHaveBeenLastCalledWith(selectedMagnets),
    );
    expect(
      screen.getByText("2 magnet links copied", { exact: true }),
    ).toBeVisible();

    fireEvent.click(screen.getByRole("grid", { name: "Transfer queue" }));
    await user.click(more);
    expect(
      screen.getByRole("menuitem", { name: "Copy magnet links" }),
    ).toHaveAttribute("aria-disabled", "true");
  });

  it("dispatches an exact test magnet through the keyboard submenu", async () => {
    const user = userEvent.setup();
    const application = new RecordingLiveApplication({
      type: "snapshot",
      snapshot: liveSnapshot({
        roots: [downloadRoot("root_a", "Downloads")],
        defaultRoot: "root_a",
        showAddOptions: false,
      }),
    });
    renderApplication(application);
    const draft = screen.getByRole("textbox", {
      name: "Magnet link or torrent URL",
    });
    await user.type(draft, "unfinished draft");

    const more = screen.getByRole("button", { name: "More" });
    more.focus();
    await user.keyboard("{ArrowDown}");
    const addTestTorrent = screen.getByRole("menuitem", {
      name: "Add test torrent",
    });
    expect(
      screen.getByRole("menuitem", { name: "Copy magnet links" }),
    ).toHaveAttribute("aria-disabled", "true");
    expect(more).toHaveAttribute("aria-expanded", "true");
    expect(addTestTorrent).toHaveAttribute("data-focused");

    await user.keyboard("{ArrowRight}");
    const submenu = screen.getByRole("menu", { name: "Add test torrent" });
    const bunny = within(submenu).getByRole("menuitem", {
      name: "Big Buck Bunny",
    });
    expect(bunny).toHaveAttribute("data-focused");
    await user.keyboard("{End}");
    const wired = within(submenu).getByRole("menuitem", { name: "WIRED CD" });
    expect(wired).toHaveAttribute("data-focused");
    await user.keyboard("{Enter}");

    const wiredSource = WEBTORRENT_TEST_TORRENTS.at(-1)!;
    await waitFor(() => {
      expect(application.commands).toEqual([
        {
          type: "add_magnet",
          magnet: wiredSource.magnet,
          storageRoot: "root_a",
          startContent: true,
        },
      ]);
    });
    expect(draft).toHaveValue("unfinished draft");
    expect(screen.getByText("Torrent added", { exact: true })).toBeVisible();
    expect(
      screen.queryByRole("menu", { name: "More" }),
    ).not.toBeInTheDocument();
    await waitFor(() => expect(more).toHaveFocus());

    await user.click(more);
    await user.click(
      screen.getByRole("menuitem", { name: "Add test torrent" }),
    );
    const clickedSubmenu = screen.getByRole("menu", {
      name: "Add test torrent",
    });
    expect(clickedSubmenu).toBeVisible();
    await waitFor(() =>
      expect(
        screen.getByRole("menuitem", { name: "Big Buck Bunny" }),
      ).toHaveFocus(),
    );
    await user.keyboard("{Escape}");
    expect(
      screen.queryByRole("menu", { name: "Add test torrent" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("menuitem", { name: "Add test torrent" }),
    ).toHaveAttribute("data-focused");
    await user.keyboard("{Escape}");
    expect(
      screen.queryByRole("menu", { name: "More" }),
    ).not.toBeInTheDocument();
    expect(more).toHaveFocus();

    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menu", { name: "More" })).toBeVisible();
    await user.tab();
    expect(
      screen.queryByRole("menu", { name: "More" }),
    ).not.toBeInTheDocument();
  });
});

function renderScenario(
  scenarioId: ConstructorParameters<typeof DemoApplication>[0]["scenarioId"],
  elapsedMs: number,
  appearanceStorage?: AppearanceStorage | null,
) {
  return renderApplication(
    new DemoApplication({ scenarioId, elapsedMs, running: false }),
    appearanceStorage,
  );
}

function renderApplication(
  application: InspectionApplication,
  appearanceStorage?: AppearanceStorage | null,
) {
  const controller = new InspectionController(application, appearanceStorage);
  controllers.push(controller);
  controller.start();
  return render(
    <InspectionProvider controller={controller}>
      <App />
    </InspectionProvider>,
  );
}

function torrentFileInput(): HTMLInputElement {
  const input = document.querySelector<HTMLInputElement>(
    'input[type="file"][accept=".torrent,application/x-bittorrent"]',
  );
  if (input === null) throw new Error("torrent file input is missing");
  return input;
}

class RecordingLiveApplication implements InspectionApplication {
  readonly kind = "live" as const;
  readonly scenarios = [];
  readonly commands: InspectionCommand[] = [];
  readonly views: DesiredInspectionViews[] = [];
  rejectNextClientSettings = false;
  rejectNextTorrentId: string | undefined;
  private listener: ((update: InspectionUpdate) => void) | null = null;
  private storage: DownloadStorageSettings;
  private clientSettings: ClientSettingsRuntimeView;

  constructor(
    private readonly initialSnapshot?: InspectionUpdate & {
      readonly type: "snapshot";
    },
    private readonly rejectTorrentId?: string,
  ) {
    this.storage = initialSnapshot?.snapshot.storage ?? {
      roots: [],
      defaultRoot: null,
      showAddOptions: true,
    };
    this.clientSettings =
      initialSnapshot?.snapshot.clientSettings ??
      clientSettingsRuntimeFixture();
  }

  subscribe(listener: (update: InspectionUpdate) => void): () => void {
    this.listener = listener;
    if (this.initialSnapshot !== undefined) listener(this.initialSnapshot);
    return () => {
      this.listener = null;
    };
  }

  async setViews(views: DesiredInspectionViews): Promise<void> {
    this.views.push(views);
  }

  emitClientSettings(settings: ClientSettingsRuntimeView): void {
    this.clientSettings = settings;
    this.listener?.({
      type: "patch",
      revision: 2,
      clientSettings: settings,
    });
  }

  async dispatch(command: InspectionCommand): Promise<CommandResult> {
    this.commands.push(command);
    if (
      "torrentId" in command &&
      command.torrentId === this.rejectNextTorrentId
    ) {
      this.rejectNextTorrentId = undefined;
      throw new Error("rejected for test");
    }
    if ("torrentId" in command && command.torrentId === this.rejectTorrentId) {
      throw new Error("rejected for test");
    }
    if (command.type === "choose_download_root") {
      const root = downloadRoot(
        command.repairRoot ?? `root_${this.storage.roots.length + 1}`,
        command.repairRoot === undefined
          ? "Selected Downloads"
          : "Repaired Downloads",
      );
      this.storage = {
        ...this.storage,
        roots: [
          ...this.storage.roots.filter((candidate) => candidate.id !== root.id),
          root,
        ],
        defaultRoot: this.storage.defaultRoot ?? root.id,
      };
      this.emitStorage();
      return {
        accepted: true,
        message: "Download folder ready",
        storageRoot: root,
      };
    }
    if (command.type === "set_default_download_root") {
      this.storage = { ...this.storage, defaultRoot: command.rootId };
      this.emitStorage();
      return { accepted: true, message: "Default changed" };
    }
    if (command.type === "set_show_add_options") {
      this.storage = { ...this.storage, showAddOptions: command.show };
      this.emitStorage();
      return { accepted: true, message: "Preference changed" };
    }
    if (command.type === "remove_download_root") {
      this.storage = {
        ...this.storage,
        roots: this.storage.roots.filter((root) => root.id !== command.rootId),
        defaultRoot:
          this.storage.defaultRoot === command.rootId
            ? null
            : this.storage.defaultRoot,
      };
      this.emitStorage();
      return { accepted: true, message: "Folder removed" };
    }
    if (command.type === "set_client_settings") {
      if (this.rejectNextClientSettings) {
        this.rejectNextClientSettings = false;
        return { accepted: false, message: "settings save rejected" };
      }
      this.clientSettings = {
        ...this.clientSettings,
        configured: command.settings,
        transport_application: { type: "applying" },
        port_mapping_application: { type: "applying" },
        peer_connections_application: { type: "applying" },
        upload_slots_application: { type: "applying" },
        tracker_https_authentication_application: { type: "applying" },
      };
      this.listener?.({
        type: "patch",
        revision: 2,
        clientSettings: this.clientSettings,
      });
      return { accepted: true, message: "Settings saved" };
    }
    return { accepted: true, message: "Torrent added" };
  }

  private emitStorage(): void {
    this.listener?.({
      type: "patch",
      revision: 2,
      storage: this.storage,
    });
  }

  async close(): Promise<void> {}
}

function downloadRoot(
  id: string,
  label: string,
  availability: "available" | "unavailable" = "available",
) {
  return {
    id,
    label,
    path: `/Users/test/${label}`,
    availability,
  } as const;
}

function liveSnapshot(storage: DownloadStorageSettings) {
  return {
    ...buildScenarioSnapshot("empty-library", 0, false, 1),
    demo: null,
    storage,
  };
}
