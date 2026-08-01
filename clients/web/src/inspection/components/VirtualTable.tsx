import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type UIEvent,
} from "react";

import styles from "./VirtualTable.module.css";

export interface VirtualColumn<Row> {
  readonly id: string;
  readonly label: string;
  readonly width: number;
  readonly minimumViewport?: number;
  readonly align?: "left" | "right" | "center";
  readonly sortable?: boolean;
  readonly sortValue?: (row: Row) => string | number | null;
  readonly render: (row: Row) => ReactNode;
}

export interface VirtualTableProps<Row> {
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

export function VirtualTable<Row>({
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
      columns.filter(
        (column) =>
          column.minimumViewport === undefined ||
          viewportSize.width >= column.minimumViewport,
      ),
    [columns, viewportSize.width],
  );
  const sortedRows = useMemo(() => {
    if (sort === null) return rows;
    const column = columns.find((candidate) => candidate.id === sort.columnId);
    if (column?.sortValue === undefined) return rows;
    const direction = sort.direction === "asc" ? 1 : -1;
    return [...rows].sort((left, right) =>
      compare(column.sortValue!(left), column.sortValue!(right)) * direction,
    );
  }, [columns, rows, sort]);

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
    setSort((current) => ({
      columnId: column.id,
      direction:
        current?.columnId === column.id && current.direction === "asc"
          ? "desc"
          : "asc",
    }));
  };

  const moveFocus = (nextIndex: number) => {
    if (sortedRows.length === 0) return;
    const clamped = Math.max(0, Math.min(sortedRows.length - 1, nextIndex));
    setFocusIndex(clamped);
    const viewport = viewportRef.current;
    if (viewport !== null) {
      const top = HEADER_HEIGHT + clamped * rowHeight;
      if (top < viewport.scrollTop + HEADER_HEIGHT) {
        viewport.scrollTop = top - HEADER_HEIGHT;
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

  return (
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
  );
}

function compare(
  left: string | number | null,
  right: string | number | null,
): number {
  if (left === right) return 0;
  if (left === null) return 1;
  if (right === null) return -1;
  if (typeof left === "number" && typeof right === "number") {
    return left - right;
  }
  return String(left).localeCompare(String(right), undefined, {
    numeric: true,
    sensitivity: "base",
  });
}
