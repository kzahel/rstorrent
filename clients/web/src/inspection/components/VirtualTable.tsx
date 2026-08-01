import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
  type UIEvent,
} from "react";

import styles from "./VirtualTable.module.css";

export interface VirtualColumn<Row> {
  readonly id: string;
  readonly label: string;
  readonly width: number;
  readonly minimumWidth?: number;
  readonly maximumWidth?: number;
  readonly defaultVisible?: boolean;
  readonly minimumViewport?: number;
  readonly align?: "left" | "right" | "center";
  readonly sortable?: boolean;
  readonly sortValue?: (row: Row) => string | number | null;
  readonly sortKind?: "text" | "number" | "decimal";
  readonly sortOrder?: readonly string[];
  readonly render: (row: Row) => ReactNode;
}

export interface VirtualTableProps<Row> {
  readonly tableId: string;
  readonly label: string;
  readonly rows: readonly Row[];
  readonly getRowId: (row: Row) => string;
  readonly columns: readonly VirtualColumn<Row>[];
  readonly rowHeight?: number;
  readonly overscan?: number;
  readonly selectedId?: string | null;
  readonly onSelect?: (row: Row) => void;
  readonly emptyMessage: string;
  readonly initialSort?: { readonly columnId: string; readonly direction: "asc" | "desc" };
}

interface SortState {
  readonly columnId: string;
  readonly direction: "asc" | "desc";
}

const HEADER_HEIGHT = 34;
const TABLE_CONFIG_VERSION = 1;

