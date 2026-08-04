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
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  ActionMenuItem,
  ActionMenuPopover,
  ActionMenuSection,
  ActionMenuSeparator,
  ActionMenuTrigger,
  AnchoredDialog,
  AnchoredDialogTrigger,
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
    await user.tab();
    expect(
      screen.queryByRole("menu", { name: "Actions" }),
    ).not.toBeInTheDocument();
    await waitFor(() => expect(outside).toHaveFocus());

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

  it("supports context invocation without binding product rows", () => {
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

  it("supports labelled sections, separators, and disabled actions", async () => {
    const user = userEvent.setup();
    render(
      <ActionMenuTrigger>
        <OverlayButton>Structured actions</OverlayButton>
        <ActionMenuPopover>
          <ActionMenuSection label="File">
            <ActionMenuItem>Inspect</ActionMenuItem>
          </ActionMenuSection>
          <ActionMenuSeparator />
          <ActionMenuItem isDisabled>Unavailable</ActionMenuItem>
        </ActionMenuPopover>
      </ActionMenuTrigger>,
    );

    await user.click(
      screen.getByRole("button", { name: "Structured actions" }),
    );
    expect(screen.getByText("File", { exact: true })).toBeVisible();
    expect(screen.getByRole("separator")).toBeVisible();
    expect(
      screen.getByRole("menuitem", { name: "Unavailable" }),
    ).toHaveAttribute("aria-disabled", "true");
  });

  it("closes when its trigger disables or owner unmounts", async () => {
    const user = userEvent.setup();
    const menu = (disabled: boolean) => (
      <ActionMenuTrigger isDisabled={disabled}>
        <OverlayButton isDisabled={disabled}>Lifecycle</OverlayButton>
        <ActionMenuPopover>
          <ActionMenuItem>Inspect</ActionMenuItem>
        </ActionMenuPopover>
      </ActionMenuTrigger>
    );
    const view = render(menu(false));
    await user.click(screen.getByRole("button", { name: "Lifecycle" }));
    expect(screen.getByRole("menu", { name: "Lifecycle" })).toBeVisible();
    view.rerender(menu(true));
    expect(
      screen.queryByRole("menu", { name: "Lifecycle" }),
    ).not.toBeInTheDocument();

    view.rerender(menu(false));
    await user.click(screen.getByRole("button", { name: "Lifecycle" }));
    view.unmount();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("keeps dialog state while outside dismissal absorbs the opening tap", async () => {
    const user = userEvent.setup();
    const outsideAction = vi.fn();

    function Harness() {
      const [checked, setChecked] = useState(false);
      return (
        <>
          <AnchoredDialogTrigger>
            <OverlayButton>Columns</OverlayButton>
            <AnchoredDialog aria-label="Column settings">
              <label>
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={(event) => setChecked(event.currentTarget.checked)}
                />
                Optional
              </label>
            </AnchoredDialog>
          </AnchoredDialogTrigger>
          <button type="button" onClick={outsideAction}>
            Outside dialog
          </button>
        </>
      );
    }

    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Columns" });
    const outside = screen.getByRole("button", { name: "Outside dialog" });
    await user.click(trigger);
    await user.click(screen.getByRole("checkbox", { name: "Optional" }));
    await user.click(screen.getByTestId("underlay"));
    expect(outsideAction).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("dialog", { name: "Column settings" }),
    ).not.toBeInTheDocument();

    await user.click(outside);
    expect(outsideAction).toHaveBeenCalledOnce();

    await user.click(trigger);
    expect(screen.getByRole("checkbox", { name: "Optional" })).toBeChecked();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(trigger).toHaveFocus());
  });
});
