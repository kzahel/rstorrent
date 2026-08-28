import { useEffect, useMemo, useRef, useState } from "react";

import type { DataUnits } from "../appearance";
import { formatBytes } from "../format";
import type {
  MetadataAcquisitionProgress,
  TorrentPreparation,
} from "../model";
import styles from "./PreparationProgress.module.css";

const CELL_SIZE = 5;
const CELL_GAP = 1;
const MAX_DEVICE_PIXEL_RATIO = 3;

export const enum MetadataBlockState {
  Missing = 0,
  Requested = 1,
  Received = 2,
}

export function metadataBlockStateAt(
  packed: Uint8Array,
  blockIndex: number,
): MetadataBlockState {
  const byte = packed[Math.floor(blockIndex / 4)] ?? 0;
  return ((byte >> ((blockIndex % 4) * 2)) & 0b11) as MetadataBlockState;
}

export function PreparationProgress({
  preparation,
  dataUnits,
}: {
  readonly preparation: TorrentPreparation;
  readonly dataUnits: DataUnits;
}) {
  return (
    <div className={styles.stack} aria-label="Torrent preparation activity">
      {preparation.metadata === null ? null : (
        <MetadataCard
          metadata={preparation.metadata}
          dataUnits={dataUnits}
        />
      )}
      {preparation.integrity === null ? null : (
        <section className={styles.card} aria-label="Piece hash preparation">
          <header className={styles.header}>
            <div>
              <p className={styles.eyebrow}>Content preparation</p>
              <h3>
                {preparation.integrity.phase === "waiting_for_peer"
                  ? "Waiting for a hash-capable peer"
                  : "Fetching piece hashes"}
              </h3>
              <p>
                {preparation.integrity.phase === "waiting_for_peer"
                  ? "The selected files need Merkle proof data before payload requests can start."
                  : "Downloading Merkle proof data needed to verify the selected files."}
              </p>
            </div>
            <span className={styles.phase}>
              {preparation.integrity.phase === "waiting_for_peer"
                ? "Waiting"
                : "Active"}
            </span>
          </header>
          <dl className={styles.metrics}>
            <Metric
              label="Hash ranges needed"
              value={preparation.integrity.neededHashRanges.toLocaleString()}
            />
            <Metric
              label="Active requests"
              value={preparation.integrity.activeRequests.toLocaleString()}
            />
          </dl>
        </section>
      )}
    </div>
  );
}

function MetadataCard({
  metadata,
  dataUnits,
}: {
  readonly metadata: MetadataAcquisitionProgress;
  readonly dataUnits: DataUnits;
}) {
  const total = metadata.totalSizeBytes;
  const progress =
    total === null ? null : Math.min(1, metadata.receivedBytes / total);
  const progressText =
    total === null
      ? "Discovering metadata size"
      : `${formatBytes(metadata.receivedBytes, dataUnits)} of ${formatBytes(total, dataUnits)}`;

  return (
    <section className={styles.card} aria-label="Metadata download progress">
      <header className={styles.header}>
        <div>
          <p className={styles.eyebrow}>Metadata preparation</p>
          <h3>
            {metadata.phase === "discovering"
              ? "Finding metadata"
              : "Downloading metadata"}
          </h3>
          <p>
            {metadata.phase === "discovering"
              ? "Waiting for a metadata-capable peer to advertise the info dictionary."
              : "Receiving the verified info dictionary before file selection and piece transfer."}
          </p>
        </div>
        <strong className={styles.progressText}>
          {progress === null ? "Waiting" : `${Math.floor(progress * 100)}%`}
        </strong>
      </header>
      <div className={styles.progressRow}>
        <progress
          aria-label="Metadata bytes received"
          max={total ?? undefined}
          value={total === null ? undefined : metadata.receivedBytes}
        />
        <span>{progressText}</span>
      </div>
      {metadata.blockCount === 0 ? null : <MetadataBlockMap metadata={metadata} />}
      <dl className={styles.metrics}>
        <Metric
          label="Metadata peers"
          value={metadata.activePeers.toLocaleString()}
        />
        <Metric
          label="Requests in flight"
          value={metadata.requestsInFlight.toLocaleString()}
        />
        <Metric label="Hash retries" value={metadata.hashRetries.toLocaleString()} />
      </dl>
    </section>
  );
}

