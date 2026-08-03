import { useEffect, useRef } from "react";

import { useInspectionStore } from "../context";
import {
  formatBytes,
  formatProgress,
  formatRate,
} from "../format";
import type { DetailTab } from "../model";
import { DETAIL_TABS } from "../tabs";
import { PeerTable } from "./PeerTable";
import { SwarmTable } from "./SwarmTable";
import { FileTable } from "./FileTable";
import { TrackerTable } from "./TrackerTable";
import { DiskPanel } from "./DiskPanel";
import { PieceMapPanel } from "./PieceMapPanel";
import { LogConsole } from "./LogConsole";
import styles from "./DetailPane.module.css";

export function DetailPane() {
  const activeTorrentId = useInspectionStore(
    (state) => state.presentation.activeTorrentId,
  );
  const torrent = useInspectionStore((state) =>
    state.presentation.activeTorrentId === null
      ? undefined
      : state.torrents[state.presentation.activeTorrentId],
  );
  const activeTab = useInspectionStore((state) => state.presentation.activeTab);
  const layout = useInspectionStore((state) => state.presentation.layout);
  const detailOpen = useInspectionStore(
    (state) => state.presentation.detailOpen,
  );
  const selectTab = useInspectionStore((state) => state.selectTab);
  const closeDetail = useInspectionStore((state) => state.closeDetail);
  const tabsRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      const tabList = tabsRef.current;
      const active = tabList?.querySelector<HTMLElement>(
        `[data-tab-id="${activeTab}"]`,
      );
      if (tabList === null || active === null || active === undefined) return;
      const left = Math.max(
        0,
        active.offsetLeft - (tabList.clientWidth - active.offsetWidth) / 2,
      );
      if (typeof tabList.scrollTo === "function") {
        tabList.scrollTo({ left });
      } else {
        active.scrollIntoView?.({ block: "nearest", inline: "nearest" });
      }
    });
    return () => cancelAnimationFrame(frame);
  }, [activeTab, detailOpen, layout]);

  const selectAdjacentTab = (tab: DetailTab, direction: -1 | 1) => {
    const index = DETAIL_TABS.findIndex((candidate) => candidate.id === tab);
    const next = DETAIL_TABS[(index + direction + DETAIL_TABS.length) % DETAIL_TABS.length];
    if (next === undefined) return;
    selectTab(next.id);
    requestAnimationFrame(() => {
      document.querySelector<HTMLElement>(`[data-tab-id="${next.id}"]`)?.focus();
    });
  };

  return (
    <section className={styles.detail} aria-label="Torrent details">
      <div className={styles.mobileHeading}>
        <button type="button" onClick={closeDetail}>
          <span aria-hidden="true">←</span> Torrents
        </button>
        <strong>{torrent?.name ?? "Torrent details"}</strong>
      </div>
      <div
        ref={tabsRef}
        className={styles.tabs}
        role="tablist"
        aria-label="Torrent detail views"
      >
        {DETAIL_TABS.map((tab) => {
          const count =
            tab.id === "peers"
              ? (torrent?.peersConnected ?? null)
              : tab.id === "trackers"
                ? (torrent?.configuredTrackerCount ?? null)
                : undefined;
          return (
            <button
              key={tab.id}
              type="button"
              role="tab"
              id={`tab-${tab.id}`}
              data-tab-id={tab.id}
              data-tab-scope={tab.scope}
              aria-label={tab.label}
              aria-selected={activeTab === tab.id}
              aria-controls={`panel-${tab.id}`}
              tabIndex={activeTab === tab.id ? 0 : -1}
              onClick={() => selectTab(tab.id)}
              onKeyDown={(event) => {
                if (event.key === "ArrowLeft") {
                  event.preventDefault();
                  selectAdjacentTab(tab.id, -1);
                } else if (event.key === "ArrowRight") {
                  event.preventDefault();
                  selectAdjacentTab(tab.id, 1);
                }
              }}
            >
              {tab.label}
              {count === undefined ? null : (
                <span
                  className={styles.tabCount}
                  data-empty={torrent === undefined ? "true" : undefined}
                  title={count === null ? "Count unavailable" : count.toLocaleString()}
                  aria-hidden="true"
                >
                  {count === null ? "—" : formatTabCount(count)}
                </span>
              )}
            </button>
          );
        })}
      </div>
      <div
        className={styles.panel}
        id={`panel-${activeTab}`}
        role="tabpanel"
        aria-labelledby={`tab-${activeTab}`}
      >
        {torrent === undefined && activeTab !== "logs" && activeTab !== "disk" ? (
          <EmptyDetail />
        ) : activeTab === "peers" && activeTorrentId !== null ? (
          <PeerTable torrentId={activeTorrentId} />
        ) : activeTab === "swarm" && activeTorrentId !== null ? (
          <SwarmTable torrentId={activeTorrentId} />
        ) : activeTab === "trackers" && activeTorrentId !== null ? (
          <TrackerTable torrentId={activeTorrentId} />
        ) : activeTab === "files" && activeTorrentId !== null ? (
          <FileTable torrentId={activeTorrentId} />
        ) : activeTab === "pieces" && activeTorrentId !== null ? (
          <PieceMapPanel torrentId={activeTorrentId} />
        ) : activeTab === "disk" ? (
          <DiskPanel />
        ) : activeTab === "general" && torrent !== undefined ? (
          <GeneralDetail torrent={torrent} />
        ) : activeTab === "logs" ? (
          <LogConsole />
        ) : (
          <UnavailableDetail tab={activeTab} />
        )}
      </div>
    </section>
  );
}

