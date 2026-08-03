// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import { VirtualTable, type VirtualColumn } from "./VirtualTable";

interface Row {
  readonly id: string;
  readonly value: string | null;
}

const COLUMNS: readonly VirtualColumn<Row>[] = [
  {
    id: "value",
    label: "Value",
    width: 140,
    sortKind: "decimal",
    sortValue: (row) => row.value,
    headerHelp: <p>Values are exact decimal strings.</p>,
    render: (row) => row.value ?? "—",
  },
  {
    id: "optional",
    label: "Optional",
    width: 100,
    defaultVisible: false,
    render: () => "extra",
  },
];

const storedValues = new Map<string, string>();
const originalLocalStorage = Object.getOwnPropertyDescriptor(
  globalThis,
  "localStorage",
);

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
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      clear: () => storedValues.clear(),
      getItem: (key: string) => storedValues.get(key) ?? null,
      setItem: (key: string, value: string) => storedValues.set(key, value),
    } satisfies Pick<Storage, "clear" | "getItem" | "setItem">,
  });
});

afterAll(() => {
  if (originalLocalStorage === undefined) delete (globalThis as { localStorage?: Storage }).localStorage;
  else Object.defineProperty(globalThis, "localStorage", originalLocalStorage);
});

afterEach(() => {
  cleanup();
  if (typeof globalThis.localStorage?.clear === "function") {
    globalThis.localStorage.clear();
  }
});

