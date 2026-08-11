import { useEffect, useRef, useState, type FormEvent } from "react";

import { useInspectionCommand, useInspectionStore } from "../context";
import {
  checkingStatusLabel,
  formatBytes,
  formatDuration,
  formatProgress,
  formatRate,
  torrentVisibleProgress,
} from "../format";
import type { DetailTab } from "../model";
import {
  RATE_LIMIT_MAXIMUM_BYTES,
  rateLimitDraftValue,
  sameRateLimit,
  validateRateLimit,
} from "../transfer-rate";
import { DETAIL_TABS } from "../tabs";
import { PeerTable } from "./PeerTable";
import { SwarmTable } from "./SwarmTable";
import { FileTable } from "./FileTable";
import { TrackerTable } from "./TrackerTable";
import { DiskPanel } from "./DiskPanel";
import { PieceMapPanel } from "./PieceMapPanel";
import { LogConsole } from "./LogConsole";
import { SpeedPanel } from "./SpeedPanel";
import { DhtPanel } from "./DhtPanel";
import styles from "./DetailPane.module.css";

export function DetailPane() {
  const currentTorrentId = useInspectionStore(
    (state) => state.presentation.currentTorrentId,
  );
  const torrent = useInspectionStore((state) =>
    state.presentation.currentTorrentId === null
      ? undefined
      : state.torrents[state.presentation.currentTorrentId],
  );
  const activeTab = useInspectionStore((state) => state.presentation.activeTab);
  const layout = useInspectionStore((state) => state.presentation.layout);
  const interfaceSize = useInspectionStore(
    (state) => state.presentation.interfaceSize,
  );
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
  }, [activeTab, detailOpen, interfaceSize, layout]);

  const selectAdjacentTab = (tab: DetailTab, direction: -1 | 1) => {
    const index = DETAIL_TABS.findIndex((candidate) => candidate.id === tab);
    const next =
      DETAIL_TABS[
        (index + direction + DETAIL_TABS.length) % DETAIL_TABS.length
      ];
    if (next === undefined) return;
    selectTab(next.id);
    requestAnimationFrame(() => {
      document
        .querySelector<HTMLElement>(`[data-tab-id="${next.id}"]`)
        ?.focus();
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
        {DETAIL_TABS.map((tab) => (
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
          </button>
        ))}
      </div>
      <div
        className={styles.panel}
        id={`panel-${activeTab}`}
        role="tabpanel"
        aria-labelledby={`tab-${activeTab}`}
      >
        {torrent === undefined &&
        activeTab !== "logs" &&
        activeTab !== "disk" &&
        activeTab !== "speed" &&
        activeTab !== "dht" ? (
          <EmptyDetail />
        ) : activeTab === "peers" && currentTorrentId !== null ? (
          <PeerTable torrentId={currentTorrentId} />
        ) : activeTab === "swarm" && currentTorrentId !== null ? (
          <SwarmTable torrentId={currentTorrentId} />
        ) : activeTab === "trackers" && currentTorrentId !== null ? (
          <TrackerTable torrentId={currentTorrentId} />
        ) : activeTab === "files" && currentTorrentId !== null ? (
          <FileTable torrentId={currentTorrentId} />
        ) : activeTab === "pieces" && currentTorrentId !== null ? (
          <PieceMapPanel torrentId={currentTorrentId} />
        ) : activeTab === "disk" ? (
          <DiskPanel />
        ) : activeTab === "speed" ? (
          <SpeedPanel />
        ) : activeTab === "dht" ? (
          <DhtPanel />
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

function GeneralDetail({
  torrent,
}: {
  readonly torrent: NonNullable<ReturnType<typeof useCurrentTorrent>>;
}) {
  const detailTarget = useInspectionStore(
    (state) => state.presentation.detailTarget,
  );
  const clearDetailTarget = useInspectionStore(
    (state) => state.clearDetailTarget,
  );
  const dataUnits = useInspectionStore((state) => state.presentation.dataUnits);
  const errorRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (
      detailTarget?.type !== "torrent_error" ||
      detailTarget.torrentId !== torrent.id
    ) {
      return;
    }
    const error = errorRef.current;
    if (error !== null) {
      error.scrollIntoView?.({ block: "nearest" });
      error.focus({ preventScroll: true });
    }
    clearDetailTarget();
  }, [clearDetailTarget, detailTarget, torrent.id]);

  const checking = torrent.status === "checking" ? torrent.checking : null;
  const visibleProgress = torrentVisibleProgress(torrent);
  const progressLabel =
    torrent.status === "checking"
      ? checkingStatusLabel(torrent)
      : formatProgress(torrent.progress);

  return (
    <div className={styles.general}>
      <section className={styles.summaryCard}>
        <div>
          <p className={styles.eyebrow}>
            {torrent.status === "checking" ? "Current check" : "Current transfer"}
          </p>
          <h2>{torrent.name}</h2>
          <p>
            {torrent.status === "checking"
              ? checkingStatusLabel(torrent)
              : torrent.progressReason}
          </p>
        </div>
        <div className={styles.largeProgress}>
          <strong>{progressLabel}</strong>
          <span
            data-indeterminate={
              (torrent.status === "checking" && visibleProgress === null) || undefined
            }
            role="progressbar"
            aria-label={`${torrent.name} ${torrent.status === "checking" ? "checking" : "download"} progress: ${progressLabel}`}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={
              visibleProgress === null
                ? undefined
                : Math.round(visibleProgress * 100)
            }
          >
            {visibleProgress === null ? null : (
              <span style={{ width: `${Math.round(visibleProgress * 100)}%` }} />
            )}
          </span>
        </div>
      </section>
      {checking === null ? null : (
        <dl className={`${styles.metrics} ${styles.checkingMetrics}`}>
          <Metric
            label="Pieces checked"
            value={`${checking.piecesProcessed.toLocaleString()} / ${checking.piecesTotal.toLocaleString()}`}
          />
          <Metric label="Matched" value={checking.piecesMatched.toLocaleString()} />
          <Metric label="Absent" value={checking.piecesAbsent.toLocaleString()} />
          <Metric
            label="Mismatched"
            value={checking.piecesMismatched.toLocaleString()}
          />
          <Metric label="Checker activity" value={checkerActivity(checking)} />
        </dl>
      )}
      <dl className={styles.metrics}>
        <Metric label="Status" value={torrent.status} />
        <Metric
          label="Size"
          value={formatBytes(torrent.sizeBytes, dataUnits)}
        />
        <Metric
          label="Downloaded"
          value={formatBytes(torrent.downloadedBytes, dataUnits)}
        />
        <Metric
          label="Uploaded"
          value={formatBytes(torrent.uploadedBytes, dataUnits)}
        />
        <Metric
          label="Download speed"
          value={formatRate(torrent.downloadRate, dataUnits)}
        />
        <Metric
          label="Upload speed"
          value={formatRate(torrent.uploadRate, dataUnits)}
        />
        <Metric
          label="Connected peers"
          value={torrent.peersConnected.toLocaleString()}
        />
        <Metric
          label="Known peers"
          value={torrent.peersKnown?.toLocaleString() ?? "—"}
        />
      </dl>
      <div className={styles.identity}>
        <span>Info hash</span>
        <code>{torrent.infoHash}</code>
      </div>
      <TorrentRateLimits torrent={torrent} />
      {torrent.error === null ? null : (
        <div ref={errorRef} className={styles.error} role="alert" tabIndex={-1}>
          <strong>Storage needs attention</strong>
          <span>{torrent.error}</span>
        </div>
      )}
    </div>
  );
}

function TorrentRateLimits({
  torrent,
}: {
  readonly torrent: NonNullable<ReturnType<typeof useCurrentTorrent>>;
}) {
  const execute = useInspectionCommand();
  const [uploadUnlimited, setUploadUnlimited] = useState(
    torrent.transferLimits.upload.type === "unlimited",
  );
  const [uploadKiB, setUploadKiB] = useState(
    rateLimitDraftValue(torrent.transferLimits.upload, "1024"),
  );
  const [downloadUnlimited, setDownloadUnlimited] = useState(
    torrent.transferLimits.download.type === "unlimited",
  );
  const [downloadKiB, setDownloadKiB] = useState(
    rateLimitDraftValue(torrent.transferLimits.download, "4096"),
  );
  const [pending, setPending] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    setUploadUnlimited(torrent.transferLimits.upload.type === "unlimited");
    if (torrent.transferLimits.upload.type === "limited") {
      setUploadKiB(rateLimitDraftValue(torrent.transferLimits.upload, "1024"));
    }
    setDownloadUnlimited(torrent.transferLimits.download.type === "unlimited");
    if (torrent.transferLimits.download.type === "limited") {
      setDownloadKiB(rateLimitDraftValue(torrent.transferLimits.download, "4096"));
    }
  }, [torrent.transferLimits]);

  const upload = validateRateLimit(uploadUnlimited, uploadKiB);
  const download = validateRateLimit(downloadUnlimited, downloadKiB);
  const limits =
    upload.limit === null || download.limit === null
      ? null
      : { upload: upload.limit, download: download.limit };
  const dirty =
    limits !== null &&
    (!sameRateLimit(limits.upload, torrent.transferLimits.upload) ||
      !sameRateLimit(limits.download, torrent.transferLimits.download));

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!dirty || limits === null || pending) return;
    setPending(true);
    setStatus(null);
    try {
      await execute({
        type: "set_torrent_transfer_limits",
        torrentId: torrent.id,
        limits,
      });
      setStatus("Torrent peer transfer limits saved.");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setPending(false);
    }
  };

  return (
    <form className={styles.rateLimits} onSubmit={(event) => void submit(event)}>
      <div>
        <p className={styles.eyebrow}>Peer transfer limits</p>
        <h3>Only this torrent</h3>
        <p>
          These caps combine with the All torrents limits. Trackers, DHT,
          connection handshakes, and network headers are not counted.
        </p>
      </div>
      <TorrentRateField
        direction="upload"
        unlimited={uploadUnlimited}
        value={uploadKiB}
        error={upload.error}
        disabled={pending}
        onUnlimited={setUploadUnlimited}
        onValue={setUploadKiB}
      />
      <TorrentRateField
        direction="download"
        unlimited={downloadUnlimited}
        value={downloadKiB}
        error={download.error}
        disabled={pending}
        onUnlimited={setDownloadUnlimited}
        onValue={setDownloadKiB}
      />
      <div className={styles.rateLimitActions}>
        <button type="submit" disabled={!dirty || pending || limits === null}>
          {pending ? "Saving…" : "Save torrent limits"}
        </button>
        {status === null ? null : <output aria-live="polite">{status}</output>}
      </div>
    </form>
  );
}