function MetadataBlockMap({
  metadata,
}: {
  readonly metadata: MetadataAcquisitionProgress;
}) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [width, setWidth] = useState(640);
  const geometry = useMemo(
    () => blockMapGeometry(width, metadata.blockCount),
    [metadata.blockCount, width],
  );
  const counts = useMemo(() => blockStateCounts(metadata), [metadata]);

  useEffect(() => {
    const element = wrapRef.current;
    if (element === null) return;
    const measure = () => {
      const next = Math.floor(element.getBoundingClientRect().width);
      if (next > 0) setWidth(next);
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (canvas === null) return;
    const frame = requestAnimationFrame(() => {
      drawBlockMap(canvas, geometry, metadata);
    });
    return () => cancelAnimationFrame(frame);
  }, [geometry, metadata]);

  const label = `${metadata.blockCount.toLocaleString()} metadata blocks: ${counts.received.toLocaleString()} received, ${counts.requested.toLocaleString()} requested, ${counts.missing.toLocaleString()} missing`;
  return (
    <div className={styles.mapSection}>
      <div ref={wrapRef} className={styles.canvasWrap}>
        <canvas
          ref={canvasRef}
          className={styles.canvas}
          role="img"
          aria-label={label}
        />
      </div>
      <div className={styles.legend} aria-label="Metadata block state legend">
        <Legend label="Missing" state="missing" />
        <Legend label="Requested" state="requested" />
        <Legend label="Received" state="received" />
        <span>16 KiB per block</span>
      </div>
    </div>
  );
}

function blockMapGeometry(width: number, blockCount: number) {
  const boundedWidth = Math.max(CELL_SIZE, Math.floor(width));
  const stride = CELL_SIZE + CELL_GAP;
  const columns = Math.max(1, Math.floor((boundedWidth + CELL_GAP) / stride));
  const rows = Math.ceil(blockCount / columns);
  return {
    width: boundedWidth,
    height: rows * stride,
    columns,
    cellSize: CELL_SIZE,
    gap: CELL_GAP,
  };
}

function drawBlockMap(
  canvas: HTMLCanvasElement,
  geometry: ReturnType<typeof blockMapGeometry>,
  metadata: MetadataAcquisitionProgress,
) {
  const ratio = Math.min(
    MAX_DEVICE_PIXEL_RATIO,
    Math.max(1, window.devicePixelRatio || 1),
  );
  canvas.width = Math.ceil(geometry.width * ratio);
  canvas.height = Math.ceil(geometry.height * ratio);
  canvas.style.width = `${geometry.width}px`;
  canvas.style.height = `${geometry.height}px`;
  const context = canvas.getContext("2d");
  if (context === null) return;
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  context.clearRect(0, 0, geometry.width, geometry.height);
  const computed = getComputedStyle(canvas);
  const colors = [
    computed.getPropertyValue("--metadata-missing").trim() || "#edf1f5",
    computed.getPropertyValue("--metadata-requested").trim() || "#d3982e",
    computed.getPropertyValue("--metadata-received").trim() || "#2477d1",
  ];
  const stride = geometry.cellSize + geometry.gap;
  for (let index = 0; index < metadata.blockCount; index += 1) {
    const state = metadataBlockStateAt(metadata.blockStates, index);
    context.fillStyle = colors[state] ?? colors[MetadataBlockState.Missing]!;
    context.fillRect(
      (index % geometry.columns) * stride,
      Math.floor(index / geometry.columns) * stride,
      geometry.cellSize,
      geometry.cellSize,
    );
  }
}

function blockStateCounts(metadata: MetadataAcquisitionProgress) {
  const counts = { missing: 0, requested: 0, received: 0 };
  for (let index = 0; index < metadata.blockCount; index += 1) {
    switch (metadataBlockStateAt(metadata.blockStates, index)) {
      case MetadataBlockState.Requested:
        counts.requested += 1;
        break;
      case MetadataBlockState.Received:
        counts.received += 1;
        break;
      case MetadataBlockState.Missing:
        counts.missing += 1;
        break;
    }
  }
  return counts;
}

function Metric({ label, value }: { readonly label: string; readonly value: string }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}

function Legend({
  label,
  state,
}: {
  readonly label: string;
  readonly state: "missing" | "requested" | "received";
}) {
  return (
    <span>
      <i className={styles.swatch} data-state={state} aria-hidden="true" />
      {label}
    </span>
  );
}
