import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
} from "react";

import { applyAppearancePreferences } from "../appearance";
import { useInspectionCommand, useInspectionStore } from "../context";
import { formatRate } from "../format";
import type { ApplicationDestination } from "../model";
import {
  MAX_DETAIL_PANE_PERCENT,
  MIN_DETAIL_PANE_PERCENT,
} from "../state";
import { DetailPane } from "./DetailPane";
import { Icon, type IconName } from "./Icon";
import { LibraryView } from "./LibraryView";
import { ScenarioBar } from "./ScenarioBar";
import { SettingsDialog } from "./SettingsDialog";
import { Sidebar } from "./Sidebar";
import { TorrentActions } from "./TorrentActions";
import { TorrentTable } from "./TorrentTable";
import { TransfersView } from "./TransfersView";
import styles from "./App.module.css";

const DESTINATIONS: readonly {
  readonly id: ApplicationDestination;
  readonly label: string;
  readonly icon: IconName;
}[] = [
  { id: "library", label: "Library", icon: "library" },
  { id: "transfers", label: "Transfers", icon: "transfers" },
  { id: "workbench", label: "Workbench", icon: "workbench" },
];

export function App() {
  const session = useInspectionStore((state) => state.session);
  const demo = useInspectionStore((state) => state.demo);
  const storage = useInspectionStore((state) => state.storage);
  const execute = useInspectionCommand();
  const destination = useInspectionStore(
    (state) => state.presentation.destination,
  );
  const detailOpen = useInspectionStore(
    (state) => state.presentation.detailOpen,
  );
  const sidebarOpen = useInspectionStore(
    (state) => state.presentation.sidebarOpen,
  );
  const detailPanePercent = useInspectionStore(
    (state) => state.presentation.detailPanePercent,
  );
  const interfaceSize = useInspectionStore(
    (state) => state.presentation.interfaceSize,
  );
  const colorTheme = useInspectionStore(
    (state) => state.presentation.colorTheme,
  );
  const selectDestination = useInspectionStore(
    (state) => state.selectDestination,
  );
  const toggleSidebar = useInspectionStore((state) => state.toggleSidebar);
  const closeSidebar = useInspectionStore((state) => state.closeSidebar);
  const setLayout = useInspectionStore((state) => state.setLayout);
  const setDetailPanePercent = useInspectionStore(
    (state) => state.setDetailPanePercent,
  );
  const setInterfaceSize = useInspectionStore(
    (state) => state.setInterfaceSize,
  );
  const setColorTheme = useInspectionStore((state) => state.setColorTheme);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [resizingDetail, setResizingDetail] = useState(false);
  const settingsButtonRef = useRef<HTMLButtonElement>(null);
  const mainRef = useRef<HTMLElement>(null);
  const splitterRef = useRef<HTMLDivElement>(null);
  const activeSplitterPointer = useRef<number | null>(null);

  useLayoutEffect(() => {
    applyAppearancePreferences({ colorTheme, interfaceSize });
  }, [colorTheme, interfaceSize]);

  useEffect(() => {
    const update = () => {
      setLayout(
        window.innerWidth < 680
          ? "phone"
          : window.innerWidth < 1_100
            ? "compact"
            : "wide",
      );
    };
    update();
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, [setLayout]);

  const resizeDetailFromPointer = (clientY: number) => {
    const mainBounds = mainRef.current?.getBoundingClientRect();
    const splitterBounds = splitterRef.current?.getBoundingClientRect();
    if (mainBounds === undefined || splitterBounds === undefined) return;
    const availableHeight = mainBounds.height - splitterBounds.height;
    if (availableHeight <= 0) return;
    const detailHeight =
      mainBounds.bottom - clientY - splitterBounds.height / 2;
    setDetailPanePercent((detailHeight / availableHeight) * 100);
  };

  const startDetailResize = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    activeSplitterPointer.current = event.pointerId;
    event.currentTarget.setPointerCapture?.(event.pointerId);
    setResizingDetail(true);
    resizeDetailFromPointer(event.clientY);
  };

  const continueDetailResize = (event: PointerEvent<HTMLDivElement>) => {
    if (activeSplitterPointer.current !== event.pointerId) return;
    resizeDetailFromPointer(event.clientY);
  };

  const stopDetailResize = (event: PointerEvent<HTMLDivElement>) => {
    if (activeSplitterPointer.current !== event.pointerId) return;
    activeSplitterPointer.current = null;
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    setResizingDetail(false);
  };

  const resizeDetailWithKeyboard = (event: KeyboardEvent<HTMLDivElement>) => {
    let nextPercent: number | undefined;
    switch (event.key) {
      case "ArrowUp":
        nextPercent = detailPanePercent + 5;
        break;
      case "ArrowDown":
        nextPercent = detailPanePercent - 5;
        break;
      case "Home":
        nextPercent = MIN_DETAIL_PANE_PERCENT;
        break;
      case "End":
        nextPercent = MAX_DETAIL_PANE_PERCENT;
        break;
    }
    if (nextPercent === undefined) return;
    event.preventDefault();
    setDetailPanePercent(nextPercent);
  };

  const paneGridStyle = {
    "--collection-pane-share": `${100 - detailPanePercent}fr`,
    "--detail-pane-share": `${detailPanePercent}fr`,
  } as CSSProperties;

  return (
    <div
      className={styles.app}
      data-destination={destination}
      data-detail-open={detailOpen}
      data-interface-size={interfaceSize}
      data-sidebar-open={sidebarOpen}
    >
      <header className={styles.header}>
        <button
          className={styles.menuButton}
          type="button"
          aria-label={`Toggle ${destinationLabel(destination)} filters`}
          aria-expanded={sidebarOpen}
          onClick={toggleSidebar}
        >
          <Icon name="menu" />
        </button>
        <div className={styles.brand}>
          <span aria-hidden="true">RS</span>
          <strong>RSTorrent</strong>
        </div>
        <nav className={styles.primaryNavigation} aria-label="Primary">
          {DESTINATIONS.map((item) => (
            <button
              key={item.id}
              type="button"
              aria-current={destination === item.id ? "page" : undefined}
              onClick={() => selectDestination(item.id)}
            >
              <Icon name={item.icon} />
              <span>{item.label}</span>
            </button>
          ))}
        </nav>
        <div className={styles.sessionStats} aria-label="Session transfer rates">
          <span><b aria-hidden="true">↓</b> {formatRate(session.downloadRate)}</span>
          <span><b aria-hidden="true">↑</b> {formatRate(session.uploadRate)}</span>
        </div>
        <div className={styles.connection} data-state={session.connection}>
          <span aria-hidden="true" />
          {session.connection === "demo" ? "Demo adapter" : session.connection}
        </div>
        <button
          ref={settingsButtonRef}
          className={styles.settingsButton}
          type="button"
          aria-label="Settings"
          aria-haspopup="dialog"
          aria-expanded={settingsOpen}
          title="Settings"
          onClick={() => setSettingsOpen(true)}
        >
          <Icon name="settings" />
        </button>
      </header>
      <ScenarioBar />
      <div className={styles.workspace}>
        <div className={styles.sidebarWrap}>
          <Sidebar />
        </div>
        <button
          className={styles.scrim}
          type="button"
          aria-label={`Close ${destinationLabel(destination)} filters`}
          onClick={closeSidebar}
        />
        <main
          ref={mainRef}
          className={styles.main}
          data-destination={destination}
          data-resizing={resizingDetail}
          style={paneGridStyle}
        >
          {destination === "library" ? (
            <LibraryView />
          ) : destination === "transfers" ? (
            <TransfersView />
          ) : (
            <>
              <section
                className={styles.collection}
                aria-label="Workbench torrent collection"
              >
                <TorrentActions />
                <div className={styles.tableWrap}>
                  <TorrentTable />
                </div>
              </section>
              <div
                ref={splitterRef}
                className={styles.splitter}
                role="separator"
                aria-label="Resize torrent details"
                aria-orientation="horizontal"
                aria-valuemin={MIN_DETAIL_PANE_PERCENT}
                aria-valuemax={MAX_DETAIL_PANE_PERCENT}
                aria-valuenow={detailPanePercent}
                aria-valuetext={`${detailPanePercent}% height for torrent details`}
                data-resizing={resizingDetail}
                tabIndex={0}
                title="Drag to resize; use Up and Down arrow keys for precise control"
                onPointerDown={startDetailResize}
                onPointerMove={continueDetailResize}
                onPointerUp={stopDetailResize}
                onPointerCancel={stopDetailResize}
                onLostPointerCapture={(event) => {
                  if (activeSplitterPointer.current === event.pointerId) {
                    activeSplitterPointer.current = null;
                    setResizingDetail(false);
                  }
                }}
                onKeyDown={resizeDetailWithKeyboard}
              >
                <span aria-hidden="true" />
              </div>
              <DetailPane />
            </>
          )}
        </main>
      </div>
      {settingsOpen ? (
        <SettingsDialog
          colorTheme={colorTheme}
          interfaceSize={interfaceSize}
          storage={storage}
          downloadsManageable={demo === null}
          returnFocus={settingsButtonRef}
          onColorThemeChange={setColorTheme}
          onInterfaceSizeChange={setInterfaceSize}
          onChooseFolder={async (repairRoot) => {
            const result = await execute({
              type: "choose_download_root",
              ...(repairRoot === undefined ? {} : { repairRoot }),
            });
            return result.storageRoot ?? null;
          }}
          onDefaultRootChange={async (rootId) => {
            await execute({ type: "set_default_download_root", rootId });
          }}
          onShowAddOptionsChange={async (show) => {
            await execute({ type: "set_show_add_options", show });
          }}
          onRemoveRoot={async (rootId) => {
            await execute({ type: "remove_download_root", rootId });
          }}
          onClose={() => setSettingsOpen(false)}
        />
      ) : null}
    </div>
  );
}

function destinationLabel(destination: ApplicationDestination): string {
  return destination.slice(0, 1).toUpperCase() + destination.slice(1);
}
