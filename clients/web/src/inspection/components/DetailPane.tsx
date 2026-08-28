import { useEffect, useRef, useState, type FormEvent } from "react";

import type { TorrentSettingsPatch, TransferRateLimit } from "../../api";
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
  settingsDraftFields,
  settingsDraftPhase,
  settingsDraftValue,
  type SettingsDraftComparators,
  type SettingsDraftPhase,
  type SettingsDraftState,
} from "../settings-draft";
import {
  RATE_LIMIT_MAXIMUM_BYTES,
  rateLimitDraftValue,
  validateRateLimit,
} from "../transfer-rate";
import { DETAIL_TABS } from "../tabs";
import { useSettingsDraft } from "../use-settings-draft";
import { PeerTable } from "./PeerTable";
import { SwarmTable } from "./SwarmTable";
import { FileTable } from "./FileTable";
import { TrackerTable } from "./TrackerTable";
import { DiskPanel } from "./DiskPanel";
import { PieceMapPanel } from "./PieceMapPanel";
import { PreparationProgress } from "./PreparationProgress";
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
            data-indeterminate={visibleProgress === null || undefined}
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
      {torrent.preparation == null ? null : (
        <PreparationProgress
          preparation={torrent.preparation}
          dataUnits={dataUnits}
        />
      )}
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
      {torrent.protocolIdentities?.v1 != null &&
      torrent.protocolIdentities.v2 != null ? (
        <>
          <div className={styles.identity}>
            <span>Info hash (v1)</span>
            <code>{torrent.protocolIdentities.v1}</code>
          </div>
          <div className={styles.identity}>
            <span>Info hash (v2)</span>
            <code>{torrent.protocolIdentities.v2}</code>
          </div>
        </>
      ) : (
        <div className={styles.identity}>
          <span>Info hash</span>
          <code>{torrent.infoHash}</code>
        </div>
      )}
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
  const durableRevision = useInspectionStore((state) => state.durableRevision);
  const transportPending = useRef(false);
  const [acceptedMessage, setAcceptedMessage] = useState<string | null>(null);
  const authority = torrentRateDraft(torrent.transferLimits);
  const [draftState, dispatchDraft] = useSettingsDraft(
    torrent.id,
    durableRevision,
    authority,
    TORRENT_RATE_COMPARATORS,
  );
  const draft = settingsDraftValue(draftState) ?? authority;
  const upload = validateRateLimit(draft.upload.unlimited, draft.upload.valueKiB);
  const download = validateRateLimit(
    draft.download.unlimited,
    draft.download.valueKiB,
  );
  const dirtyFields = settingsDraftFields(draftState);
  const patch = torrentRatePatch(dirtyFields, upload.limit, download.limit);
  const phase = settingsDraftPhase(draftState);
  const pending = phase === "submitting" || phase === "awaiting_view";

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (patch === null || transportPending.current || draftState.submission !== null) {
      return;
    }
    transportPending.current = true;
    setAcceptedMessage(null);
    dispatchDraft({ type: "submit" });
    try {
      const result = await execute({
        type: "update_torrent_settings",
        torrentId: torrent.id,
        patch,
      });
      if (result.resultingRevision === undefined) {
        throw new Error("Settings response did not include a durable revision.");
      }
      setAcceptedMessage("Torrent peer transfer limits saved.");
      dispatchDraft({ type: "accept", revision: result.resultingRevision });
    } catch (error) {
      setAcceptedMessage(null);
      dispatchDraft({
        type: "fail",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      transportPending.current = false;
    }
  };
  const status =
    draftStatus(draftState, phase) ??
    (phase === "pristine" ? acceptedMessage : null);

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
        unlimited={draft.upload.unlimited}
        value={draft.upload.valueKiB}
        error={upload.error}
        disabled={false}
        onUnlimited={(unlimited) =>
          dispatchDraft({
            type: "edit",
            field: "upload",
            value: { ...draft.upload, unlimited },
          })
        }
        onValue={(valueKiB) =>
          dispatchDraft({
            type: "edit",
            field: "upload",
            value: { ...draft.upload, valueKiB },
          })
        }
      />
      <TorrentRateField
        direction="download"
        unlimited={draft.download.unlimited}
        value={draft.download.valueKiB}
        error={download.error}
        disabled={false}
        onUnlimited={(unlimited) =>
          dispatchDraft({
            type: "edit",
            field: "download",
            value: { ...draft.download, unlimited },
          })
        }
        onValue={(valueKiB) =>
          dispatchDraft({
            type: "edit",
            field: "download",
            value: { ...draft.download, valueKiB },
          })
        }
      />
      <div className={styles.rateLimitActions}>
        <button type="submit" disabled={patch === null || pending}>
          {pending ? "Saving…" : "Save torrent limits"}
        </button>
        {status === null ? null : (
          <output aria-live="polite">{status}</output>
        )}
      </div>
    </form>
  );
}

interface TorrentRateDraftField {
  readonly unlimited: boolean;
  readonly valueKiB: string;
}

interface TorrentRateDraft {
  readonly upload: TorrentRateDraftField;
  readonly download: TorrentRateDraftField;
}

const TORRENT_RATE_COMPARATORS: SettingsDraftComparators<TorrentRateDraft> = {
  upload: sameTorrentRateDraftField,
  download: sameTorrentRateDraftField,
};

function torrentRateDraft(limits: {
  readonly upload: TransferRateLimit;
  readonly download: TransferRateLimit;
}): TorrentRateDraft {
  return {
    upload: {
      unlimited: limits.upload.type === "unlimited",
      valueKiB: rateLimitDraftValue(limits.upload, "1024"),
    },
    download: {
      unlimited: limits.download.type === "unlimited",
      valueKiB: rateLimitDraftValue(limits.download, "4096"),
    },
  };
}

function sameTorrentRateDraftField(
  left: TorrentRateDraftField,
  right: TorrentRateDraftField,
): boolean {
  return left.unlimited === right.unlimited &&
    (left.unlimited || left.valueKiB === right.valueKiB);
}

function torrentRatePatch(
  fields: readonly (keyof TorrentRateDraft)[],
  upload: TransferRateLimit | null,
  download: TransferRateLimit | null,
): TorrentSettingsPatch | null {
  if (upload === null || download === null || fields.length === 0) return null;
  return {
    ...(fields.includes("upload") ? { upload_rate_limit: upload } : {}),
    ...(fields.includes("download") ? { download_rate_limit: download } : {}),
  };
}

function draftStatus(
  state: SettingsDraftState<TorrentRateDraft>,
  phase: SettingsDraftPhase,
): string | null {
  if (phase === "submitting") return "Saving torrent limits…";
  if (phase === "awaiting_view") return "Saved; waiting for the live view…";
  if (phase === "conflict") {
    return "These limits changed elsewhere. Your draft is preserved for review.";
  }
  return state.failure;
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
