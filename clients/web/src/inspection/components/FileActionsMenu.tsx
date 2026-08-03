import { useEffect, useRef, useState } from "react";

import { Icon } from "./Icon";
import styles from "./FileActionsMenu.module.css";

export function FileActionsMenu({
  targetCount,
}: {
  readonly targetCount: number;
}) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const unavailableReason =
    targetCount === 0
      ? "Select a file to use these actions."
      : "File actions are not available yet.";

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
          aria-describedby="file-actions-unavailable"
          tabIndex={-1}
        >
          <button type="button" role="menuitem" disabled>
            Download
          </button>
          <button type="button" role="menuitem" disabled>
            Skip download
          </button>
          <p id="file-actions-unavailable">{unavailableReason}</p>
        </div>
      ) : null}
    </div>
  );
}
