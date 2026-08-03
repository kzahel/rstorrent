import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  type UIEvent,
} from "react";
import { createPortal } from "react-dom";

import {
  INTERFACE_METRICS,
  type InterfaceSize,
} from "../appearance";
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
  readonly headerHelp?: ReactNode;
  readonly headerHelpWidth?: number;
  readonly render: (row: Row) => ReactNode;
}

export interface VirtualTableProps<Row> {
  readonly tableId: string;
  readonly label: string;
  readonly rows: readonly Row[];
  readonly getRowId: (row: Row) => string;
  readonly columns: readonly VirtualColumn<Row>[];
  readonly interfaceSize: InterfaceSize;
  readonly overscan?: number;
  readonly selectedId?: string | null;
  readonly selection?: VirtualTableSelection<Row>;
  readonly onSelect?: (row: Row) => void;
  readonly onClear?: () => void;
  readonly emptyMessage: string;
  readonly initialSort?: { readonly columnId: string; readonly direction: "asc" | "desc" };
}

export interface VirtualTableSelection<Row> {
  readonly active: boolean;
  readonly selectedIds: ReadonlySet<string>;
  readonly getRowLabel: (row: Row) => string;
  readonly onEnter: (row?: Row) => void;
  readonly onExit: () => void;
  readonly onToggle: (row: Row) => void;
  readonly onReplace: (rows: readonly Row[]) => void;
  readonly onSetAll: (rows: readonly Row[], selected: boolean) => void;
}

interface SortState {
  readonly columnId: string;
  readonly direction: "asc" | "desc";
}

interface HeaderHelpState {
  readonly columnId: string;
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly maximumHeight: number;
}

const TABLE_CONFIG_VERSION = 1;

