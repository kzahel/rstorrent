// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type {
  DesktopUpdater,
  DesktopUpdaterSnapshot,
} from "../updater/types";
import { AboutUpdatesSettingsSection } from "./AboutUpdatesSettingsSection";

describe("About and updates settings", () => {
  it("shows build identity and routes manual check and install", async () => {
    const user = userEvent.setup();
    const updater = fakeUpdater();
    render(
      <AboutUpdatesSettingsSection
        updater={updater}
        snapshot={snapshot({
          phase: "available",
          version: "0.1.1",
          notes: "Updater is ready.",
          reason: "startup",
        })}
      />,
    );

    expect(screen.getByText("0.1.0")).toBeVisible();
    expect(screen.getByText("abcdef123456")).toBeVisible();
    expect(screen.getByText("aarch64-apple-darwin")).toBeVisible();
    expect(screen.getByText("Updater is ready.")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Check for updates" }));
    await user.click(screen.getByRole("button", { name: "Install and restart" }));
    expect(updater.check).toHaveBeenCalledWith("manual");
    expect(updater.install).toHaveBeenCalledOnce();
  });

  it("shows a manual release path for a package-manager install", () => {
    render(
      <AboutUpdatesSettingsSection
        updater={fakeUpdater()}
        snapshot={snapshot({
          phase: "manual-install",
          packageLabel: "Linux DEB package",
        })}
      />,
    );
    expect(screen.getByText(/Linux DEB package stays/)).toBeVisible();
    expect(
      screen.getByRole("link", { name: "Open release downloads" }),
    ).toHaveAttribute("href", "https://github.com/kzahel/rstorrent/releases/latest");
  });

  it("shows the headless apply command without a browser install action", () => {
    const updater = fakeUpdater();
    const view = render(
      <AboutUpdatesSettingsSection
        updater={updater}
        snapshot={{
          info: {
            version: "0.1.0",
            buildId: "0.1.0",
            target: "linux-gnu",
            arch: "x86_64",
            bundleType: "headless",
            checkPrivacy: "anonymous",
          },
          state: {
            phase: "available",
            version: "0.1.1",
            reason: "startup",
            manualApply: {
              command: "$HOME/.local/bin/rstorrent-headless update --apply",
              releaseUrl:
                "https://github.com/kzahel/rstorrent/releases/tag/headless-v0.1.1",
            },
          },
        }}
      />,
    );
    expect(
      within(view.container).getByText(
        "$HOME/.local/bin/rstorrent-headless update --apply",
      ),
    ).toBeVisible();
    expect(
      within(view.container).getByText(/no installation identifier/i),
    ).toBeVisible();
    expect(
      within(view.container).queryByRole("button", {
        name: "Install and restart",
      }),
    ).not.toBeInTheDocument();
    expect(
      within(view.container).getByRole("link", { name: "Review signed release" }),
    ).toHaveAttribute(
      "href",
      "https://github.com/kzahel/rstorrent/releases/tag/headless-v0.1.1",
    );
  });
});

function snapshot(
  state: DesktopUpdaterSnapshot["state"],
): DesktopUpdaterSnapshot {
  return {
    info: {
      version: "0.1.0",
      buildId: "abcdef1234567890",
      target: "aarch64-apple-darwin",
      arch: "aarch64",
      bundleType: state.phase === "manual-install" ? "deb" : "app",
    },
    state,
  };
}

function fakeUpdater(): DesktopUpdater {
  return {
    getSnapshot: vi.fn(),
    subscribe: vi.fn(() => () => undefined),
    check: vi.fn(async () => undefined),
    install: vi.fn(async () => undefined),
    dismiss: vi.fn(),
    close: vi.fn(),
  };
}
