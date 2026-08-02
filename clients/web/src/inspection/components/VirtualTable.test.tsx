// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

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
});

function rowIds(container: HTMLElement): string[] {
  return [...container.querySelectorAll<HTMLElement>("[data-row-id]")].map(
    (element) => element.dataset.rowId ?? "",
  );
}
