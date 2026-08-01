// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, it } from "vitest";

import { InspectionProvider } from "../context";
import { InspectionController } from "../controller";
import { DemoApplication } from "../demo/DemoApplication";
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
    expect(screen.getByRole("grid", { name: "Torrent library" })).toHaveAttribute("aria-rowcount", "4");
    expect(screen.getByRole("grid", { name: "Connected and candidate peers" })).toBeVisible();

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
    const peerGrid = screen.getByRole("grid", { name: "Connected and candidate peers" });
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
});

function renderScenario(
  scenarioId: ConstructorParameters<typeof DemoApplication>[0]["scenarioId"],
  elapsedMs: number,
) {
  const controller = new InspectionController(
    new DemoApplication({ scenarioId, elapsedMs, running: false }),
  );
  controllers.push(controller);
  controller.start();
  return render(
    <InspectionProvider controller={controller}>
      <App />
    </InspectionProvider>,
  );
}
