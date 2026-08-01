// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, it } from "vitest";

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
});

const controllers: InspectionController[] = [];

afterEach(async () => {
  cleanup();
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

    await user.click(screen.getByRole("tab", { name: "General" }));
    expect(screen.getByText("Selected transfer")).toBeVisible();
    await user.click(screen.getByRole("tab", { name: "Logs" }));
    expect(screen.getByRole("grid", { name: "Diagnostic log" })).toBeVisible();
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
) {
  return renderApplication(
    new DemoApplication({ scenarioId, elapsedMs, running: false }),
  );
}

function renderApplication(application: InspectionApplication) {
  const controller = new InspectionController(application);
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
