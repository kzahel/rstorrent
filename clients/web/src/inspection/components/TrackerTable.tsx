import { useEffect, useMemo, useState } from "react";

import { useInspectionStore } from "../context";
import type {
  TrackerRow,
  TrackerSet,
  ViewMaterialization,
} from "../model";
import { VirtualTable, type VirtualColumn } from "./VirtualTable";
import styles from "./TrackerTable.module.css";

export function TrackerTable({ torrentId }: { readonly torrentId: string }) {
  const trackerSet = useInspectionStore(
    (state) => state.trackersByTorrent[torrentId],
  );
  const materialization = useInspectionStore(
    (state) => state.viewStatus.trackers,
  );
  const interfaceSize = useInspectionStore(
    (state) => state.presentation.interfaceSize,
  );
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    setNowMs(Date.now());
    const timer = globalThis.setInterval(() => setNowMs(Date.now()), 1_000);
    return () => globalThis.clearInterval(timer);
  }, []);

  const rows = useMemo(
    () =>
      (trackerSet?.order ?? [])
        .map((id) => trackerSet?.rows[id])
        .filter((row): row is TrackerRow => row !== undefined),
    [trackerSet],
  );
  const columns = useMemo(() => trackerColumns(nowMs), [nowMs]);
  const announcing = rows.filter((row) => row.status === "announcing").length;
  const waiting = rows.filter(
    (row) => row.status === "retry_wait" || row.status === "reannounce_wait",
  ).length;

  return (
    <div className={styles.trackerPanel}>
      <div className={styles.summary}>
        <span>{rows.length.toLocaleString()} trackers</span>
        <span>{announcing.toLocaleString()} announcing</span>
        <span>{waiting.toLocaleString()} scheduled</span>
      </div>
      <VirtualTable
        tableId="trackers"
        label="Torrent trackers"
        rows={rows}
        getRowId={(row) => row.id}
        columns={columns}
        interfaceSize={interfaceSize}
        emptyMessage={trackerEmptyMessage(materialization, trackerSet?.state)}
        initialSort={{ columnId: "tier", direction: "asc" }}
      />
    </div>
  );
}