export function VirtualTable<Row>({
  tableId,
  label,
  rows,
  getRowId,
  columns,
  rowHeight = 32,
  overscan = 8,
  selectedId = null,
  onSelect,
  emptyMessage,
  initialSort,
}: VirtualTableProps<Row>) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportSize, setViewportSize] = useState({ width: 960, height: 360 });
  const [focusIndex, setFocusIndex] = useState(0);
  const [sort, setSort] = useState<SortState | null>(initialSort ?? null);
  const [liveSort, setLiveSort] = useState(false);
  const [frozenOrder, setFrozenOrder] = useState<readonly string[] | null>(null);
  const [hiddenColumns, setHiddenColumns] = useState<ReadonlySet<string>>(
    () => loadTableConfig(tableId, columns)?.hiddenColumns ?? defaultHiddenColumns(columns),
  );
  const [columnWidths, setColumnWidths] = useState<Readonly<Record<string, number>>>(
    () => loadTableConfig(tableId, columns)?.widths ?? {},
  );
  const [columnsOpen, setColumnsOpen] = useState(false);
  const columnsButtonRef = useRef<HTMLButtonElement>(null);
  const resizeRef = useRef<{
    readonly columnId: string;
    readonly pointerId: number;
    readonly startX: number;
    readonly startWidth: number;
  } | null>(null);

  useEffect(() => {
    const loaded = loadTableConfig(tableId, columns);
    setHiddenColumns(loaded?.hiddenColumns ?? defaultHiddenColumns(columns));
    setColumnWidths(loaded?.widths ?? {});
    setSort(loaded?.sort ?? initialSort ?? null);
    setLiveSort(loaded?.liveSort ?? false);
    setFrozenOrder(null);
  }, [columns, initialSort, tableId]);

  useEffect(() => {
    saveTableConfig(tableId, {
      hiddenColumns,
      widths: columnWidths,
      sort,
      liveSort,
    });
  }, [columnWidths, hiddenColumns, liveSort, sort, tableId]);

  useEffect(() => {
    const element = viewportRef.current;
    if (element === null) return;
    const measure = () => {
      setViewportSize({
        width: element.clientWidth || 960,
        height: element.clientHeight || 360,
      });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const visibleColumns = useMemo(
    () =>
      columns
        .filter(
          (column) =>
            !hiddenColumns.has(column.id) &&
            (column.minimumViewport === undefined ||
              viewportSize.width >= column.minimumViewport),
        )
        .map((column) => ({
          ...column,
          width: columnWidths[column.id] ?? column.width,
        })),
    [columnWidths, columns, hiddenColumns, viewportSize.width],
  );
  const sortedRows = useMemo(() => {
    if (sort === null) return rows;
    const column = columns.find((candidate) => candidate.id === sort.columnId);
    if (column?.sortValue === undefined) return rows;
    if (liveSort || frozenOrder === null) {
      return sortRows(rows, getRowId, column, sort.direction);
    }
    const byId = new Map(rows.map((row) => [getRowId(row), row]));
    const retained = frozenOrder
      .map((id) => byId.get(id))
      .filter((row): row is Row => row !== undefined);
    const retainedIds = new Set(retained.map(getRowId));
    return [...retained, ...rows.filter((row) => !retainedIds.has(getRowId(row)))];
  }, [columns, frozenOrder, getRowId, liveSort, rows, sort]);

  useEffect(() => {
    if (sort === null || liveSort || frozenOrder !== null) return;
    const column = columns.find((candidate) => candidate.id === sort.columnId);
    if (column?.sortValue === undefined) return;
    setFrozenOrder(
      sortRows(rows, getRowId, column, sort.direction).map(getRowId),
    );
  }, [columns, frozenOrder, getRowId, liveSort, rows, sort]);

  const bodyScroll = Math.max(0, scrollTop - HEADER_HEIGHT);
  const firstIndex = Math.max(0, Math.floor(bodyScroll / rowHeight) - overscan);
  const visibleCount = Math.ceil(viewportSize.height / rowHeight) + overscan * 2;
  const lastIndex = Math.min(sortedRows.length, firstIndex + visibleCount);
  const renderedRows = sortedRows.slice(firstIndex, lastIndex);
  const gridTemplateColumns = visibleColumns
    .map((column) => `${column.width}px`)
    .join(" ");
  const minimumWidth = visibleColumns.reduce((sum, column) => sum + column.width, 0);
  const gridStyle = {
    gridTemplateColumns,
    minWidth: `${minimumWidth}px`,
  } satisfies CSSProperties;

  const handleScroll = (event: UIEvent<HTMLDivElement>) => {
    setScrollTop(event.currentTarget.scrollTop);
  };

  const changeSort = (column: VirtualColumn<Row>) => {
    if (column.sortable === false || column.sortValue === undefined) return;
    const next: SortState = {
      columnId: column.id,
      direction:
        sort?.columnId === column.id && sort.direction === "asc"
          ? "desc"
          : "asc",
    };
    setSort(next);
    setFrozenOrder(sortRows(rows, getRowId, column, next.direction).map(getRowId));
  };

  const moveFocus = (nextIndex: number) => {
    if (sortedRows.length === 0) return;
    const clamped = Math.max(0, Math.min(sortedRows.length - 1, nextIndex));
    setFocusIndex(clamped);
    const viewport = viewportRef.current;
    if (viewport !== null) {
      const fixedHeight = HEADER_HEIGHT;
      const top = fixedHeight + clamped * rowHeight;
      if (top < viewport.scrollTop + fixedHeight) {
        viewport.scrollTop = top - fixedHeight;
      } else if (top + rowHeight > viewport.scrollTop + viewport.clientHeight) {
        viewport.scrollTop = top + rowHeight - viewport.clientHeight;
      }
      requestAnimationFrame(() => {
        viewport
          .querySelector<HTMLElement>(`[data-row-index="${clamped}"]`)
          ?.focus();
      });
    }
  };

  const resizeColumn = (column: VirtualColumn<Row>, width: number) => {
    const next = Math.round(
      Math.min(
        column.maximumWidth ?? 720,
        Math.max(column.minimumWidth ?? 52, width),
      ),
    );
    setColumnWidths((current) => ({ ...current, [column.id]: next }));
  };

  const startResize = (
    event: PointerEvent<HTMLDivElement>,
    column: VirtualColumn<Row>,
  ) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    resizeRef.current = {
      columnId: column.id,
      pointerId: event.pointerId,
      startX: event.clientX,
      startWidth: column.width,
    };
    event.currentTarget.setPointerCapture?.(event.pointerId);
  };

  const continueResize = (
    event: PointerEvent<HTMLDivElement>,
    column: VirtualColumn<Row>,
  ) => {
    const resize = resizeRef.current;
    if (resize?.pointerId !== event.pointerId || resize.columnId !== column.id) return;
    resizeColumn(column, resize.startWidth + event.clientX - resize.startX);
  };

  const stopResize = (event: PointerEvent<HTMLDivElement>) => {
    if (resizeRef.current?.pointerId !== event.pointerId) return;
    resizeRef.current = null;
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const resizeWithKeyboard = (
    event: KeyboardEvent<HTMLDivElement>,
    column: VirtualColumn<Row>,
  ) => {
    const current = columnWidths[column.id] ?? column.width;
    const next =
      event.key === "ArrowLeft"
        ? current - 12
        : event.key === "ArrowRight"
          ? current + 12
          : event.key === "Home"
            ? (column.minimumWidth ?? 52)
            : event.key === "End"
              ? (column.maximumWidth ?? 720)
              : null;
    if (next === null) return;
    event.preventDefault();
    event.stopPropagation();
    resizeColumn(column, next);
  };

  const configuredVisibleCount = columns.filter(
    (column) => !hiddenColumns.has(column.id),
  ).length;

  return (
    <div className={styles.container}>
      <div className={styles.toolbar}>
        <span>{sortedRows.length.toLocaleString()} rows</span>
        <button
          ref={columnsButtonRef}
          type="button"
          aria-haspopup="dialog"
          aria-expanded={columnsOpen}
          onClick={() => setColumnsOpen((open) => !open)}
        >
          Columns
        </button>
        {columnsOpen ? (
          <div
            className={styles.columnMenu}
            role="dialog"
            aria-label="Table column settings"
            onKeyDown={(event) => {
              if (event.key !== "Escape") return;
              event.preventDefault();
              setColumnsOpen(false);
              requestAnimationFrame(() => columnsButtonRef.current?.focus());
            }}
          >
            <strong>Visible columns</strong>
            {columns.map((column) => {
              const checked = !hiddenColumns.has(column.id);
              return (
                <label key={column.id}>
                  <input
                    type="checkbox"
                    checked={checked}
                    disabled={checked && configuredVisibleCount === 1}
                    onChange={(event) => {
                      const next = new Set(hiddenColumns);
                      if (event.currentTarget.checked) next.delete(column.id);
                      else next.add(column.id);
                      setHiddenColumns(next);
                    }}
                  />
                  {column.label}
                </label>
              );
            })}
            <label className={styles.liveSort}>
              <input
                type="checkbox"
                checked={liveSort}
                onChange={(event) => {
                  const enabled = event.currentTarget.checked;
                  if (!enabled && sort !== null) {
                    const column = columns.find((candidate) => candidate.id === sort.columnId);
                    if (column?.sortValue !== undefined) {
                      setFrozenOrder(
                        sortRows(rows, getRowId, column, sort.direction).map(getRowId),
                      );
                    }
                  }
                  setLiveSort(enabled);
                }}
              />
              Re-sort live updates
            </label>
            <button
              type="button"
              onClick={() => {
                setHiddenColumns(
                  new Set(
                    columns
                      .filter((column) => column.defaultVisible === false)
                      .map((column) => column.id),
                  ),
                );
                setColumnWidths({});
                setSort(initialSort ?? null);
                setLiveSort(false);
                setFrozenOrder(null);
              }}
            >
              Reset table
            </button>
          </div>
        ) : null}
      </div>
      <div
        ref={viewportRef}
        className={styles.viewport}
        role="grid"
        aria-label={label}
        aria-rowcount={sortedRows.length + 1}
        aria-colcount={visibleColumns.length}
        onScroll={handleScroll}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            moveFocus(focusIndex + 1);
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            moveFocus(focusIndex - 1);
          } else if (event.key === "Home") {
            event.preventDefault();
            moveFocus(0);
          } else if (event.key === "End") {
            event.preventDefault();
            moveFocus(sortedRows.length - 1);
          } else if (event.key === "Enter" || event.key === " ") {
            const row = sortedRows[focusIndex];
            if (row !== undefined && onSelect !== undefined) {
              event.preventDefault();
              onSelect(row);
            }
          }
        }}
        data-testid="virtual-table"
      >
      <div className={styles.header} role="row" style={gridStyle}>
        {visibleColumns.map((column) => {
          const sorted = sort?.columnId === column.id;
          return (
            <div
              key={column.id}
              className={styles.headerCell}
              role="columnheader"
              aria-label={column.label}
              aria-sort={
                sorted
                  ? sort.direction === "asc"
                    ? "ascending"
                    : "descending"
                  : undefined
              }
              data-align={column.align ?? "left"}
            >
              <button
                type="button"
                onClick={() => changeSort(column)}
                disabled={column.sortable === false || column.sortValue === undefined}
              >
                <span>{column.label}</span>
                {sorted ? <span aria-hidden="true">{sort.direction === "asc" ? "▲" : "▼"}</span> : null}
              </button>
              <div
                className={styles.resizeHandle}
                role="separator"
                aria-label={`Resize ${column.label} column`}
                aria-orientation="vertical"
                aria-valuemin={column.minimumWidth ?? 52}
                aria-valuemax={column.maximumWidth ?? 720}
                aria-valuenow={column.width}
                tabIndex={0}
                onPointerDown={(event) => startResize(event, column)}
                onPointerMove={(event) => continueResize(event, column)}
                onPointerUp={stopResize}
                onPointerCancel={stopResize}
                onKeyDown={(event) => resizeWithKeyboard(event, column)}
              />
            </div>
          );
        })}
      </div>
      {sortedRows.length === 0 ? (
        <div className={styles.empty}>{emptyMessage}</div>
      ) : (
        <div
          className={styles.canvas}
          style={{
            height: `${sortedRows.length * rowHeight}px`,
            minWidth: `${minimumWidth}px`,
          }}
        >
          {renderedRows.map((row, offset) => {
            const index = firstIndex + offset;
            const rowId = getRowId(row);
            const selected = rowId === selectedId;
            return (
              <div
                key={rowId}
                className={styles.row}
                role="row"
                aria-rowindex={index + 2}
                aria-selected={selected}
                tabIndex={index === focusIndex ? 0 : -1}
                data-row-index={index}
                data-row-id={rowId}
                data-selected={selected}
                style={{
                  ...gridStyle,
                  height: `${rowHeight}px`,
                  transform: `translateY(${index * rowHeight}px)`,
                }}
                onFocus={() => setFocusIndex(index)}
                onClick={() => {
                  setFocusIndex(index);
                  onSelect?.(row);
                }}
              >
                {visibleColumns.map((column) => (
                  <div
                    key={column.id}
                    className={styles.cell}
                    role="gridcell"
                    data-align={column.align ?? "left"}
                  >
                    {column.render(row)}
                  </div>
                ))}
              </div>
            );
          })}
        </div>
      )}
      </div>
    </div>
  );
}