function formatTabCount(count: number): string {
  return count > 99 ? "99+" : count.toLocaleString();
}

function GeneralDetail({
  torrent,
}: {
  readonly torrent: NonNullable<ReturnType<typeof useActiveTorrent>>;
}) {
  return (
    <div className={styles.general}>
      <section className={styles.summaryCard}>
        <div>
          <p className={styles.eyebrow}>Active transfer</p>
          <h2>{torrent.name}</h2>
          <p>{torrent.progressReason}</p>
        </div>
        <div className={styles.largeProgress}>
          <strong>{formatProgress(torrent.progress)}</strong>
          <span aria-hidden="true">
            <span style={{ width: `${Math.round((torrent.progress ?? 0) * 100)}%` }} />
          </span>
        </div>
      </section>
      <dl className={styles.metrics}>
        <Metric label="Status" value={torrent.status} />
        <Metric label="Size" value={formatBytes(torrent.sizeBytes)} />
        <Metric label="Downloaded" value={formatBytes(torrent.downloadedBytes)} />
        <Metric label="Uploaded" value={formatBytes(torrent.uploadedBytes)} />
        <Metric label="Download speed" value={formatRate(torrent.downloadRate)} />
        <Metric label="Upload speed" value={formatRate(torrent.uploadRate)} />
        <Metric label="Connected peers" value={torrent.peersConnected.toLocaleString()} />
        <Metric
          label="Known peers"
          value={torrent.peersKnown?.toLocaleString() ?? "—"}
        />
      </dl>
      <div className={styles.identity}>
        <span>Info hash</span>
        <code>{torrent.infoHash}</code>
      </div>
      {torrent.error === null ? null : (
        <div className={styles.error} role="alert">
          <strong>Storage needs attention</strong>
          <span>{torrent.error}</span>
        </div>
      )}
    </div>
  );
}

function useActiveTorrent() {
  return useInspectionStore((state) =>
    state.presentation.activeTorrentId === null
      ? undefined
      : state.torrents[state.presentation.activeTorrentId],
  );
}

function Metric({ label, value }: { readonly label: string; readonly value: string }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}

function EmptyDetail() {
  return (
    <div className={styles.empty}>
      <span className={styles.emptyMark} aria-hidden="true">↙</span>
      <strong>Select a torrent to inspect it</strong>
      <p>The detail surface preserves its tab and navigation context.</p>
    </div>
  );
}

function UnavailableDetail({ tab }: { readonly tab: DetailTab }) {
  return (
    <div className={styles.empty}>
      <span className={styles.emptyMark} aria-hidden="true">◇</span>
      <strong>{titleCase(tab)} view scaffold</strong>
      <p>
        This named projection is not connected yet. The empty state is
        intentional and does not claim that the engine has no {tab} data.
      </p>
    </div>
  );
}

function titleCase(value: string): string {
  return value.slice(0, 1).toUpperCase() + value.slice(1);
}
