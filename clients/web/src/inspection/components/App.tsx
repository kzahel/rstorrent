import { useEffect, useState, type FormEvent } from "react";

import { useInspectionDispatch, useInspectionStore } from "../context";
import { formatRate } from "../format";
import { validateTorrentInput } from "../torrentInput";
import { DetailPane } from "./DetailPane";
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
  const toggleSidebar = useInspectionStore((state) => state.toggleSidebar);
  const closeSidebar = useInspectionStore((state) => state.closeSidebar);
  const setLayout = useInspectionStore((state) => state.setLayout);
  const dispatch = useInspectionDispatch();
  const [status, setStatus] = useState("");
  const [torrentInput, setTorrentInput] = useState("");
  const [inputInvalid, setInputInvalid] = useState(false);
  const [adding, setAdding] = useState(false);

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

  const addTorrent = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (adding) return;
    const validated = validateTorrentInput(torrentInput);
    if (!validated.accepted) {
      setInputInvalid(true);
      setStatus(validated.message);
      return;
    }
    setInputInvalid(false);
    setAdding(true);
    try {
      if (await send({ type: "add_magnet", magnet: validated.magnet })) {
        setTorrentInput("");
      }
    } finally {
      setAdding(false);
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
        <main className={styles.main}>
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
              {demo === null ? null : (
                <button
                  type="button"
                  disabled={selected === undefined || selected.archived === null}
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
              )}
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
          <div className={styles.splitter} aria-hidden="true"><span /></div>
          <DetailPane />
        </main>
      </div>
    </div>
  );
}