function sortRows<Row>(
  rows: readonly Row[],
  getRowId: (row: Row) => string,
  column: VirtualColumn<Row>,
  direction: "asc" | "desc",
): Row[] {
  const multiplier = direction === "asc" ? 1 : -1;
  return [...rows].sort((left, right) => {
    const leftValue = column.sortValue?.(left) ?? null;
    const rightValue = column.sortValue?.(right) ?? null;
    if (leftValue === null && rightValue === null) {
      return stableIdCompare(getRowId(left), getRowId(right));
    }
    if (leftValue === null) return 1;
    if (rightValue === null) return -1;
    const compared = comparePresent(leftValue, rightValue, column);
    return compared === 0
      ? stableIdCompare(getRowId(left), getRowId(right))
      : compared * multiplier;
  });
}

function comparePresent<Row>(
  left: string | number,
  right: string | number,
  column: VirtualColumn<Row>,
): number {
  if (column.sortOrder !== undefined) {
    const leftRank = column.sortOrder.indexOf(String(left));
    const rightRank = column.sortOrder.indexOf(String(right));
    if (leftRank !== rightRank) {
      return (leftRank < 0 ? Number.MAX_SAFE_INTEGER : leftRank) -
        (rightRank < 0 ? Number.MAX_SAFE_INTEGER : rightRank);
    }
  }
  if (column.sortKind === "decimal") {
    const leftBigInt = canonicalBigInt(left);
    const rightBigInt = canonicalBigInt(right);
    return leftBigInt < rightBigInt ? -1 : leftBigInt > rightBigInt ? 1 : 0;
  }
  if (column.sortKind === "number" || (typeof left === "number" && typeof right === "number")) {
    return Number(left) - Number(right);
  }
  return String(left).localeCompare(String(right), undefined, {
    numeric: true,
    sensitivity: "base",
  });
}

