import { useMemo, type CSSProperties } from "react";

import { useInspectionStore } from "../context";
import { formatBytes, formatRate } from "../format";
import type { DiskPieceRow, DiskPipeline, ViewMaterialization } from "../model";
import { VirtualTable, type VirtualColumn } from "./VirtualTable";
import styles from "./DiskPanel.module.css";

const DISK_COLUMNS: readonly VirtualColumn<DiskPieceRow>[] = [
  {
    id: "torrent",
    label: "Torrent",
    width: 210,
    minimumWidth: 130,
    maximumWidth: 520,
    sortValue: (row) => row.torrentName,
    render: (row) => <span title={row.torrentName}>{row.torrentName}</span>,
  },
  {
    id: "piece",
    label: "Piece",
    width: 76,
    align: "right",
    sortKind: "number",
    sortValue: (row) => row.pieceIndex,
    render: (row) => row.pieceIndex.toLocaleString(),
  },
  {
    id: "stage",
    label: "State",
    width: 112,
    sortValue: (row) => row.stage,
    sortOrder: [
      "failed",
      "checkpoint_committing",
      "checkpoint_syncing",
      "checkpoint_dirty",
      "hashing",
      "writing",
      "queued",
      "receiving",
      "stored",
    ],
    render: (row) => (
      <span className={styles.stage} data-stage={row.stage}>
        <span aria-hidden="true" />
        {titleCase(row.stage)}
      </span>
    ),
  },
  {
    id: "requested",
    label: "Requested",
    width: 96,
    align: "right",
    minimumViewport: 520,
    sortKind: "number",
    sortValue: (row) => row.requestedBytes,
    render: (row) => formatBytes(row.requestedBytes),
  },
  {
    id: "received",
    label: "Received",
    width: 92,
    align: "right",
    minimumViewport: 620,
    sortKind: "number",
    sortValue: (row) => row.receivedBytes,
    render: (row) => formatBytes(row.receivedBytes),
  },
  {
    id: "stored",
    label: "Stored",
    width: 88,
    align: "right",
    minimumViewport: 700,
    sortKind: "number",
    sortValue: (row) => row.storedBytes,
    render: (row) => formatBytes(row.storedBytes),
  },
  {
    id: "queueAge",
    label: "State age",
    width: 90,
    align: "right",
    minimumViewport: 760,
    sortKind: "number",
    sortValue: (row) => row.stageAgeMillis,
    render: (row) => formatDuration(row.stageAgeMillis),
  },
  {
    id: "age",
    label: "Age",
    width: 86,
    align: "right",
    defaultVisible: false,
    sortKind: "number",
    sortValue: (row) => row.ageMillis,
    render: (row) => formatDuration(row.ageMillis),
  },
  {
    id: "attempt",
    label: "Attempt",
    width: 82,
    align: "right",
    defaultVisible: false,
    sortKind: "number",
    sortValue: (row) => row.attempt,
    render: (row) => row.attempt.toLocaleString(),
  },
  {
    id: "error",
    label: "Error",
    width: 300,
    minimumWidth: 140,
    maximumWidth: 680,
    sortValue: (row) => row.error,
    render: (row) => <span title={row.error ?? undefined}>{row.error ?? "—"}</span>,
  },
];

export function DiskPanel() {
  const disk = useInspectionStore((state) => state.disk);
  const materialization = useInspectionStore((state) => state.viewStatus.disk);
  const interfaceSize = useInspectionStore(
    (state) => state.presentation.interfaceSize,
  );
  const rows = useMemo(
    () =>
      disk.order
        .map((id) => disk.rows[id])
        .filter((row): row is DiskPieceRow => row !== undefined),
    [disk],
  );
  return (
    <div className={styles.panel}>
      <DiskSummary pipeline={disk.pipeline} activePieces={rows.length} />
      <section className={styles.tableSection} aria-labelledby="active-disk-pieces">
        <div className={styles.tableHeading}>
          <div>
            <h2 id="active-disk-pieces">Active storage pieces</h2>
            <p>Piece-level work only; 16 KiB block jobs stay inside the engine.</p>
          </div>
          <span>{rows.length.toLocaleString()} active</span>
        </div>
        <VirtualTable
          tableId="session-disk-pieces"
          label="Active storage pieces"
          rows={rows}
          getRowId={(row) => row.id}
          columns={DISK_COLUMNS}
          interfaceSize={interfaceSize}
          emptyMessage={diskEmptyMessage(materialization)}
          initialSort={{ columnId: "queueAge", direction: "desc" }}
        />
      </section>
    </div>
  );
}