function trackerColumns(nowMs: number): readonly VirtualColumn<TrackerRow>[] {
  return [
    {
      id: "url",
      label: "URL",
      width: 220,
      minimumWidth: 170,
      maximumWidth: 760,
      sortValue: (row) => row.url,
      render: (row) => (
        <span className={styles.url} title={row.url}>
          {row.url}
        </span>
      ),
    },
    {
      id: "status",
      label: "Status",
      width: 170,
      sortValue: (row) => row.status,
      sortOrder: [
        "announcing",
        "retry_wait",
        "reannounce_wait",
        "idle",
        "inactive",
      ],
      render: (row) => (
        <span className={styles.status} data-status={row.status}>
          <span aria-hidden="true" />
          {formatStatus(row.status)}
        </span>
      ),
    },
    {
      id: "tier",
      label: "Tier",
      width: 58,
      align: "right",
      sortKind: "number",
      sortValue: (row) => row.tier,
      render: (row) => row.tier.toLocaleString(),
    },
    {
      id: "peers",
      label: "Peers",
      width: 70,
      align: "right",
      sortKind: "number",
      sortValue: (row) => row.lastPeerCount,
      render: (row) => formatCount(row.lastPeerCount),
    },
    {
      id: "seeds",
      label: "Seeds",
      width: 70,
      align: "right",
      sortKind: "number",
      sortValue: (row) => row.seeders,
      render: (row) => formatCount(row.seeders),
    },
    {
      id: "leeches",
      label: "Leeches",
      width: 76,
      align: "right",
      sortKind: "number",
      sortValue: (row) => row.leechers,
      render: (row) => formatCount(row.leechers),
    },
    {
      id: "next",
      label: "Next announce",
      width: 132,
      sortKind: "number",
      sortValue: (row) => nextDeadline(row),
      render: (row) => formatNextAction(row, nowMs),
    },
    {
      id: "error",
      label: "Error",
      width: 300,
      minimumWidth: 140,
      maximumWidth: 700,
      sortValue: (row) => row.error,
      render: (row) => (
        <span className={styles.error} title={row.error ?? undefined}>
          {row.error ?? "—"}
        </span>
      ),
    },
    {
      id: "transport",
      label: "Type",
      width: 68,
      defaultVisible: false,
      sortValue: (row) => row.transport,
      render: (row) => (
        <span
          title={
            row.security === "encrypted_unauthenticated"
              ? "Encrypted, certificate and hostname not validated"
              : "Unencrypted tracker transport"
          }
        >
          {row.transport.toUpperCase()}
        </span>
      ),
    },
    {
      id: "family",
      label: "Family",
      width: 74,
      defaultVisible: false,
      sortValue: (row) => row.lastConnectionFamily,
      render: (row) =>
        row.lastConnectionFamily === null
          ? "—"
          : row.lastConnectionFamily === "ipv4"
            ? "IPv4"
            : "IPv6",
    },
    {
      id: "source",
      label: "Source",
      width: 82,
      defaultVisible: false,
      sortValue: (row) => row.source,
      render: (row) => formatStatus(row.source),
    },
    {
      id: "event",
      label: "Event",
      width: 84,
      defaultVisible: false,
      sortValue: (row) => row.announceEvent,
      render: (row) => row.announceEvent ?? "—",
    },
    {
      id: "attempts",
      label: "Attempts",
      width: 84,
      align: "right",
      defaultVisible: false,
      sortKind: "number",
      sortValue: (row) => row.totalAttempts,
      render: (row) => row.totalAttempts.toLocaleString(),
    },
    {
      id: "failures",
      label: "Failures",
      width: 80,
      align: "right",
      defaultVisible: false,
      sortKind: "number",
      sortValue: (row) => row.consecutiveFailures,
      render: (row) => row.consecutiveFailures.toLocaleString(),
    },
    {
      id: "interval",
      label: "Interval",
      width: 90,
      align: "right",
      defaultVisible: false,
      sortKind: "number",
      sortValue: (row) => row.intervalSeconds,
      render: (row) => formatDuration(row.intervalSeconds === null ? null : row.intervalSeconds * 1_000),
    },
    {
      id: "lastSuccess",
      label: "Last success",
      width: 106,
      align: "right",
      defaultVisible: false,
      sortKind: "number",
      sortValue: (row) => row.lastSuccessAgeMs,
      render: (row) => ageLabel(row.lastSuccessAgeMs, row, nowMs),
    },
    {
      id: "lastFailure",
      label: "Last failure",
      width: 106,
      align: "right",
      defaultVisible: false,
      sortKind: "number",
      sortValue: (row) => row.lastFailureAgeMs,
      render: (row) => ageLabel(row.lastFailureAgeMs, row, nowMs),
    },
  ];
}

function nextDeadline(row: TrackerRow): number | null {
  return row.nextActionInMs === null
    ? null
    : row.observedAtMs + row.nextActionInMs;
}

function formatNextAction(row: TrackerRow, nowMs: number): string {
  if (row.status === "announcing") return "Now";
  const deadline = nextDeadline(row);
  if (deadline === null || row.nextAction === null) return "—";
  const action = row.nextAction === "retry" ? "Retry" : "Announce";
  return `${action} in ${formatDuration(Math.max(0, deadline - nowMs))}`;
}

function ageLabel(
  ageMs: number | null,
  row: TrackerRow,
  nowMs: number,
): string {
  return ageMs === null
    ? "—"
    : `${formatDuration(ageMs + Math.max(0, nowMs - row.observedAtMs))} ago`;
}

function formatDuration(milliseconds: number | null): string {
  if (milliseconds === null) return "—";
  const seconds = Math.max(0, Math.ceil(milliseconds / 1_000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  if (minutes < 60) return remainder === 0 ? `${minutes}m` : `${minutes}m ${remainder}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

function formatStatus(value: string): string {
  return value.replaceAll("_", " ");
}

function formatCount(value: number | null): string {
  return value === null ? "—" : value.toLocaleString();
}

function trackerEmptyMessage(
  materialization: ViewMaterialization,
  catalogState: TrackerSet["state"] | undefined,
): string {
  switch (materialization.status) {
    case "not_requested":
      return "Tracker inspection is not requested.";
    case "loading":
      return "Loading configured trackers…";
    case "unavailable":
    case "unsupported":
    case "stale":
      return materialization.reason;
    case "ready":
      return catalogState === "torrent_missing"
        ? "The torrent is no longer present."
        : "This torrent has no configured trackers.";
  }
}
