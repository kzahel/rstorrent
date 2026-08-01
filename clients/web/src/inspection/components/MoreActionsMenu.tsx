import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";

import {
  WEBTORRENT_TEST_TORRENTS,
  type TestTorrentShortcut,
} from "../testTorrents";
import styles from "./MoreActionsMenu.module.css";

export interface MoreActionsMenuProps {
  readonly disabled: boolean;
  readonly onAddTestTorrent: (torrent: TestTorrentShortcut) => Promise<void>;
}

export function MoreActionsMenu({
  disabled,
  onAddTestTorrent,
}: MoreActionsMenuProps) {
  const [open, setOpen] = useState(false);
  const [submenuOpen, setSubmenuOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const rootMenuRef = useRef<HTMLDivElement>(null);
  const submenuRef = useRef<HTMLDivElement>(null);
  const submenuTriggerRef = useRef<HTMLButtonElement>(null);

  const closeMenu = (restoreFocus: boolean) => {
    setOpen(false);
    setSubmenuOpen(false);
    if (restoreFocus) queueMicrotask(() => triggerRef.current?.focus());
  };

  useEffect(() => {
    if (!open) return;
    focusFirst(rootMenuRef.current, "rootMenuItem");

    const handlePointerDown = (event: PointerEvent) => {
      if (
        event.target instanceof Node &&
        !containerRef.current?.contains(event.target)
      ) {
        closeMenu(false);
      }
    };
    const handleKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Tab") {
        closeMenu(false);
      } else if (event.key === "Escape") {
        event.preventDefault();
        if (submenuOpen) {
          setSubmenuOpen(false);
          queueMicrotask(() => submenuTriggerRef.current?.focus());
        } else {
          closeMenu(true);
        }
      }
    };
    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [open, submenuOpen]);

  useEffect(() => {
    if (submenuOpen) focusFirst(submenuRef.current, "submenuItem");
  }, [submenuOpen]);

  useEffect(() => {
    if (disabled && open) closeMenu(false);
  }, [disabled, open]);

  const handleRootKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!(event.target instanceof HTMLElement)) return;
    if (event.target.dataset.rootMenuItem === undefined) return;
    if (moveWithinMenu(event, rootMenuRef.current, "rootMenuItem")) return;
    if (event.key === "ArrowRight") {
      event.preventDefault();
      setSubmenuOpen(true);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      closeMenu(true);
    }
  };

  const handleSubmenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (moveWithinMenu(event, submenuRef.current, "submenuItem")) return;
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      setSubmenuOpen(false);
      queueMicrotask(() => submenuTriggerRef.current?.focus());
    }
  };

  const selectTestTorrent = async (torrent: TestTorrentShortcut) => {
    closeMenu(false);
    await onAddTestTorrent(torrent);
    globalThis.setTimeout(() => triggerRef.current?.focus(), 0);
  };

  return (
    <div className={styles.container} ref={containerRef}>
      <button
        ref={triggerRef}
        className={styles.trigger}
        type="button"
        disabled={disabled}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => {
          setOpen((current) => !current);
          setSubmenuOpen(false);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        More <span aria-hidden="true">▾</span>
      </button>
      {open ? (
        <div
          ref={rootMenuRef}
          className={styles.menu}
          role="menu"
          aria-label="More actions"
          onKeyDown={handleRootKeyDown}
        >
          <div
            className={styles.submenuOwner}
            onMouseEnter={() => setSubmenuOpen(true)}
          >
            <button
              ref={submenuTriggerRef}
              className={styles.menuItem}
              type="button"
              role="menuitem"
              tabIndex={-1}
              data-root-menu-item
              aria-haspopup="menu"
              aria-expanded={submenuOpen}
              onClick={() => setSubmenuOpen(true)}
            >
              <span aria-hidden="true">＋</span>
              <span>Add test torrent</span>
              <span className={styles.submenuArrow} aria-hidden="true">
                ›
              </span>
            </button>
            {submenuOpen ? (
              <div
                ref={submenuRef}
                className={styles.submenu}
                role="menu"
                aria-label="Add test torrent"
                onKeyDown={handleSubmenuKeyDown}
              >
                {WEBTORRENT_TEST_TORRENTS.map((torrent) => (
                  <button
                    key={torrent.id}
                    className={styles.menuItem}
                    type="button"
                    role="menuitem"
                    tabIndex={-1}
                    data-submenu-item
                    onClick={() => void selectTestTorrent(torrent)}
                  >
                    {torrent.menuLabel}
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}

function focusFirst(container: HTMLElement | null, dataName: string): void {
  menuItems(container, dataName)[0]?.focus();
}

function moveWithinMenu(
  event: KeyboardEvent<HTMLElement>,
  container: HTMLElement | null,
  dataName: string,
): boolean {
  const items = menuItems(container, dataName);
  if (items.length === 0) return false;
  const current = items.indexOf(document.activeElement as HTMLElement);
  let next: number | null = null;
  if (event.key === "ArrowDown") next = (current + 1) % items.length;
  if (event.key === "ArrowUp") next = (current - 1 + items.length) % items.length;
  if (event.key === "Home") next = 0;
  if (event.key === "End") next = items.length - 1;
  if (next === null) return false;
  event.preventDefault();
  items[next]?.focus();
  return true;
}

function menuItems(
  container: HTMLElement | null,
  dataName: string,
): HTMLElement[] {
  if (container === null) return [];
  const attribute = dataName.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`);
  return Array.from(
    container.querySelectorAll<HTMLElement>(`[data-${attribute}]`),
  );
}
