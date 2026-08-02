import { useEffect, useMemo, useRef } from "react";

import { useInspectionStore } from "../context";
import {
  formatBytes,
  formatProgress,
  formatRate,
  formatTime,
} from "../format";
import type { DetailTab, LogRow, ViewMaterialization } from "../model";
import { visibleLogs } from "../state";
import { PeerTable } from "./PeerTable";
import { FileTable } from "./FileTable";
import { TrackerTable } from "./TrackerTable";
import { DiskPanel } from "./DiskPanel";
import { VirtualTable, type VirtualColumn } from "./VirtualTable";
import styles from "./DetailPane.module.css";

const TABS: readonly {
  readonly id: DetailTab;
  readonly label: string;
  readonly scope: "torrent" | "session";
}[] = [
  { id: "general", label: "General", scope: "torrent" },
  { id: "trackers", label: "Trackers", scope: "torrent" },
  { id: "peers", label: "Peers", scope: "torrent" },
  { id: "swarm", label: "Swarm", scope: "torrent" },
  { id: "files", label: "Files", scope: "torrent" },
  { id: "pieces", label: "Pieces", scope: "torrent" },
  { id: "disk", label: "Disk", scope: "session" },
  { id: "logs", label: "Logs", scope: "session" },
  { id: "speed", label: "Speed", scope: "session" },
  { id: "dht", label: "DHT", scope: "session" },
];

const LOG_COLUMNS: readonly VirtualColumn<LogRow>[] = [
  {
    id: "time",
    label: "Time",
    width: 86,
    sortable: true,
    sortValue: (row) => row.timestampMs,
    render: (row) => <time>{formatTime(row.timestampMs)}</time>,
  },
  {
    id: "severity",
    label: "Level",
    width: 82,
    sortable: true,
    sortValue: (row) => row.severity,
    render: (row) => (
      <span className={styles.logSeverity} data-severity={row.severity}>
        {row.severity}
      </span>
    ),
  },
  {
    id: "category",
    label: "Category",
    width: 104,
    minimumViewport: 520,
    sortable: true,
    sortValue: (row) => row.category,
    render: (row) => <code>{row.category}</code>,
  },
  {
    id: "summary",
    label: "Message",
    width: 680,
    sortable: true,
    sortValue: (row) => row.summary,
    render: (row) => <span title={row.summary}>{row.summary}</span>,
  },
];

export function DetailPane() {
  const selectedId = useInspectionStore(
    (state) => state.presentation.selectedTorrentId,
  );
  const torrent = useInspectionStore((state) =>
    state.presentation.selectedTorrentId === null
      ? undefined
      : state.torrents[state.presentation.selectedTorrentId],
  );
  const activeTab = useInspectionStore((state) => state.presentation.activeTab);
  const layout = useInspectionStore((state) => state.presentation.layout);
  const selectTab = useInspectionStore((state) => state.selectTab);
  const closeDetail = useInspectionStore((state) => state.closeDetail);
  const logs = useInspectionStore((state) => state.logs);
  const droppedLogs = useInspectionStore((state) => state.droppedLogs);
  const logsMaterialization = useInspectionStore((state) => state.viewStatus.logs);
  const selectedLogs = useMemo(
    () => visibleLogs(logs, selectedId),
    [logs, selectedId],
  );
  const tabsRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      tabsRef.current
        ?.querySelector<HTMLElement>(`[data-tab-id="${activeTab}"]`)
        ?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    });
    return () => cancelAnimationFrame(frame);
  }, [activeTab, layout]);

  const selectAdjacentTab = (tab: DetailTab, direction: -1 | 1) => {
    const index = TABS.findIndex((candidate) => candidate.id === tab);
    const next = TABS[(index + direction + TABS.length) % TABS.length];
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
        {TABS.map((tab) => {
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
        ) : activeTab === "peers" && selectedId !== null ? (
          <PeerTable torrentId={selectedId} />
        ) : activeTab === "trackers" && selectedId !== null ? (
          <TrackerTable torrentId={selectedId} />
        ) : activeTab === "files" && selectedId !== null ? (
          <FileTable torrentId={selectedId} />
        ) : activeTab === "disk" ? (
          <DiskPanel />
        ) : activeTab === "general" && torrent !== undefined ? (
          <GeneralDetail torrent={torrent} />
        ) : activeTab === "logs" ? (
          <div className={styles.logPanel}>
            <div className={styles.logSummary}>
              <span>{selectedLogs.length.toLocaleString()} shown</span>
              <span>{droppedLogs.toLocaleString()} dropped</span>
              <span>Selected torrent + session</span>
            </div>
            <VirtualTable
              tableId="logs"
              label="Diagnostic log"
              rows={selectedLogs}
              getRowId={(row) => row.id}
              columns={LOG_COLUMNS}
              emptyMessage={detailEmptyMessage(logsMaterialization, "diagnostic events")}
              initialSort={{ columnId: "time", direction: "asc" }}
            />
          </div>
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

function detailEmptyMessage(
  materialization: ViewMaterialization,
  noun: string,
): string {
  switch (materialization.status) {
    case "not_requested":
      return `${titleCase(noun)} are not requested.`;
    case "loading":
      return `Loading ${noun}…`;
    case "unavailable":
    case "unsupported":
    case "stale":
      return materialization.reason;
    case "ready":
      return `No ${noun} are currently available.`;
  }
}

function GeneralDetail({
  torrent,
}: {
  readonly torrent: NonNullable<ReturnType<typeof useSelectedTorrent>>;
}) {
  return (
    <div className={styles.general}>
      <section className={styles.summaryCard}>
        <div>
          <p className={styles.eyebrow}>Selected transfer</p>
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

function useSelectedTorrent() {
  return useInspectionStore((state) =>
    state.presentation.selectedTorrentId === null
      ? undefined
      : state.torrents[state.presentation.selectedTorrentId],
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