describe("VirtualTable", () => {
  it("sorts decimal u64 strings exactly and keeps null last in both directions", () => {
    const { container } = render(
      <VirtualTable
        tableId="sort-test"
        label="Exact decimal sorting"
        rows={[
          { id: "null", value: null },
          { id: "large", value: "9007199254740993" },
          { id: "ten", value: "10" },
        ]}
        getRowId={(row) => row.id}
        columns={COLUMNS}
        interfaceSize="standard"
        emptyMessage="empty"
      />,
    );
    const value = screen.getByRole("button", { name: "Value" });
    fireEvent.click(value);
    expect(rowIds(container)).toEqual(["ten", "large", "null"]);
    fireEvent.click(value);
    expect(rowIds(container)).toEqual(["large", "ten", "null"]);
  });

  it("opens column help without sorting and restores focus on Escape", () => {
    render(
      <VirtualTable
        tableId="help-test"
        label="Column help"
        rows={[{ id: "one", value: "1" }]}
        getRowId={(row) => row.id}
        columns={COLUMNS}
        interfaceSize="standard"
        emptyMessage="empty"
      />,
    );
    const help = screen.getByRole("button", { name: "Explain Value" });
    const header = screen.getByRole("columnheader", { name: "Value" });
    fireEvent.click(help);
    expect(header).not.toHaveAttribute("aria-sort");
    expect(help).toHaveAttribute("aria-expanded", "true");
    expect(
      screen.getByRole("dialog", { name: "Value column help" }),
    ).toHaveTextContent("exact decimal strings");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(
      screen.queryByRole("dialog", { name: "Value column help" }),
    ).not.toBeInTheDocument();
    expect(help).toHaveFocus();
  });

  it("hides optional columns by default and exposes persisted controls", () => {
    const first = render(
      <VirtualTable
        tableId="columns-test"
        label="Column configuration"
        rows={[{ id: "one", value: "1" }]}
        getRowId={(row) => row.id}
        columns={COLUMNS}
        interfaceSize="standard"
        emptyMessage="empty"
      />,
    );
    expect(screen.queryByRole("columnheader", { name: "Optional" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Columns" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Optional" }));
    expect(screen.getByRole("columnheader", { name: /Optional/ })).toBeVisible();
    first.unmount();

    render(
      <VirtualTable
        tableId="columns-test"
        label="Restored column configuration"
        rows={[{ id: "one", value: "1" }]}
        getRowId={(row) => row.id}
        columns={COLUMNS}
        interfaceSize="standard"
        emptyMessage="empty"
      />,
    );
    expect(screen.getByRole("columnheader", { name: /Optional/ })).toBeVisible();
  });

  it("reconciles stale hidden columns and clamps persisted widths", () => {
    storedValues.set(
      "rstorrent.table.stale-test",
      JSON.stringify({
        version: 1,
        hiddenColumns: ["value", "optional", "removed"],
        widths: { value: 50_000, removed: 12 },
        sort: { columnId: "removed", direction: "asc" },
        liveSort: true,
      }),
    );
    render(
      <VirtualTable
        tableId="stale-test"
        label="Reconciled column configuration"
        rows={[{ id: "one", value: "1" }]}
        getRowId={(row) => row.id}
        columns={COLUMNS}
        interfaceSize="standard"
        emptyMessage="empty"
      />,
    );
    expect(screen.getByRole("columnheader", { name: "Value" })).toBeVisible();
    expect(screen.getByRole("separator", { name: "Resize Value column" })).toHaveAttribute(
      "aria-valuenow",
      "720",
    );
  });

  it("updates absolute row geometry with the selected interface size", () => {
    const table = (interfaceSize: "compact" | "spacious") => (
      <VirtualTable
        tableId="size-test"
        label="Interface size geometry"
        rows={[
          { id: "one", value: "1" },
          { id: "two", value: "2" },
          { id: "three", value: "3" },
        ]}
        getRowId={(row) => row.id}
        columns={COLUMNS}
        interfaceSize={interfaceSize}
        emptyMessage="empty"
      />
    );
    const rendered = render(table("compact"));
    const root = () => rendered.container.firstElementChild as HTMLElement;
    const second = () =>
      rendered.container.querySelector<HTMLElement>('[data-row-id="two"]');
    expect(root().style.getPropertyValue("--ui-table-header-height")).toBe("34px");
    expect(root().style.getPropertyValue("--ui-table-row-height")).toBe("32px");
    expect(second()).toHaveStyle({ transform: "translateY(32px)" });

    rendered.rerender(table("spacious"));
    expect(root().style.getPropertyValue("--ui-table-header-height")).toBe("44px");
    expect(root().style.getPropertyValue("--ui-table-row-height")).toBe("42px");
    expect(second()).toHaveStyle({ transform: "translateY(42px)" });
  });

  it("keeps actionable checks visible while batch mode stays explicit", () => {
    function SelectableTable() {
      const rows = [
        { id: "one", value: "1" },
        { id: "two", value: "2" },
      ];
      const [active, setActive] = useState(false);
      const [currentId, setCurrentId] = useState<string | null>("one");
      const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(new Set());
      return (
        <VirtualTable
          tableId="selection-test"
          label="Selectable rows"
          rows={rows}
          getRowId={(row) => row.id}
          columns={COLUMNS}
          interfaceSize="standard"
          emptyMessage="empty"
          activeRowId={currentId}
          onActivate={(row) => setCurrentId(row.id)}
          onClearActive={() => setCurrentId(null)}
          batchSelection={{
            active,
            batchSelectedIds: selectedIds,
            getRowLabel: (row) => row.id,
            onEnterBatch: (row) => {
              setActive(true);
              setSelectedIds(new Set(row === undefined ? [] : [row.id]));
            },
            onExitBatch: () => {
              setActive(false);
              setSelectedIds(new Set());
            },
            onToggleBatch: (row) => {
              setActive(true);
              setSelectedIds((current) => {
                const next = new Set(current);
                if (next.has(row.id)) next.delete(row.id);
                else next.add(row.id);
                return next;
              });
            },
            onReplaceBatch: (rangeRows) => {
              setActive(true);
              setSelectedIds(new Set(rangeRows.map((row) => row.id)));
            },
            onSetAllBatch: (selectedRows, selected) => {
              setActive(true);
              setSelectedIds(
                selected
                  ? new Set(selectedRows.map((row) => row.id))
                  : new Set(),
              );
            },
          }}
        />
      );
    }

    render(<SelectableTable />);
    const grid = screen.getByRole("grid", { name: "Selectable rows" });
    expect(grid).toHaveAttribute("aria-multiselectable", "true");
    expect(screen.getByRole("checkbox", { name: "Select one" })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Select two" })).not.toBeChecked();
    expect(screen.getByRole("row", { name: /1/ })).toHaveAttribute(
      "aria-current",
      "true",
    );
    expect(screen.getByRole("row", { name: /1/ })).toHaveAttribute(
      "aria-selected",
      "false",
    );

    fireEvent.click(
      screen.getByRole("checkbox", { name: "Select two" }).parentElement!,
    );
    expect(screen.getByRole("checkbox", { name: "Deselect two" })).toBeChecked();
    const selectAll = screen.getByRole("checkbox", { name: "Select all rows" });
    expect(selectAll).toHaveProperty("indeterminate", true);
    fireEvent.click(selectAll);
    expect(screen.getByRole("checkbox", { name: "Deselect all rows" })).toBeChecked();

    const firstRow = screen.getByRole("row", { name: /Deselect one/ });
    firstRow.focus();
    fireEvent.keyDown(grid, {
      key: " ",
    });
    expect(screen.getByRole("checkbox", { name: "Select one" })).not.toBeChecked();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select all rows" }));
    expect(screen.getByText("2 selected for actions")).toBeVisible();

    fireEvent.click(
      screen.getByRole("button", {
        name: "Done selecting rows in Selectable rows",
      }),
    );
    expect(screen.getByRole("checkbox", { name: "Select one" })).not.toBeChecked();
    const currentRow = screen.getByRole("row", { name: /1/ });
    currentRow.focus();
    fireEvent.keyDown(grid, { key: " " });
    expect(screen.getByRole("checkbox", { name: "Deselect one" })).toBeChecked();
    fireEvent.keyDown(grid, { key: "Escape" });
    expect(screen.getByRole("checkbox", { name: "Select one" })).not.toBeChecked();

    const secondRow = screen.getByRole("row", { name: /2/ });
    fireEvent.click(secondRow, { ctrlKey: true });
    expect(screen.getByRole("checkbox", { name: "Deselect two" })).toBeChecked();
    const firstInSelection = screen.getByRole("row", { name: /Select one/ });
    fireEvent.focus(firstInSelection);
    fireEvent.keyDown(grid, { key: "Enter" });
    expect(screen.getByRole("checkbox", { name: "Select one" })).not.toBeChecked();
    expect(firstInSelection).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("checkbox", { name: "Deselect two" })).toBeChecked();
    fireEvent.click(grid);
    expect(screen.getByRole("checkbox", { name: "Select one" })).not.toBeChecked();
  });

  it("replaces forward and reverse Shift ranges in sorted row order", () => {
    function RangeTable() {
      const rows = ["one", "two", "three", "four", "five"].map(
        (id, index) => ({ id, value: String(index + 1) }),
      );
      const [active, setActive] = useState(false);
      const [activeRowId, setActiveRowId] = useState<string | null>("two");
      const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(
        new Set(),
      );
      return (
        <VirtualTable
          tableId="range-test"
          label="Range rows"
          rows={rows}
          getRowId={(row) => row.id}
          columns={COLUMNS}
          interfaceSize="standard"
          activeRowId={activeRowId}
          onActivate={(row) => setActiveRowId(row.id)}
          emptyMessage="empty"
          batchSelection={{
            active,
            batchSelectedIds: selectedIds,
            getRowLabel: (row) => row.id,
            onEnterBatch: (row) => {
              setActive(true);
              setSelectedIds(new Set(row === undefined ? [] : [row.id]));
            },
            onExitBatch: () => {
              setActive(false);
              setSelectedIds(new Set());
            },
            onToggleBatch: (row) => {
              setActive(true);
              setSelectedIds((current) => {
                const next = new Set(current);
                if (next.has(row.id)) next.delete(row.id);
                else next.add(row.id);
                return next;
              });
            },
            onReplaceBatch: (rangeRows) => {
              setActive(true);
              setSelectedIds(new Set(rangeRows.map((row) => row.id)));
            },
            onSetAllBatch: (rangeRows, selected) => {
              setActive(true);
              setSelectedIds(
                selected ? new Set(rangeRows.map((row) => row.id)) : new Set(),
              );
            },
          }}
        />
      );
    }

    render(<RangeTable />);
    fireEvent.click(screen.getByRole("row", { name: /4/ }), { shiftKey: true });
    expect(checkedRowNames()).toEqual(["two", "three", "four"]);
    expect(screen.getByRole("row", { name: /4/ })).toHaveAttribute(
      "aria-current",
      "true",
    );
    fireEvent.click(screen.getByRole("row", { name: /3/ }), { shiftKey: true });
    expect(checkedRowNames()).toEqual(["two", "three"]);

    fireEvent.click(screen.getByRole("row", { name: /5/ }));
    fireEvent.click(screen.getByRole("row", { name: /3/ }), { shiftKey: true });
    expect(checkedRowNames()).toEqual(["three", "four", "five"]);
    fireEvent.click(screen.getByRole("checkbox", { name: "Deselect four" }), {
      shiftKey: true,
    });
    expect(checkedRowNames()).toEqual(["four", "five"]);

    fireEvent.click(
      screen.getByRole("button", { name: "Done selecting rows in Range rows" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Value" }));
    fireEvent.click(screen.getByRole("button", { name: "Value" }));
    fireEvent.click(screen.getByRole("row", { name: /2/ }));
    fireEvent.click(screen.getByRole("row", { name: /4/ }), { shiftKey: true });
    expect(checkedRowNames()).toEqual(["four", "three", "two"]);
  });

  it("moves the active row with focus and extends keyboard batch ranges", () => {
    function KeyboardTable() {
      const rows = ["one", "two", "three", "four", "five"].map(
        (id, index) => ({ id, value: String(index + 1) }),
      );
      const [activeRowId, setActiveRowId] = useState<string | null>("two");
      const [batchActive, setBatchActive] = useState(false);
      const [batchSelectedIds, setBatchSelectedIds] = useState<
        ReadonlySet<string>
      >(new Set());
      return (
        <VirtualTable
          tableId="keyboard-range-test"
          label="Keyboard rows"
          rows={rows}
          getRowId={(row) => row.id}
          columns={COLUMNS}
          interfaceSize="standard"
          activeRowId={activeRowId}
          onActivate={(row) => setActiveRowId(row.id)}
          emptyMessage="empty"
          batchSelection={{
            active: batchActive,
            batchSelectedIds,
            getRowLabel: (row) => row.id,
            onEnterBatch: (row) => {
              setBatchActive(true);
              setBatchSelectedIds(new Set(row === undefined ? [] : [row.id]));
            },
            onExitBatch: () => {
              setBatchActive(false);
              setBatchSelectedIds(new Set());
            },
            onToggleBatch: (row) => {
              setBatchActive(true);
              setBatchSelectedIds((current) => {
                const next = new Set(current);
                if (next.has(row.id)) next.delete(row.id);
                else next.add(row.id);
                return next;
              });
            },
            onReplaceBatch: (rangeRows) => {
              setBatchActive(true);
              setBatchSelectedIds(new Set(rangeRows.map((row) => row.id)));
            },
            onSetAllBatch: vi.fn(),
          }}
        />
      );
    }

    render(<KeyboardTable />);
    const grid = screen.getByRole("grid", { name: "Keyboard rows" });
    const active = () =>
      screen.getAllByRole("row").find((row) => row.getAttribute("aria-current") === "true");
    screen.getByRole("row", { name: /2/ }).focus();

    fireEvent.keyDown(grid, { key: "ArrowDown" });
    expect(active()).toHaveAttribute("data-row-id", "three");
    expect(active()).toHaveFocus();
    expect(checkedRowNames()).toEqual([]);

    fireEvent.keyDown(grid, { key: "ArrowDown", shiftKey: true });
    expect(active()).toHaveAttribute("data-row-id", "four");
    expect(checkedRowNames()).toEqual(["three", "four"]);
    fireEvent.keyDown(grid, { key: "ArrowDown", shiftKey: true });
    expect(checkedRowNames()).toEqual(["three", "four", "five"]);
    fireEvent.keyDown(grid, { key: "ArrowUp", shiftKey: true });
    expect(checkedRowNames()).toEqual(["three", "four"]);
    fireEvent.keyDown(grid, { key: "Home", shiftKey: true });
    expect(active()).toHaveAttribute("data-row-id", "one");
    expect(checkedRowNames()).toEqual(["one", "two", "three"]);

    fireEvent.keyDown(grid, { key: "a", metaKey: true });
    expect(screen.getByText("5 selected for actions")).toBeVisible();
    fireEvent.keyDown(grid, { key: "ArrowDown" });
    expect(active()).toHaveAttribute("data-row-id", "two");
    expect(screen.getByText("5 selected for actions")).toBeVisible();
    fireEvent.keyDown(grid, { key: "Enter" });
    expect(screen.getByText("5 selected for actions")).toBeVisible();
    fireEvent.keyDown(grid, { key: "Escape" });
    expect(active()).toHaveAttribute("data-row-id", "two");
    expect(checkedRowNames()).toEqual([]);

    const nestedCheckbox = screen.getByRole("checkbox", { name: "Select one" });
    nestedCheckbox.focus();
    fireEvent.keyDown(nestedCheckbox, { key: "a", ctrlKey: true });
    expect(active()).toHaveAttribute("data-row-id", "two");
    expect(screen.getByRole("row", { name: /2/ })).toHaveAttribute(
      "tabindex",
      "0",
    );
    expect(
      screen.queryByText("5 selected for actions"),
    ).not.toBeInTheDocument();

    fireEvent.keyDown(grid, { key: "a", ctrlKey: true });
    expect(screen.getByText("5 selected for actions")).toBeVisible();
  });

  it("selects offscreen logical rows and discloses hidden batch targets", () => {
    function LargeTable() {
      const rows = Array.from({ length: 200 }, (_, index) => ({
        id: `row-${index}`,
        value: String(index),
      }));
      const [batchSelectedIds, setBatchSelectedIds] = useState<
        ReadonlySet<string>
      >(new Set(["outside-current-view"]));
      return (
        <VirtualTable
          tableId="logical-select-all-test"
          label="Logical rows"
          rows={rows}
          getRowId={(row) => row.id}
          columns={COLUMNS}
          interfaceSize="standard"
          activeRowId="row-0"
          onActivate={vi.fn()}
          emptyMessage="empty"
          batchSelection={{
            active: true,
            batchSelectedIds,
            getRowLabel: (row) => row.id,
            onEnterBatch: vi.fn(),
            onExitBatch: vi.fn(),
            onToggleBatch: vi.fn(),
            onReplaceBatch: (rangeRows) =>
              setBatchSelectedIds(new Set(rangeRows.map((row) => row.id))),
            onSetAllBatch: vi.fn(),
          }}
        />
      );
    }

    render(<LargeTable />);
    const grid = screen.getByRole("grid", { name: "Logical rows" });
    expect(screen.getByText("1 selected for actions (1 outside this view)")).toBeVisible();
    expect(grid.querySelectorAll('[role="row"]').length).toBeLessThan(100);
    grid.querySelector<HTMLElement>('[data-row-id="row-0"]')?.focus();
    fireEvent.keyDown(grid, { key: "a", ctrlKey: true });
    expect(screen.getByText("200 selected for actions")).toBeVisible();
    expect(grid.querySelectorAll('[role="row"]').length).toBeLessThan(100);
  });

  it("falls back to the clicked row when the range anchor disappears", () => {
    function FilteredRangeTable() {
      const [rows, setRows] = useState([
        { id: "one", value: "1" },
        { id: "two", value: "2" },
        { id: "three", value: "3" },
      ]);
      const [active, setActive] = useState(false);
      const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(
        new Set(),
      );
      return (
        <>
          <button
            type="button"
            onClick={() =>
              setRows((current) =>
                current.filter((row) => row.id !== "two"),
              )
            }
          >
            Hide anchor
          </button>
          <VirtualTable
            tableId="missing-anchor-test"
            label="Filtered rows"
            rows={rows}
            getRowId={(row) => row.id}
            columns={COLUMNS}
            interfaceSize="standard"
            emptyMessage="empty"
            batchSelection={{
              active,
              batchSelectedIds: selectedIds,
              getRowLabel: (row) => row.id,
              onEnterBatch: (row) => {
                setActive(true);
                setSelectedIds(new Set(row === undefined ? [] : [row.id]));
              },
              onExitBatch: () => {
                setActive(false);
                setSelectedIds(new Set());
              },
              onToggleBatch: (row) => {
                setActive(true);
                setSelectedIds(new Set([row.id]));
              },
              onReplaceBatch: (rangeRows) => {
                setActive(true);
                setSelectedIds(new Set(rangeRows.map((row) => row.id)));
              },
              onSetAllBatch: vi.fn(),
            }}
          />
        </>
      );
    }

    render(<FilteredRangeTable />);
    fireEvent.click(screen.getByRole("checkbox", { name: "Select two" }));
    fireEvent.click(screen.getByRole("button", { name: "Hide anchor" }));
    fireEvent.click(screen.getByRole("row", { name: /3/ }), { shiftKey: true });
    expect(checkedRowNames()).toEqual(["three"]);
  });

  it("enters selection on a stationary touch hold and cancels on movement", () => {
    vi.useFakeTimers();
    const enter = vi.fn();
    render(
      <VirtualTable
        tableId="long-press-test"
        label="Touch rows"
        rows={[{ id: "one", value: "1" }]}
        getRowId={(row) => row.id}
        columns={COLUMNS}
        interfaceSize="standard"
        emptyMessage="empty"
        batchSelection={{
          active: false,
          batchSelectedIds: new Set(),
          getRowLabel: (row) => row.id,
          onEnterBatch: enter,
          onExitBatch: vi.fn(),
          onToggleBatch: vi.fn(),
          onReplaceBatch: vi.fn(),
          onSetAllBatch: vi.fn(),
        }}
      />,
    );
    const row = screen.getByRole("row", { name: /1/ });
    fireEvent.pointerDown(row, {
      button: 0,
      pointerId: 1,
      pointerType: "touch",
      clientX: 10,
      clientY: 10,
    });
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 21, clientY: 10 });
    act(() => vi.advanceTimersByTime(500));
    expect(enter).not.toHaveBeenCalled();

    fireEvent.pointerDown(row, {
      button: 0,
      pointerId: 2,
      pointerType: "touch",
      clientX: 10,
      clientY: 10,
    });
    act(() => vi.advanceTimersByTime(500));
    expect(enter).toHaveBeenCalledWith(expect.objectContaining({ id: "one" }));
    vi.useRealTimers();
  });
});

function rowIds(container: HTMLElement): string[] {
  return [...container.querySelectorAll<HTMLElement>("[data-row-id]")].map(
    (element) => element.dataset.rowId ?? "",
  );
}

function checkedRowNames(): string[] {
  return screen
    .getAllByRole("checkbox")
    .filter((checkbox) => checkbox.getAttribute("aria-label")?.startsWith("Deselect "))
    .map((checkbox) => checkbox.getAttribute("aria-label")!.slice("Deselect ".length));
}
