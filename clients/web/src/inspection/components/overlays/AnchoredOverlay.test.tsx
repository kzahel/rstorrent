// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  ActionMenuItem,
  ActionMenuPopover,
  ActionMenuTrigger,
  OverlayButton,
} from "./AnchoredOverlay";

afterEach(cleanup);

describe("anchored overlay primitives", () => {
  it("portals menus, navigates items, dismisses outside, and restores focus", async () => {
    const user = userEvent.setup();
    const firstAction = vi.fn();
    render(
      <div data-testid="owner">
        <ActionMenuTrigger>
          <OverlayButton>Actions</OverlayButton>
          <ActionMenuPopover>
            <ActionMenuItem onAction={firstAction}>First</ActionMenuItem>
            <ActionMenuItem>Second</ActionMenuItem>
          </ActionMenuPopover>
        </ActionMenuTrigger>
        <button type="button">Outside</button>
      </div>,
    );

    const trigger = screen.getByRole("button", { name: "Actions" });
    const outside = screen.getByRole("button", { name: "Outside" });
    await user.click(trigger);
    const menu = screen.getByRole("menu", { name: "Actions" });
    expect(screen.getByTestId("owner")).not.toContainElement(menu);
    expect(menu).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(menu).not.toBeInTheDocument();
    await waitFor(() => expect(trigger).toHaveFocus());

    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: "First" })).toHaveAttribute(
      "data-focused",
    );
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: "Second" })).toHaveAttribute(
      "data-focused",
    );
    await user.keyboard("{Escape}");

    await user.click(trigger);
    await user.click(outside);
    expect(
      screen.queryByRole("menu", { name: "Actions" }),
    ).not.toBeInTheDocument();

    await user.click(trigger);
    await user.click(screen.getByRole("menuitem", { name: "First" }));
    expect(firstAction).toHaveBeenCalledOnce();
    expect(
      screen.queryByRole("menu", { name: "Actions" }),
    ).not.toBeInTheDocument();
  });

  it("supports context-menu positioning without binding product rows", () => {
    render(
      <ActionMenuTrigger trigger="contextMenu">
        <OverlayButton>Context target</OverlayButton>
        <ActionMenuPopover>
          <ActionMenuItem>Inspect</ActionMenuItem>
        </ActionMenuPopover>
      </ActionMenuTrigger>,
    );

    fireEvent.contextMenu(
      screen.getByRole("button", { name: "Context target" }),
      { clientX: 120, clientY: 80 },
    );
    expect(screen.getByRole("menu", { name: "Context target" })).toBeVisible();
  });
});
