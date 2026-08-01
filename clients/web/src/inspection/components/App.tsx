import { useState } from "react";

import { useInspectionDispatch, useInspectionStore } from "../context";
import { formatRate } from "../format";
import { DetailPane } from "./DetailPane";
import { ScenarioBar } from "./ScenarioBar";
import { Sidebar } from "./Sidebar";
import { TorrentTable } from "./TorrentTable";
import styles from "./App.module.css";

export function App() {
  const session = useInspectionStore((state) => state.session);
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
  const toggleSidebar = useInspectionStore((state) => state.toggleSidebar);
  const closeSidebar = useInspectionStore((state) => state.closeSidebar);
  const dispatch = useInspectionDispatch();
  const [status, setStatus] = useState("");

  const send = async (command: Parameters<typeof dispatch>[0]) => {
    try {
      setStatus(await dispatch(command));
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    }
  };

  return (
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
          <span className={styles.peerTotal}>{session.knownPeers.toLocaleString()} peers</span>
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
        <main className={styles.main}>
          <section className={styles.collection} aria-label="Torrent collection">
            <div className={styles.toolbar}>
              <button type="button" className={styles.primaryAction} onClick={() => void send({ type: "add_demo_torrent" })}>
                <span aria-hidden="true">＋</span> Add demo
              </button>
              <span className={styles.divider} aria-hidden="true" />
              <button
                type="button"
                disabled={selected === undefined || selected.status === "downloading" || selected.status === "metadata"}
                onClick={() => selected === undefined ? undefined : void send({ type: "resume", torrentId: selected.id })}
              >
                <span aria-hidden="true">▶</span> Start
              </button>
              <button
                type="button"
                disabled={selected === undefined || selected.status === "paused" || selected.status === "complete"}
                onClick={() => selected === undefined ? undefined : void send({ type: "pause", torrentId: selected.id })}
              >
                <span aria-hidden="true">Ⅱ</span> Pause
              </button>
              <button
                type="button"
                disabled={selected === undefined}
                onClick={() =>
                  selected === undefined
                    ? undefined
                    : void send({
                        type: selected.archived ? "unarchive" : "archive",
                        torrentId: selected.id,
                      })
                }
              >
                <span aria-hidden="true">□</span> {selected?.archived ? "Restore" : "Archive"}
              </button>
              <output className={styles.commandStatus} aria-live="polite">{status}</output>
            </div>
            <div className={styles.tableWrap}>
              <TorrentTable />
            </div>
          </section>
          <div className={styles.splitter} aria-hidden="true"><span /></div>
          <DetailPane />
        </main>
      </div>
    </div>
  );
}