function DiskSummary({
  pipeline,
  activePieces,
}: {
  readonly pipeline: DiskPipeline;
  readonly activePieces: number;
}) {
  const pipelineMaximum = Math.max(
    1,
    pipeline.residentLimitBytes,
    pipeline.requestedBytes,
    pipeline.residentBytes,
  );
  const stages = [
    ["Requested", pipeline.requestedBytes],
    ["Resident", pipeline.residentBytes],
    ["Queued write", pipeline.queuedWriteBytes],
    ["Writing", pipeline.writingBytes],
    ["Hashing", pipeline.hashingBytes],
    ["Checkpoint dirty", pipeline.checkpointDirtyBytes],
  ] as const;
  const pressureLabel = titleCase(pipeline.pressure);
  return (
    <section className={styles.summary} aria-labelledby="disk-pipeline-title">
      <div className={styles.summaryHeading}>
        <div>
          <p className={styles.eyebrow}>Session storage</p>
          <h2 id="disk-pipeline-title">Receive → write → verify → checkpoint</h2>
        </div>
        <span className={styles.pressure} data-pressure={pipeline.pressure}>
          <span aria-hidden="true" />
          {pressureLabel}
        </span>
      </div>
      <div className={styles.pipeline} aria-label={`Disk pressure ${pressureLabel}`}>
        {stages.map(([label, value]) => (
          <div key={label}>
            <span>{label}</span>
            <strong>{formatBytes(value)}</strong>
            <span className={styles.bar} aria-hidden="true">
              <span
                style={
                  {
                    "--disk-fill": `${value === 0 ? 0 : Math.max(2, Math.min(100, (value / pipelineMaximum) * 100))}%`,
                  } as CSSProperties
                }
              />
            </span>
          </div>
        ))}
      </div>
      <dl className={styles.metrics}>
        <Metric
          label="Resident / limit"
          value={`${formatBytes(pipeline.residentBytes)} / ${formatBytes(pipeline.residentLimitBytes)}`}
          detail={`high ${formatBytes(pipeline.residentHighWatermarkBytes)} · low ${formatBytes(pipeline.residentLowWatermarkBytes)}`}
        />
        <Metric
          label="Receive / write"
          value={`${formatRate(pipeline.receiveRateBytes)} / ${formatRate(pipeline.writeRateBytes)}`}
          detail={`${formatDuration(pipeline.sampleMillis)} sample`}
        />
        <Metric
          label="Stored / verified"
          value={`${formatBytes(pipeline.storedBytesTotal)} / ${formatBytes(pipeline.verifiedBytesTotal)}`}
          detail={`${formatRate(pipeline.hashRateBytes)} verify rate`}
        />
        <Metric
          label="Pending work"
          value={`${pipeline.storageJobsPending.toLocaleString()} jobs · ${activePieces.toLocaleString()} pieces`}
          detail={`${pipeline.pressureTransitionCount.toLocaleString()} pressure transitions`}
        />
        <Metric
          label="Write queue wait"
          value={formatMicros(pipeline.writeQueueWaitMaxMicros)}
          detail={`${formatMicros(pipeline.writeQueueWaitMicros)} cumulative`}
        />
        <Metric
          label="Write service"
          value={formatMicros(pipeline.writeServiceMaxMicros)}
          detail={`${pipeline.writeOperationsCompleted.toLocaleString()} / ${pipeline.writeOperationsStarted.toLocaleString()} operations`}
        />
        <Metric
          label="Hash service"
          value={formatMicros(pipeline.hashServiceMaxMicros)}
          detail={`${pipeline.hashOperationsCompleted.toLocaleString()} / ${pipeline.hashOperationsStarted.toLocaleString()} operations`}
        />
        <Metric
          label="Checkpoint backlog"
          value={`${pipeline.checkpointDirtyPieces.toLocaleString()} pieces · ${formatBytes(pipeline.checkpointDirtyBytes)}`}
          detail={`oldest ${formatDuration(pipeline.checkpointOldestDirtyMillis)} · high ${pipeline.checkpointDirtyPieceHighWater.toLocaleString()} pieces / ${formatBytes(pipeline.checkpointDirtyByteHighWater)}`}
        />
        <Metric
          label="Checkpoint stage"
          value={titleCase(pipeline.checkpointStage)}
          detail={
            pipeline.checkpointActiveMicros === null
              ? `${pipeline.checkpointBatchesCompleted.toLocaleString()} / ${pipeline.checkpointBatchesStarted.toLocaleString()} batches`
              : `${formatMicros(pipeline.checkpointActiveMicros)} active`
          }
        />
        <Metric
          label="Payload sync"
          value={formatMicros(pipeline.checkpointSyncServiceMaxMicros)}
          detail={`${formatMicros(pipeline.checkpointSyncServiceMicros)} cumulative · ${pipeline.checkpointSyncOperationsCompleted.toLocaleString()} targets`}
        />
        <Metric
          label="Checkpoint commit"
          value={formatMicros(pipeline.checkpointCommitServiceMaxMicros)}
          detail={`${formatMicros(pipeline.checkpointCommitServiceMicros)} cumulative · ${pipeline.checkpointPiecesCompleted.toLocaleString()} pieces`}
        />
        <Metric
          label="Backpressured"
          value={formatDuration(pipeline.backpressuredMillisTotal)}
          detail={pipeline.intakeBackpressured ? "intake paused now" : "intake is open"}
        />
      </dl>
      {pipeline.lastError === null ? null : (
        <div className={styles.error} role="alert">
          <strong>Storage error</strong>
          <span>{pipeline.lastError}</span>
        </div>
      )}
    </section>
  );
}

function Metric({
  label,
  value,
  detail,
}: {
  readonly label: string;
  readonly value: string;
  readonly detail: string;
}) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>
        <span>{value}</span>
        <small>{detail}</small>
      </dd>
    </div>
  );
}

function diskEmptyMessage(materialization: ViewMaterialization): string {
  switch (materialization.status) {
    case "not_requested":
      return "Disk state is not requested.";
    case "loading":
      return "Loading disk state…";
    case "unavailable":
    case "unsupported":
    case "stale":
      return materialization.reason;
    case "ready":
      return "No pieces are waiting on storage.";
  }
}

function formatMicros(micros: number): string {
  return formatDuration(micros / 1_000);
}

function formatDuration(milliseconds: number): string {
  if (!Number.isFinite(milliseconds) || milliseconds <= 0) return "0 ms";
  if (milliseconds < 1_000) return `${Math.round(milliseconds)} ms`;
  if (milliseconds < 60_000) return `${(milliseconds / 1_000).toFixed(milliseconds < 10_000 ? 1 : 0)} s`;
  return `${Math.floor(milliseconds / 60_000)}m ${Math.floor((milliseconds % 60_000) / 1_000)}s`;
}

function titleCase(value: string): string {
  return value.slice(0, 1).toUpperCase() + value.slice(1).replaceAll("_", " ");
}