function TorrentRateField({
  direction,
  unlimited,
  value,
  error,
  disabled,
  onUnlimited,
  onValue,
}: {
  readonly direction: "upload" | "download";
  readonly unlimited: boolean;
  readonly value: string;
  readonly error: string | null;
  readonly disabled: boolean;
  readonly onUnlimited: (value: boolean) => void;
  readonly onValue: (value: string) => void;
}) {
  const label = `Torrent ${direction} limit`;
  const id = `torrent-${direction}-rate`;
  return (
    <fieldset className={styles.rateLimitField}>
      <legend>{label}</legend>
      <label>
        <input
          type="checkbox"
          aria-label={`${label} unlimited`}
          checked={unlimited}
          disabled={disabled}
          onChange={(event) => onUnlimited(event.currentTarget.checked)}
        />
        Unlimited
      </label>
      <label htmlFor={id}>KiB/s</label>
      <input
        id={id}
        aria-label={`${label} in KiB per second`}
        type="number"
        inputMode="decimal"
        min={1}
        max={RATE_LIMIT_MAXIMUM_BYTES / 1_024}
        step={1 / 1_024}
        value={value}
        required={!unlimited}
        disabled={disabled || unlimited}
        aria-invalid={error !== null}
        onChange={(event) => onValue(event.currentTarget.value)}
      />
      {error === null ? null : <small role="alert">{error}</small>}
    </fieldset>
  );
}

