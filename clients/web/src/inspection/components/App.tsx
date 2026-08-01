import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
  type KeyboardEvent,
  type PointerEvent,
} from "react";

import { useInspectionDispatch, useInspectionStore } from "../context";
import { formatRate } from "../format";
import type { TorrentRow } from "../model";
import {
  MAX_DETAIL_PANE_PERCENT,
  MIN_DETAIL_PANE_PERCENT,
} from "../state";
import type { TestTorrentShortcut } from "../testTorrents";
import { validateTorrentInput } from "../torrentInput";
import { DetailPane } from "./DetailPane";
import { MoreActionsMenu } from "./MoreActionsMenu";
import { RemoveTorrentDialog } from "./RemoveTorrentDialog";
import { ScenarioBar } from "./ScenarioBar";
import { Sidebar } from "./Sidebar";
import { TorrentTable } from "./TorrentTable";
import styles from "./App.module.css";

export function App() {
  const session = useInspectionStore((state) => state.session);
  const demo = useInspectionStore((state) => state.demo);
  const selected = useInspectionStore((state) =>
    state.presentation.selectedTorrentId === null
      ? undefined
      : state.torrents[state.presentation.selectedTorrentId],
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
  const toggleSidebar = useInspectionStore((state) => state.toggleSidebar);
  const closeSidebar = useInspectionStore((state) => state.closeSidebar);
  const setLayout = useInspectionStore((state) => state.setLayout);
  const setDetailPanePercent = useInspectionStore(
    (state) => state.setDetailPanePercent,
  );
  const dispatch = useInspectionDispatch();
  const [status, setStatus] = useState("");
  const [torrentInput, setTorrentInput] = useState("");
  const [inputInvalid, setInputInvalid] = useState(false);
  const [adding, setAdding] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<TorrentRow | undefined>();
  const [resizingDetail, setResizingDetail] = useState(false);
  const addingRef = useRef(false);
  const removeButtonRef = useRef<HTMLButtonElement>(null);
  const mainRef = useRef<HTMLElement>(null);
  const splitterRef = useRef<HTMLDivElement>(null);
  const activeSplitterPointer = useRef<number | null>(null);

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

  const send = async (
    command: Parameters<typeof dispatch>[0],
  ): Promise<boolean> => {
    try {
      setStatus(await dispatch(command));
      return true;
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
      return false;
    }
  };

  const addMagnet = async (source: string, clearInputOnSuccess: boolean) => {
    if (addingRef.current) return false;
    const validated = validateTorrentInput(source);
    if (!validated.accepted) {
      setInputInvalid(true);
      setStatus(validated.message);
      return false;
    }
    setInputInvalid(false);
    addingRef.current = true;
    setAdding(true);
    try {
      const accepted = await send({
        type: "add_magnet",
        magnet: validated.magnet,
      });
      if (accepted && clearInputOnSuccess) {
        setTorrentInput("");
      }
      return accepted;
    } finally {
      addingRef.current = false;
      setAdding(false);
    }
  };

  const addTorrent = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    await addMagnet(torrentInput, true);
  };

  const addTestTorrent = async (torrent: TestTorrentShortcut) => {
    if (await addMagnet(torrent.magnet, false)) {
      setStatus(`${torrent.menuLabel} added`);
    }
  };

  const removeTorrent = async (deleteData: boolean) => {
    if (removeTarget === undefined) return;
    try {
      const message = await dispatch({
        type: "remove",
        torrentId: removeTarget.id,
        deleteData,
      });
      setStatus(message);
      setRemoveTarget(undefined);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
      throw error;
    }
  };

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
    <>
      <div
        className={styles.app}
        data-detail-open={detailOpen}
        data-sidebar-open={sidebarOpen}
      >
      <header className={styles.header}>
        <button
          className={styles.menuButton}
          type="button"
          aria-label="Toggle library navigation"
          aria-expanded={sidebarOpen}
          onClick={toggleSidebar}
        >
          <span aria-hidden="true">☰</span>
        </button>
        <div className={styles.brand}>
          <span aria-hidden="true">RS</span>
          <strong>RSTorrent</strong>
          <small>Inspection</small>
        </div>
        <div className={styles.sessionStats} aria-label="Session transfer rates">
          <span><b aria-hidden="true">↓</b> {formatRate(session.downloadRate)}</span>
          <span><b aria-hidden="true">↑</b> {formatRate(session.uploadRate)}</span>
          <span className={styles.peerTotal}>
            {session.knownPeers === null
              ? "Peers unavailable"
              : `${session.knownPeers.toLocaleString()} peers`}
          </span>
        </div>
        <div className={styles.connection} data-state={session.connection}>
          <span aria-hidden="true" />
          {session.connection === "demo" ? "Demo adapter" : session.connection}
        </div>
      </header>
      <ScenarioBar />
      <div className={styles.workspace}>
        <div className={styles.sidebarWrap}>
          <Sidebar />
        </div>
        <button
          className={styles.scrim}
          type="button"
          aria-label="Close library navigation"
          onClick={closeSidebar}
        />
        <main
          ref={mainRef}
          className={styles.main}
          data-resizing={resizingDetail}
          style={paneGridStyle}
        >
          <section className={styles.collection} aria-label="Torrent collection">
            <div className={styles.toolbar}>
              {demo === null ? (
                <form
                  className={styles.addForm}
                  aria-label="Add torrent"
                  onSubmit={(event) => void addTorrent(event)}
                >
                  <input
                    className={styles.addInput}
                    type="text"
                    value={torrentInput}
                    aria-label="Magnet link or torrent URL"
                    aria-describedby="command-status"
                    aria-invalid={inputInvalid}
                    autoComplete="off"
                    spellCheck={false}
                    placeholder="Magnet link or URL"
                    onChange={(event) => {
                      setTorrentInput(event.currentTarget.value);
                      setInputInvalid(false);
                    }}
                  />
                  <button
                    className={styles.addButton}
                    type="submit"
                    disabled={adding}
                  >
                    {adding ? "Adding…" : "Add"}
                  </button>
                </form>
              ) : (
                <>
                  <button type="button" className={styles.primaryAction} onClick={() => void send({ type: "add_demo_torrent" })}>
                    <span aria-hidden="true">＋</span> Add demo
                  </button>
                  <span className={styles.divider} aria-hidden="true" />
                </>
              )}
              <button
                type="button"
                disabled={
                  selected === undefined ||
                  selected.removalState !== null ||
                  selected.status === "downloading" ||
                  selected.status === "metadata"
                }
                onClick={() => selected === undefined ? undefined : void send({ type: "resume", torrentId: selected.id })}
              >
                <span aria-hidden="true">▶</span> Start
              </button>
              <button
                type="button"
                disabled={
                  selected === undefined ||
                  selected.removalState !== null ||
                  selected.status === "paused" ||
                  selected.status === "complete"
                }
                onClick={() => selected === undefined ? undefined : void send({ type: "pause", torrentId: selected.id })}
              >
                <span aria-hidden="true">Ⅱ</span> Pause
              </button>
              {demo === null ? (
                <MoreActionsMenu
                  disabled={adding}
                  onAddTestTorrent={addTestTorrent}
                />
              ) : null}
              <button
                type="button"
                disabled={
                  selected === undefined ||
                  selected.archived === null ||
                  selected.removalState !== null
                }
                onClick={() =>
                  selected === undefined || selected.archived === null
                    ? undefined
                    : void send({
                        type: selected.archived ? "unarchive" : "archive",
                        torrentId: selected.id,
                      })
                }
              >
                <span aria-hidden="true">□</span> {selected?.archived ? "Restore" : "Archive"}
              </button>
              <button
                ref={removeButtonRef}
                type="button"
                disabled={
                  selected === undefined ||
                  (selected.removalState !== null && selected.removalState !== "failed")
                }
                onClick={() => setRemoveTarget(selected)}
              >
                <span aria-hidden="true">×</span> Remove
              </button>
              <output
                id="command-status"
                className={styles.commandStatus}
                aria-live="polite"
              >
                {status}
              </output>
            </div>
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
        </main>
      </div>
      </div>
      {removeTarget === undefined ? null : (
        <RemoveTorrentDialog
          torrentName={removeTarget.name}
          deleteDataSupported={removeTarget.deleteManagedDataSupported}
          returnFocus={removeButtonRef}
          onCancel={() => setRemoveTarget(undefined)}
          onConfirm={removeTorrent}
        />
      )}
    </>
  );
}
