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
import type {
  CommandResult,
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
  if (typeof globalThis.localStorage?.clear === "function") {
    globalThis.localStorage.clear();
  }
  await Promise.all(controllers.splice(0).map((controller) => controller.close()));
});

describe("inspection application", () => {
  it("renders the responsive hierarchy and changes detail tabs", async () => {
    const user = userEvent.setup();
    renderScenario("healthy-download", 42_000);
    expect(screen.getByRole("navigation", { name: "Torrent library" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Add demo" })).toBeVisible();
    expect(
      screen.queryByRole("textbox", { name: "Magnet link or torrent URL" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("grid", { name: "Torrent library" })).toHaveAttribute("aria-rowcount", "4");
    expect(screen.getByRole("grid", { name: "Active peer connections" })).toBeVisible();
    const peersTab = screen.getByRole("tab", { name: "Peers" });
    const peerCount = peersTab.textContent;
    expect(peerCount).toMatch(/^Peers\d+$/);

    await user.click(screen.getByRole("tab", { name: "General" }));
    expect(screen.getByText("Selected transfer")).toBeVisible();
    expect(peersTab).toHaveTextContent(peerCount!);
    await user.click(screen.getByRole("tab", { name: "Logs" }));
    expect(screen.getByRole("grid", { name: "Diagnostic log" })).toBeVisible();
  });

  it("opens Settings, changes interface size live, and restores it", async () => {
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

    await user.tab({ shift: true });
    expect(
      within(dialog).getByRole("radio", { name: /Spacious/ }),
    ).toHaveFocus();
    await user.click(within(dialog).getByRole("radio", { name: /Spacious/ }));
    expect(app).toHaveAttribute("data-interface-size", "spacious");
    expect(JSON.parse(storedAppearance ?? "null")).toEqual({
      version: 1,
      interfaceSize: "spacious",
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
  });

  it("switches named scenarios and advances the frozen clock", async () => {
    const user = userEvent.setup();
    renderScenario("tracker-recovery", 0);
    await user.click(screen.getByRole("tab", { name: "General" }));
    expect(screen.getByText(/retry scheduled in 22 seconds/i)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "+10s" }));
    await user.click(screen.getByRole("button", { name: "+10s" }));
    await user.click(screen.getByRole("button", { name: "+10s" }));
    expect(screen.getByText(/recovered tracker cohort/i)).toBeVisible();
    await user.selectOptions(screen.getByLabelText("Demo scenario"), "disk-error");
    expect(screen.getAllByText(/storage failure/i)[0]).toBeVisible();
  });

  it("keeps rendered rows bounded for large logical collections", () => {
    renderScenario("large-swarm", 0);
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
    expect(screen.queryByRole("grid", { name: "Torrent files" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Files" }));
    const files = screen.getByRole("grid", { name: "Torrent files" });
    expect(files).toHaveAttribute("aria-rowcount", "4096");
    expect(within(files).getAllByRole("row").length).toBeLessThanOrEqual(100);
    expect(screen.getByText("1 padding hidden")).toBeVisible();
    await user.click(screen.getAllByRole("button", { name: "Columns" }).at(-1)!);
    await user.click(screen.getByRole("checkbox", { name: "Storage Path" }));
    expect(within(files).getByRole("columnheader", { name: /Storage Path/ })).toBeVisible();
  });

  it("materializes the global disk pipeline only on the Disk tab", async () => {
    const user = userEvent.setup();
    renderScenario("slow-disk-pressure", 20_000);
    expect(
      screen.queryByRole("grid", { name: "Active storage pieces" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Disk" }));
    expect(screen.getByText("Receive → write → verify")).toBeVisible();
    expect(screen.getByLabelText("Disk pressure Backpressured")).toBeVisible();
    const pieces = screen.getByRole("grid", { name: "Active storage pieces" });
    expect(pieces).toHaveAttribute("aria-rowcount", "65");
    expect(within(pieces).getAllByRole("row").length).toBeLessThanOrEqual(100);
    expect(screen.getByText("intake paused now")).toBeVisible();
  });

  it("renders a bounded accessible canvas for a 250,000-piece torrent", async () => {
    const user = userEvent.setup();
    renderScenario("large-swarm", 0);

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

  it("renders truthful empty state without fabricating details", () => {
    renderScenario("empty-library", 0);
    expect(screen.getByText(/No torrents yet/i)).toBeVisible();
    expect(screen.getByText(/Select a torrent to inspect it/i)).toBeVisible();
  });

  it("dispatches an exact test magnet through the keyboard submenu", async () => {
    const user = userEvent.setup();
    const application = new RecordingLiveApplication();
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
        { type: "add_magnet", magnet: wiredSource.magnet },
      ]);
    });
    expect(draft).toHaveValue("unfinished draft");
    expect(screen.getByText("WIRED CD added", { exact: true })).toBeVisible();
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

  subscribe(_listener: (update: InspectionUpdate) => void): () => void {
    return () => {};
  }

  async setViews(_views: DesiredInspectionViews): Promise<void> {}

  async dispatch(command: InspectionCommand): Promise<CommandResult> {
    this.commands.push(command);
    return { accepted: true, message: "Torrent added" };
  }

  async close(): Promise<void> {}
}