function canonicalBigInt(value: string | number): bigint {
  try {
    return BigInt(value);
  } catch {
    return 0n;
  }
}

function stableIdCompare(left: string, right: string): number {
  return left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" });
}

function defaultHiddenColumns<Row>(
  columns: readonly VirtualColumn<Row>[],
): ReadonlySet<string> {
  return new Set(
    columns
      .filter((column) => column.defaultVisible === false)
      .map((column) => column.id),
  );
}

interface PersistedTableConfig {
  readonly hiddenColumns: ReadonlySet<string>;
  readonly widths: Readonly<Record<string, number>>;
  readonly sort: SortState | null;
  readonly liveSort: boolean;
}

function loadTableConfig<Row>(
  tableId: string,
  columns: readonly VirtualColumn<Row>[],
): PersistedTableConfig | null {
  try {
    const source = globalThis.localStorage?.getItem(`rstorrent.table.${tableId}`);
    if (source === null || source === undefined) return null;
    const value = JSON.parse(source) as {
      version?: number;
      hiddenColumns?: unknown;
      widths?: unknown;
      sort?: unknown;
      liveSort?: unknown;
    };
    if (value.version !== TABLE_CONFIG_VERSION) return null;
    const knownColumns = new Map(columns.map((column) => [column.id, column]));
    let hiddenColumns = Array.isArray(value.hiddenColumns)
      ? new Set(
          value.hiddenColumns.filter(
            (item): item is string =>
              typeof item === "string" && knownColumns.has(item),
          ),
        )
      : new Set<string>();
    if (hiddenColumns.size >= columns.length && columns[0] !== undefined) {
      hiddenColumns = new Set(hiddenColumns);
      hiddenColumns.delete(columns[0].id);
    }
    const rawWidths =
      typeof value.widths === "object" && value.widths !== null
        ? Object.fromEntries(
            Object.entries(value.widths).filter(
              ([id, width]) =>
                knownColumns.has(id) &&
                typeof width === "number" &&
                Number.isFinite(width),
            ),
          )
        : {};
    const widths = Object.fromEntries(
      Object.entries(rawWidths).map(([id, width]) => {
        const column = knownColumns.get(id)!;
        return [
          id,
          Math.round(
            Math.min(
              column.maximumWidth ?? 720,
              Math.max(column.minimumWidth ?? 52, width),
            ),
          ),
        ];
      }),
    );
    const candidateSort: SortState | null =
      typeof value.sort === "object" &&
      value.sort !== null &&
      "columnId" in value.sort &&
      typeof value.sort.columnId === "string" &&
      "direction" in value.sort &&
      (value.sort.direction === "asc" || value.sort.direction === "desc")
        ? { columnId: value.sort.columnId, direction: value.sort.direction }
        : null;
    const sortColumn =
      candidateSort === null ? undefined : knownColumns.get(candidateSort.columnId);
    const sort =
      sortColumn?.sortValue !== undefined && sortColumn.sortable !== false
        ? candidateSort
        : null;
    return {
      hiddenColumns,
      widths,
      sort,
      liveSort: value.liveSort === true,
    };
  } catch {
    return null;
  }
}

function saveTableConfig(tableId: string, config: PersistedTableConfig): void {
  try {
    globalThis.localStorage?.setItem(
      `rstorrent.table.${tableId}`,
      JSON.stringify({
        version: TABLE_CONFIG_VERSION,
        hiddenColumns: [...config.hiddenColumns],
        widths: config.widths,
        sort: config.sort,
        liveSort: config.liveSort,
      }),
    );
  } catch {
    // Storage is optional in private/sandboxed browsing contexts.
  }
}
