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
        emptyMessage="empty"
      />,
    );
    expect(screen.getByRole("columnheader", { name: "Value" })).toBeVisible();
    expect(screen.getByRole("separator", { name: "Resize Value column" })).toHaveAttribute(
      "aria-valuenow",
      "720",
    );
  });
});

function rowIds(container: HTMLElement): string[] {
  return [...container.querySelectorAll<HTMLElement>("[data-row-id]")].map(
    (element) => element.dataset.rowId ?? "",
  );
}
