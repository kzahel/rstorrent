// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { createRef } from "react";

import {
  APPEARANCE_STORAGE_KEY,
  type AppearanceStorage,
} from "../appearance";
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
    callback(0);
    return 1;
  };
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
  if (typeof globalThis.localStorage?.clear === "function") {
    globalThis.localStorage.clear();
  }
  await Promise.all(controllers.splice(0).map((controller) => controller.close()));
});

describe("inspection application", () => {
  it("renders the responsive hierarchy and changes detail tabs", async () => {
    const user = userEvent.setup();
    renderScenario("healthy-download", 42_000);
    const header = screen.getByRole("banner");
    expect(within(header).queryByText("Inspection")).not.toBeInTheDocument();
    expect(within(header).queryByText(/peers/i)).not.toBeInTheDocument();
    const primary = screen.getByRole("navigation", { name: "Primary" });
    expect(within(primary).getByRole("button", { name: "Transfers" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByRole("navigation", { name: "Transfer filters" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Add demo" })).toBeVisible();
    expect(
      screen.queryByRole("textbox", { name: "Magnet link or torrent URL" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("grid", { name: "Transfer queue" })).toHaveAttribute("aria-rowcount", "4");
    await user.click(within(primary).getByRole("button", { name: "Workbench" }));
    expect(screen.getByRole("navigation", { name: "Workbench torrent filters" })).toBeVisible();
    expect(screen.getByRole("grid", { name: "Torrent library" })).toHaveAttribute("aria-rowcount", "4");
    expect(screen.getByRole("grid", { name: "Active peer connections" })).toBeVisible();
    const peersTab = screen.getByRole("tab", { name: "Peers" });
    const peerCount = peersTab.textContent;
    expect(peerCount).toMatch(/^Peers\d+$/);
    await user.click(screen.getByRole("button", { name: "Explain Flags" }));
    const flagLegend = screen.getByRole("dialog", {
      name: "Flags column help",
    });
    expect(within(flagLegend).getByText("Incoming")).toBeVisible();
    expect(within(flagLegend).getByText("Encrypted")).toBeVisible();
    expect(within(flagLegend).queryByText(/case-sensitive/)).not.toBeInTheDocument();
    expect(
      within(flagLegend).queryByText(/remote peer initiated/),
    ).not.toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(flagLegend).not.toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "General" }));
    expect(screen.getByText("Current transfer")).toBeVisible();
    expect(peersTab).toHaveTextContent(peerCount!);
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

    expect(
      screen.getByRole("button", { name: "Workbench" }),
    ).toHaveAttribute("aria-current", "page");
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
    expect(within(grid).getAllByText(/TRK|DHT|TRACKER/i).length).toBeGreaterThan(0);
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
    expect(rendered.container.querySelectorAll("article").length).toBeLessThan(60);
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

    await user.type(screen.getByRole("searchbox", { name: "Search diagnostics" }), "watermark");
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
    const first = renderScenario(
      "healthy-download",
      42_000,
      appearanceStorage,
    );
    const app = first.container.firstElementChild;
    expect(app).toHaveAttribute("data-interface-size", "standard");
    expect(document.documentElement).toHaveAttribute(
      "data-color-theme",
      "auto",
    );

    for (const name of ["Start", "Pause", "Archive", "Remove"]) {
      expect(
        screen.getByRole("button", { name }).querySelector("svg"),
      ).not.toBeNull();
    }

    const settings = screen.getByRole("button", {
      name: "Settings",
    });
    await user.click(settings);
    const dialog = screen.getByRole("dialog", { name: "Settings" });
    const close = within(dialog).getByRole("button", {
      name: "Close settings",
    });
    expect(close).toHaveFocus();
    expect(
      within(dialog).getByRole("radio", { name: /Auto/ }),
    ).toBeChecked();

    await user.tab({ shift: true });
    expect(
      within(dialog).getByRole("radio", { name: /Spacious/ }),
    ).toHaveFocus();
    await user.click(within(dialog).getByRole("radio", { name: /Dark/ }));
    expect(document.documentElement).toHaveAttribute(
      "data-color-theme",
      "dark",
    );
    await user.click(within(dialog).getByRole("radio", { name: /Spacious/ }));
    expect(app).toHaveAttribute("data-interface-size", "spacious");
    expect(JSON.parse(storedAppearance ?? "null")).toEqual({
      version: 2,
      interfaceSize: "spacious",
      colorTheme: "dark",
    });

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Settings" })).not.toBeInTheDocument();
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
    await user.selectOptions(screen.getByLabelText("Demo scenario"), "disk-error");
    expect(screen.getAllByText(/storage failure/i)[0]).toBeVisible();
  });

  it("keeps rendered rows and cards bounded for large logical collections", async () => {
    const user = userEvent.setup();
    renderScenario("large-swarm", 0);
    const transferGrid = screen.getByRole("grid", { name: "Transfer queue" });
    expect(transferGrid).toHaveAttribute("aria-rowcount", "2001");
    expect(within(transferGrid).getAllByRole("row").length).toBeLessThanOrEqual(100);

    await user.click(screen.getByRole("button", { name: "Library" }));
    const library = screen.getByRole("list", { name: "Torrent-backed content" });
    expect(within(library).getAllByRole("listitem").length).toBeLessThan(100);

    await user.click(screen.getByRole("button", { name: "Workbench" }));
    const torrentGrid = screen.getByRole("grid", { name: "Torrent library" });
    const peerGrid = screen.getByRole("grid", { name: "Active peer connections" });
    expect(torrentGrid).toHaveAttribute("aria-rowcount", "2001");
    expect(peerGrid).toHaveAttribute("aria-rowcount", "10001");
    expect(within(torrentGrid).getAllByRole("row").length).toBeLessThanOrEqual(100);
    expect(within(peerGrid).getAllByRole("row").length).toBeLessThanOrEqual(100);
  });

  it("materializes a full file catalog only on the Files tab", async () => {
    const user = userEvent.setup();
    renderScenario("file-progress", 24_000);
    await user.click(screen.getByRole("button", { name: "Workbench" }));
    expect(screen.queryByRole("grid", { name: "Torrent files" })).not.toBeInTheDocument();
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
    await user.click(
      screen.getByRole("button", { name: "More file actions" }),
    );
    const fileActions = screen.getByRole("menu", { name: "File actions" });
    expect(within(fileActions).getByRole("menuitem", { name: "Normal" })).toBeDisabled();
    expect(
      within(fileActions).getByRole("menuitem", { name: "Skip" }),
    ).toBeDisabled();
    expect(fileActions).toHaveTextContent(
      "File priority changes are unavailable in demo scenarios.",
    );
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

    await user.click(screen.getAllByRole("button", { name: "Columns" }).at(-1)!);
    await user.click(screen.getByRole("checkbox", { name: "Storage Path" }));
    expect(within(files).getByRole("columnheader", { name: /Storage Path/ })).toBeVisible();
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
    expect(skip).toBeEnabled();
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
    expect(normal).toBeEnabled();
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

  it("materializes the global disk pipeline only on the Disk tab", async () => {
    const user = userEvent.setup();
    renderScenario("slow-disk-pressure", 20_000);
    await user.click(screen.getByRole("button", { name: "Workbench" }));
    expect(
      screen.queryByRole("grid", { name: "Active storage pieces" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Disk" }));
    expect(screen.getByText("Receive → write → verify → checkpoint")).toBeVisible();
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
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.click(deleteData);
    expect(within(dialog).getByRole("alert")).toHaveTextContent(/cannot be undone/i);
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
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByText("Torrent removed", { exact: true })).toBeVisible();
  });

  it("keeps a failed removal dialog actionable", async () => {
    const user = userEvent.setup();
    const returnFocus = createRef<HTMLButtonElement>();
    render(
      <>
        <button ref={returnFocus}>Remove trigger</button>
        <RemoveTorrentDialog
          torrentName="Test transfer"
          deleteDataSupported={true}
          returnFocus={returnFocus}
          onCancel={() => {}}
          onConfirm={async () => {
            throw new Error("Provider permission was revoked");
          }}
        />
      </>,
    );
    const dialog = screen.getByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Remove" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      "Provider permission was revoked",
    );
    expect(within(dialog).getByRole("button", { name: "Remove" })).toBeEnabled();
    expect(within(dialog).getByRole("button", { name: "Remove" })).toHaveFocus();
  });

  it("renders truthful empty states without fabricating media details", async () => {
    const user = userEvent.setup();
    renderScenario("empty-library", 0);
    expect(screen.getByText(/No transfers yet/i)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Library" }));
    expect(screen.getByText("No content sources yet")).toBeVisible();
    expect(screen.queryByRole("button", { name: /^Play / })).not.toBeInTheDocument();
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
    expect(screen.getByRole("button", { name: "Remove" })).toBeDisabled();
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
    expect(screen.queryByText("3 selected for actions")).not.toBeInTheDocument();
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
    expect(screen.queryByText("3 selected for actions")).not.toBeInTheDocument();
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
    await user.click(screen.getByRole("button", { name: "Archive" }));

    await waitFor(() =>
      expect(application.commands).toEqual([
        { type: "archive", torrentId: DEMO_PRIMARY_TORRENT_ID },
        { type: "archive", torrentId: sintel.id },
      ]),
    );
    expect(
      screen.getByText(/Archived 1 of 2; Sintel 4K open movie: rejected for test/),
    ).toBeVisible();
  });

  it("selects truthful Library cards and hands their source to Workbench", async () => {
    const user = userEvent.setup();
    renderScenario("healthy-download", 42_000);
    await user.click(screen.getByRole("button", { name: "Library" }));
    expect(screen.getByText(/media details are not connected yet/i)).toBeVisible();
    expect(screen.queryByRole("button", { name: /^Play / })).not.toBeInTheDocument();

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
    expect(screen.getAllByText("Sintel 4K open movie").length).toBeGreaterThan(0);
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

    let dialog = screen.getByRole("dialog", { name: "Choose download options" });
    expect(within(dialog).getByText(/download folder is required/i)).toBeVisible();
    expect(within(dialog).getByRole("button", { name: "Add torrent" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toHaveFocus();
    expect(draft).toHaveValue(magnet);

    await user.click(within(dialog).getByRole("button", { name: "Choose folder…" }));
    await waitFor(() =>
      expect(
        within(dialog).getByRole("radio", { name: /Selected Downloads/ }),
      ).toBeChecked(),
    );
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog", { name: "Choose download options" })).not.toBeInTheDocument();
    expect(draft).toHaveValue(magnet);
    expect(
      application.commands.filter((command) => command.type === "add_magnet"),
    ).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Add" }));
    dialog = screen.getByRole("dialog", { name: "Choose download options" });
    await user.click(
      within(dialog).getByRole("checkbox", { name: /Don’t show these options again/ }),
    );
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: /Start downloading files when metadata is available/,
      }),
    );
    await user.click(within(dialog).getByRole("button", { name: "Add torrent" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Choose download options" })).not.toBeInTheDocument(),
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
    const dialog = screen.getByRole("dialog", { name: "Choose download options" });
    expect(within(dialog).getByRole("radio", { name: /Default Downloads/ })).toBeChecked();
    await user.click(within(dialog).getByRole("radio", { name: /External Drive/ }));
    await user.click(within(dialog).getByRole("button", { name: "Add torrent" }));
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
    await user.click(within(dialog).getByRole("button", { name: "Make default" }));
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

  it("copies one selected torrent's canonical magnet with truthful feedback", async () => {
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
    expect(copy).toBeEnabled();
    await user.click(copy);

    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        `magnet:?xt=urn:btih:${current.infoHash}`,
      ),
    );
    expect(screen.getByText("Magnet link copied", { exact: true })).toBeVisible();
    expect(screen.queryByRole("menu", { name: "More actions" })).not.toBeInTheDocument();
    await waitFor(() => expect(more).toHaveFocus());

    writeText.mockRejectedValueOnce(new Error("permission denied"));
    await user.click(more);
    await user.click(screen.getByRole("menuitem", { name: "Copy magnet link" }));
    expect(
      await screen.findByText(
        "Could not copy magnet link: permission denied",
        { exact: true },
      ),
    ).toBeVisible();
    await waitFor(() => expect(more).toHaveFocus());

    await user.click(
      screen.getByRole("checkbox", { name: "Select Sintel 4K open movie" }),
    );
    await user.click(more);
    expect(
      screen.getByRole("menuitem", { name: "Copy magnet link" }),
    ).toBeDisabled();
    await user.keyboard("{Escape}");

    fireEvent.click(screen.getByRole("grid", { name: "Transfer queue" }));
    await user.click(more);
    expect(
      screen.getByRole("menuitem", { name: "Copy magnet link" }),
    ).toBeDisabled();
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
      screen.getByRole("menuitem", { name: "Copy magnet link" }),
    ).toBeDisabled();
    expect(more).toHaveAttribute("aria-expanded", "true");
    expect(addTestTorrent).toHaveFocus();

    await user.keyboard("{ArrowRight}");
    const submenu = screen.getByRole("menu", { name: "Add test torrent" });
    const bunny = within(submenu).getByRole("menuitem", {
      name: "Big Buck Bunny",
    });
    expect(bunny).toHaveFocus();
    await user.keyboard("{End}");
    const wired = within(submenu).getByRole("menuitem", { name: "WIRED CD" });
    expect(wired).toHaveFocus();
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
    expect(screen.queryByRole("menu", { name: "More actions" })).not.toBeInTheDocument();
    await waitFor(() => expect(more).toHaveFocus());

    await user.click(more);
    await user.click(
      screen.getByRole("menuitem", { name: "Add test torrent" }),
    );
    expect(screen.getByRole("menu", { name: "Add test torrent" })).toBeVisible();
    await user.keyboard("{Escape}");
    expect(
      screen.queryByRole("menu", { name: "Add test torrent" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("menuitem", { name: "Add test torrent" }),
    ).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu", { name: "More actions" })).not.toBeInTheDocument();
    expect(more).toHaveFocus();

    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menu", { name: "More actions" })).toBeVisible();
    await user.tab();
    expect(
      screen.queryByRole("menu", { name: "More actions" }),
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

class RecordingLiveApplication implements InspectionApplication {
  readonly kind = "live" as const;
  readonly scenarios = [];
  readonly commands: InspectionCommand[] = [];
  readonly views: DesiredInspectionViews[] = [];
  private listener: ((update: InspectionUpdate) => void) | null = null;
  private storage: DownloadStorageSettings;

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

  async dispatch(command: InspectionCommand): Promise<CommandResult> {
    this.commands.push(command);
    if (
      "torrentId" in command &&
      command.torrentId === this.rejectTorrentId
    ) {
      throw new Error("rejected for test");
    }
    if (command.type === "choose_download_root") {
      const root = downloadRoot(
        command.repairRoot ?? `root_${this.storage.roots.length + 1}`,
        command.repairRoot === undefined ? "Selected Downloads" : "Repaired Downloads",
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