export function VirtualTable<Row>({
  tableId,
  label,
  rows,
  getRowId,
  columns,
  interfaceSize,
  overscan = 8,
  selectedId = null,
  selection,
  onSelect,
  onClear,
  emptyMessage,
  initialSort,
}: VirtualTableProps<Row>) {
  const { tableHeaderHeight, tableRowHeight } =
    INTERFACE_METRICS[interfaceSize];
  const tableStyle = {
    "--ui-table-header-height": `${tableHeaderHeight}px`,
    "--ui-table-row-height": `${tableRowHeight}px`,
  } as CSSProperties;
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
  const headerHelpIdPrefix = useId().replaceAll(":", "");
  const headerHelpButtonRef = useRef<HTMLButtonElement>(null);
  const headerHelpPopoverRef = useRef<HTMLDivElement>(null);
  const [headerHelp, setHeaderHelp] = useState<HeaderHelpState | null>(null);
  const resizeRef = useRef<{
    readonly columnId: string;
    readonly pointerId: number;
    readonly startX: number;
    readonly startWidth: number;
  } | null>(null);
  const longPressRef = useRef<{
    readonly pointerId: number;
    readonly rowId: string;
    readonly startX: number;
    readonly startY: number;
    readonly timer: ReturnType<typeof globalThis.setTimeout>;
  } | null>(null);
  const suppressClickRowRef = useRef<string | null>(null);
  const selectionAnchorIdRef = useRef<string | null>(null);

  useEffect(
    () => () => {
      const press = longPressRef.current;
      if (press !== null) globalThis.clearTimeout(press.timer);
    },
    [],
  );

  useEffect(() => {
    if (selection?.active !== true || longPressRef.current === null) return;
    globalThis.clearTimeout(longPressRef.current.timer);
    longPressRef.current = null;
  }, [selection?.active]);

  useEffect(() => {
    if (selection?.active !== true) selectionAnchorIdRef.current = null;
  }, [selection?.active]);

  useEffect(() => {
    const loaded = loadTableConfig(tableId, columns);
    setHiddenColumns(loaded?.hiddenColumns ?? defaultHiddenColumns(columns));
    setColumnWidths(loaded?.widths ?? {});
    setSort(loaded?.sort ?? initialSort ?? null);
    setLiveSort(loaded?.liveSort ?? false);
    setFrozenOrder(null);
    setHeaderHelp(null);
  }, [columns, initialSort, tableId]);

  useEffect(() => {
    if (headerHelp === null) return;
    headerHelpPopoverRef.current?.focus();
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setHeaderHelp(null);
      headerHelpButtonRef.current?.focus();
    };
    const closeOnResize = () => setHeaderHelp(null);
    document.addEventListener("keydown", closeOnEscape);
    globalThis.addEventListener("resize", closeOnResize);
    return () => {
      document.removeEventListener("keydown", closeOnEscape);
      globalThis.removeEventListener("resize", closeOnResize);
    };
  }, [headerHelp]);

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

  const bodyScroll = Math.max(0, scrollTop - tableHeaderHeight);
  const firstIndex = Math.max(
    0,
    Math.floor(bodyScroll / tableRowHeight) - overscan,
  );
  const visibleCount =
    Math.ceil(viewportSize.height / tableRowHeight) + overscan * 2;
  const lastIndex = Math.min(sortedRows.length, firstIndex + visibleCount);
  const renderedRows = sortedRows.slice(firstIndex, lastIndex);
  const selectionActive = selection?.active === true;
  const selectionAvailable = selection !== undefined;
  const selectionColumnWidth = selectionAvailable ? 44 : 0;
  const gridTemplateColumns = [
    ...(selectionAvailable ? [`${selectionColumnWidth}px`] : []),
    ...visibleColumns.map((column) => `${column.width}px`),
  ].join(" ");
  const minimumWidth =
    selectionColumnWidth +
    visibleColumns.reduce((sum, column) => sum + column.width, 0);
  const gridStyle = {
    gridTemplateColumns,
    minWidth: `${minimumWidth}px`,
  } satisfies CSSProperties;

  const handleScroll = (event: UIEvent<HTMLDivElement>) => {
    setScrollTop(event.currentTarget.scrollTop);
    if (longPressRef.current !== null) {
      globalThis.clearTimeout(longPressRef.current.timer);
      longPressRef.current = null;
    }
  };

  const cancelLongPress = (pointerId: number) => {
    const press = longPressRef.current;
    if (press?.pointerId !== pointerId) return;
    globalThis.clearTimeout(press.timer);
    longPressRef.current = null;
  };

  const startLongPress = (
    event: ReactPointerEvent<HTMLDivElement>,
    row: Row,
    rowId: string,
  ) => {
    if (
      selection === undefined ||
      selection.active ||
      event.pointerType === "mouse" ||
      event.button !== 0
    ) {
      return;
    }
    if (longPressRef.current !== null) {
      globalThis.clearTimeout(longPressRef.current.timer);
    }
    const pointerId = event.pointerId;
    const timer = globalThis.setTimeout(() => {
      const press = longPressRef.current;
      if (press?.pointerId !== pointerId || press.rowId !== rowId) return;
      longPressRef.current = null;
      suppressClickRowRef.current = rowId;
      selectionAnchorIdRef.current = rowId;
      selection.onEnter(row);
    }, 500);
    longPressRef.current = {
      pointerId,
      rowId,
      startX: event.clientX,
      startY: event.clientY,
      timer,
    };
  };

  const continueLongPress = (event: ReactPointerEvent<HTMLDivElement>) => {
    const press = longPressRef.current;
    if (press?.pointerId !== event.pointerId) return;
    if (
      Math.hypot(event.clientX - press.startX, event.clientY - press.startY) >
      10
    ) {
      cancelLongPress(event.pointerId);
    }
  };

  const activateRow = (
    event: ReactMouseEvent<HTMLDivElement>,
    row: Row,
    rowId: string,
  ) => {
    const suppressedRow = suppressClickRowRef.current;
    if (suppressedRow !== null) {
      suppressClickRowRef.current = null;
      if (suppressedRow === rowId) return;
    }
    if (selection !== undefined && event.shiftKey) {
      replaceSelectionRange(row, rowId);
      return;
    }
    if (selection !== undefined && (event.metaKey || event.ctrlKey)) {
      if (selection.active) toggleRowSelection(row, rowId);
      else enterRowSelection(row, rowId);
      return;
    }
    if (selection?.active === true) toggleRowSelection(row, rowId);
    else onSelect?.(row);
  };

  const enterRowSelection = (row: Row, rowId: string) => {
    selectionAnchorIdRef.current = rowId;
    selection?.onEnter(row);
  };

  const toggleRowSelection = (row: Row, rowId: string) => {
    selectionAnchorIdRef.current = rowId;
    selection?.onToggle(row);
  };

  const replaceSelectionRange = (row: Row, rowId: string) => {
    if (selection === undefined) return;
    let anchorId = selectionAnchorIdRef.current;
    let anchorIndex =
      anchorId === null
        ? -1
        : sortedRows.findIndex((candidate) => getRowId(candidate) === anchorId);
    if (anchorIndex < 0 && selectedId !== null) {
      const currentIndex = sortedRows.findIndex(
        (candidate) => getRowId(candidate) === selectedId,
      );
      if (currentIndex >= 0) {
        anchorIndex = currentIndex;
        anchorId = selectedId;
      }
    }
    const clickedIndex = sortedRows.findIndex(
      (candidate) => getRowId(candidate) === rowId,
    );
    if (clickedIndex < 0) return;
    if (anchorIndex < 0) {
      anchorIndex = clickedIndex;
      anchorId = rowId;
    }
    selectionAnchorIdRef.current = anchorId;
    const start = Math.min(anchorIndex, clickedIndex);
    const end = Math.max(anchorIndex, clickedIndex);
    selection.onReplace(sortedRows.slice(start, end + 1));
  };

  const activateBackground = () => {
    if (selection?.active === true) selection.onExit();
    else onClear?.();
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
      const fixedHeight = tableHeaderHeight;
      const top = fixedHeight + clamped * tableRowHeight;
      if (top < viewport.scrollTop + fixedHeight) {
        viewport.scrollTop = top - fixedHeight;
      } else if (
        top + tableRowHeight >
        viewport.scrollTop + viewport.clientHeight
      ) {
        viewport.scrollTop =
          top + tableRowHeight - viewport.clientHeight;
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
    event: ReactPointerEvent<HTMLDivElement>,
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
    event: ReactPointerEvent<HTMLDivElement>,
    column: VirtualColumn<Row>,
  ) => {
    const resize = resizeRef.current;
    if (resize?.pointerId !== event.pointerId || resize.columnId !== column.id) return;
    resizeColumn(column, resize.startWidth + event.clientX - resize.startX);
  };

  const stopResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (resizeRef.current?.pointerId !== event.pointerId) return;
    resizeRef.current = null;
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const resizeWithKeyboard = (
    event: ReactKeyboardEvent<HTMLDivElement>,
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
  const headerHelpColumn =
    headerHelp === null
      ? undefined
      : visibleColumns.find((column) => column.id === headerHelp.columnId);
  const selectedVisibleCount =
    selection === undefined
      ? 0
      : sortedRows.filter((row) =>
          selection.selectedIds.has(getRowId(row)),
        ).length;
  const allVisibleSelected =
    sortedRows.length > 0 && selectedVisibleCount === sortedRows.length;

  const toggleHeaderHelp = (
    trigger: HTMLButtonElement,
    column: VirtualColumn<Row>,
  ) => {
    if (headerHelp?.columnId === column.id) {
      setHeaderHelp(null);
      return;
    }
    const bounds = trigger.getBoundingClientRect();
    const viewportWidth = globalThis.innerWidth || 1_024;
    const viewportHeight = globalThis.innerHeight || 768;
    const width = Math.min(
      column.headerHelpWidth ?? 352,
      Math.max(0, viewportWidth - 16),
    );
    const left = Math.max(
      8,
      Math.min(bounds.right - width, viewportWidth - width - 8),
    );
    const belowTop = bounds.bottom + 6;
    const top = viewportHeight - belowTop >= 420 ? belowTop : 8;
    headerHelpButtonRef.current = trigger;
    setHeaderHelp({
      columnId: column.id,
      left,
      top,
      width,
      maximumHeight: Math.max(160, viewportHeight - top - 8),
    });
  };

  return (
    <>
    <div className={styles.container} style={tableStyle}>
      <div className={styles.toolbar}>
        <span>
          {sortedRows.length.toLocaleString()} rows
        </span>
        {selection === undefined ? null : selection.active ? (
          <>
            <strong className={styles.selectionStatus} aria-live="polite">
              {selection.selectedIds.size.toLocaleString()} selected
            </strong>
            <button
              type="button"
              aria-label={`Done selecting rows in ${label}`}
              onClick={selection.onExit}
            >
              Done
            </button>
          </>
        ) : (
          <button
            type="button"
            aria-label={`Select rows in ${label}`}
            disabled={sortedRows.length === 0}
            onClick={() => {
              const row = sortedRows.find(
                (candidate) => getRowId(candidate) === selectedId,
              );
              if (row !== undefined) {
                selectionAnchorIdRef.current = getRowId(row);
              }
              selection.onEnter(row);
            }}
          >
            Select
          </button>
        )}
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
        aria-colcount={visibleColumns.length + (selectionAvailable ? 1 : 0)}
        aria-multiselectable={selectionAvailable ? true : undefined}
        onScroll={handleScroll}
        onClick={(event) => {
          if (event.target === event.currentTarget) activateBackground();
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape" && selection?.active === true) {
            event.preventDefault();
            selection.onExit();
            return;
          }
          if (
            event.target instanceof HTMLElement &&
            event.target.closest("button, input, select, textarea, [role='separator']")
          ) {
            return;
          }
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
          } else if (event.key === "Enter") {
            const row = sortedRows[focusIndex];
            if (row !== undefined) {
              event.preventDefault();
              if (selection?.active === true) {
                toggleRowSelection(row, getRowId(row));
              } else onSelect?.(row);
            }
          } else if (event.key === " ") {
            const row = sortedRows[focusIndex];
            if (row === undefined) return;
            if (selection !== undefined) {
              event.preventDefault();
              const rowId = getRowId(row);
              if (event.shiftKey) replaceSelectionRange(row, rowId);
              else if (selection.active) toggleRowSelection(row, rowId);
              else enterRowSelection(row, rowId);
            } else if (onSelect !== undefined) {
              event.preventDefault();
              onSelect(row);
            }
          }
        }}
        data-testid="virtual-table"
      >
      <div className={styles.header} role="row" style={gridStyle}>
        {selection === undefined ? null : (
          <div
            className={styles.selectionHeaderCell}
            role="columnheader"
            aria-label="Selection"
            onClick={(event) => {
              event.stopPropagation();
              if (event.target === event.currentTarget) {
                selectionAnchorIdRef.current =
                  sortedRows.length === 0 ? null : getRowId(sortedRows[0]!);
                selection.onSetAll(sortedRows, !allVisibleSelected);
              }
            }}
          >
            <SelectionCheckbox
              checked={allVisibleSelected}
              indeterminate={selectedVisibleCount > 0 && !allVisibleSelected}
              label={
                allVisibleSelected ? "Deselect all rows" : "Select all rows"
              }
              onChange={() => {
                selectionAnchorIdRef.current =
                  sortedRows.length === 0 ? null : getRowId(sortedRows[0]!);
                selection.onSetAll(sortedRows, !allVisibleSelected);
              }}
            />
          </div>
        )}
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
                className={styles.sortButton}
                type="button"
                onClick={() => changeSort(column)}
                disabled={column.sortable === false || column.sortValue === undefined}
              >
                <span>{column.label}</span>
                {sorted ? <span aria-hidden="true">{sort.direction === "asc" ? "▲" : "▼"}</span> : null}
              </button>
              {column.headerHelp !== undefined ? (
                <button
                  className={styles.headerHelpButton}
                  type="button"
                  aria-label={`Explain ${column.label}`}
                  aria-haspopup="dialog"
                  aria-expanded={headerHelp?.columnId === column.id}
                  aria-controls={
                    headerHelp?.columnId === column.id
                      ? `${headerHelpIdPrefix}-${column.id}-help`
                      : undefined
                  }
                  onPointerDown={(event) => event.stopPropagation()}
                  onClick={(event) =>
                    toggleHeaderHelp(event.currentTarget, column)
                  }
                >
                  ?
                </button>
              ) : null}
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
        <div className={styles.empty} onClick={activateBackground}>
          {emptyMessage}
        </div>
      ) : (
        <div
          className={styles.canvas}
          style={{
            height: `${sortedRows.length * tableRowHeight}px`,
            minWidth: `${minimumWidth}px`,
          }}
          onClick={(event) => {
            if (event.target === event.currentTarget) activateBackground();
          }}
        >
          {renderedRows.map((row, offset) => {
            const index = firstIndex + offset;
            const rowId = getRowId(row);
            const checked = selection?.selectedIds.has(rowId) ?? false;
            const highlighted = selectionActive
              ? checked
              : rowId === selectedId;
            return (
              <div
                key={rowId}
                className={styles.row}
                role="row"
                aria-rowindex={index + 2}
                aria-selected={selectionAvailable ? checked : highlighted}
                aria-current={rowId === selectedId ? "true" : undefined}
                tabIndex={index === focusIndex ? 0 : -1}
                data-row-index={index}
                data-row-id={rowId}
                data-selected={highlighted}
                data-current={rowId === selectedId}
                style={{
                  ...gridStyle,
                  transform: `translateY(${index * tableRowHeight}px)`,
                }}
                onFocus={() => setFocusIndex(index)}
                onPointerDown={(event) => startLongPress(event, row, rowId)}
                onPointerMove={continueLongPress}
                onPointerUp={(event) => cancelLongPress(event.pointerId)}
                onPointerCancel={(event) => {
                  cancelLongPress(event.pointerId);
                  if (suppressClickRowRef.current === rowId) {
                    suppressClickRowRef.current = null;
                  }
                }}
                onPointerLeave={(event) => cancelLongPress(event.pointerId)}
                onClick={(event) => {
                  setFocusIndex(index);
                  activateRow(event, row, rowId);
                }}
              >
                {selection === undefined ? null : (
                  <div
                    className={styles.selectionCell}
                    role="gridcell"
                    onClick={(event) => {
                      event.stopPropagation();
                      if (event.target === event.currentTarget) {
                        if (event.shiftKey) replaceSelectionRange(row, rowId);
                        else toggleRowSelection(row, rowId);
                      }
                    }}
                  >
                    <SelectionCheckbox
                      checked={checked}
                      indeterminate={false}
                      label={`${checked ? "Deselect" : "Select"} ${selection.getRowLabel(row)}`}
                      onChange={() => toggleRowSelection(row, rowId)}
                      onShiftChange={() => replaceSelectionRange(row, rowId)}
                    />
                  </div>
                )}
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
    {headerHelp !== null && headerHelpColumn?.headerHelp !== undefined
      ? createPortal(
          <div
            className={styles.headerHelpLayer}
            onPointerDown={() => setHeaderHelp(null)}
          >
            <div
              ref={headerHelpPopoverRef}
              id={`${headerHelpIdPrefix}-${headerHelpColumn.id}-help`}
              className={styles.headerHelpPopover}
              role="dialog"
              aria-label={`${headerHelpColumn.label} column help`}
              tabIndex={0}
              style={{
                left: headerHelp.left,
                top: headerHelp.top,
                width: headerHelp.width,
                maxHeight: headerHelp.maximumHeight,
              }}
              onPointerDown={(event) => event.stopPropagation()}
            >
              {headerHelpColumn.headerHelp}
            </div>
          </div>,
          document.body,
        )
      : null}
    </>
  );
}

function SelectionCheckbox({
  checked,
  indeterminate,
  label,
  onChange,
  onShiftChange,
}: {
  readonly checked: boolean;
  readonly indeterminate: boolean;
  readonly label: string;
  readonly onChange: () => void;
  readonly onShiftChange?: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (inputRef.current !== null) {
      inputRef.current.indeterminate = indeterminate;
    }
  }, [indeterminate]);
  return (
    <input
      ref={inputRef}
      className={styles.selectionCheckbox}
      type="checkbox"
      checked={checked}
      aria-label={label}
      onPointerDown={(event) => event.stopPropagation()}
      onClick={(event) => event.stopPropagation()}
      onChange={(event) => {
        if (
          event.nativeEvent instanceof MouseEvent &&
          event.nativeEvent.shiftKey &&
          onShiftChange !== undefined
        ) {
          onShiftChange();
        } else onChange();
      }}
    />
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