function checkerActivity(
  checking: NonNullable<ReturnType<typeof useCurrentTorrent>>["checking"],
): string {
  if (checking === null) return "Waiting";
  if (checking.oldestActiveJobAgeMs !== null) {
    return `${checking.activeHashJobs.toLocaleString()} active · oldest ${formatDuration(checking.oldestActiveJobAgeMs)}`;
  }
  if (checking.queuedHashJobs > 0) {
    return `${checking.queuedHashJobs.toLocaleString()} queued · last advance ${formatDuration(checking.lastAdvanceAgeMs)} ago`;
  }
  return `Last advance ${formatDuration(checking.lastAdvanceAgeMs)} ago`;
}

function useCurrentTorrent() {
  return useInspectionStore((state) =>
    state.presentation.currentTorrentId === null
      ? undefined
      : state.torrents[state.presentation.currentTorrentId],
  );
}

function Metric({
  label,
  value,
}: {
  readonly label: string;
  readonly value: string;
}) {
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
      <span className={styles.emptyMark} aria-hidden="true">
        ↙
      </span>
      <strong>Select a torrent to inspect it</strong>
      <p>The detail surface preserves its tab and navigation context.</p>
    </div>
  );
}

function UnavailableDetail({ tab }: { readonly tab: DetailTab }) {
  return (
    <div className={styles.empty}>
      <span className={styles.emptyMark} aria-hidden="true">
        ◇
      </span>
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
