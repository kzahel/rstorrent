import { useEffect, useId, useRef, useState } from "react";

import { Icon } from "./Icon";
import styles from "./FileActionsMenu.module.css";

export function FileActionsMenu({
  targetCount,
  pending,
  unavailableReason,
  onPriority,
}: {
  readonly targetCount: number;
  readonly pending: boolean;
  readonly unavailableReason?: string;
  readonly onPriority: (priority: "normal" | "skip") => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const reason =
    targetCount === 0
      ? "Select a file to use these actions."
      : unavailableReason;
  const reasonId = useId();
  const disabled = pending || reason !== undefined;

  const close = (restoreFocus: boolean) => {
    setOpen(false);
    if (restoreFocus) queueMicrotask(() => triggerRef.current?.focus());
  };

  useEffect(() => {
    if (!open) return;
    menuRef.current?.focus();
    const handlePointerDown = (event: PointerEvent) => {
      if (
        event.target instanceof Node &&
        !containerRef.current?.contains(event.target)
      ) {
        close(false);
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close(true);
      } else if (event.key === "Tab") {
        close(false);
      }
    };
    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [open]);

  return (
    <div ref={containerRef} className={styles.container}>
      <button
        ref={triggerRef}
        className={styles.trigger}
        type="button"
        aria-label="More file actions"
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={pending}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown") return;
          event.preventDefault();
          setOpen(true);
        }}
      >
        More <Icon name="chevronDown" />
      </button>
      {open ? (
        <div
          ref={menuRef}
          className={styles.menu}
          role="menu"
          aria-label="File actions"
          aria-describedby={reason === undefined ? undefined : reasonId}
          tabIndex={-1}
        >
          <button
            type="button"
            role="menuitem"
            disabled={disabled}
            onClick={() => {
              close(false);
              void onPriority("normal");
            }}
          >
            Normal
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={disabled}
            onClick={() => {
              close(false);
              void onPriority("skip");
            }}
          >
            Skip
          </button>
          {reason === undefined ? null : <p id={reasonId}>{reason}</p>}
        </div>
      ) : null}
    </div>
  );
}
