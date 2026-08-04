import { useMemo } from "react";

import { useInspectionStore } from "../context";
import type { SwarmRow, SwarmSet, ViewMaterialization } from "../model";
import { VirtualTable, type VirtualColumn } from "./VirtualTable";
import styles from "./SwarmTable.module.css";

const COLUMNS: readonly VirtualColumn<SwarmRow>[] = [
  {
    id: "state",
    label: "State",
    width: 132,
    sortable: true,
    sortValue: (row) => row.state,
    sortOrder: [
      "connected",
      "dialing",
      "eligible",
      "backed_off",
      "failure_limited",
      "not_connectable",
      "banned",
    ],
    render: (row) => (
      <span className={styles.state} data-state={row.state}>
        <span aria-hidden="true" />
        {formatLabel(row.state)}
      </span>
    ),
  },
  {
    id: "endpoint",
    label: "Address",
    width: 192,
    minimumWidth: 150,
    maximumWidth: 360,
    sortable: true,
    sortValue: (row) => row.endpoint,
    render: (row) => (
      <code className={styles.endpoint} title={row.endpoint}>
        {row.endpoint}
      </code>
    ),
  },
  {
    id: "sources",
    label: "Sources",
    width: 136,
    minimumViewport: 530,
    sortable: true,
    sortValue: (row) => row.sources.join(","),
    render: (row) => (
      <span title={row.sources.map(formatSource).join(", ")}>
        {row.sources.map(shortSource).join(" · ") || "—"}
      </span>
    ),
  },
  {
    id: "lastSeen",
    label: "Last seen",
    width: 92,
    minimumViewport: 650,
    align: "right",
    sortable: true,
    sortKind: "number",
    sortValue: (row) => row.lastObservedAgeMs,
    render: (row) => formatDuration(row.lastObservedAgeMs),
  },
  {
    id: "retry",
    label: "Retry",
    width: 82,
    minimumViewport: 720,
    align: "right",
    sortable: true,
    sortKind: "number",
    sortValue: (row) => row.retryInMs,
    render: (row) => formatDuration(row.retryInMs),
  },
  {
    id: "attempts",
    label: "Dials",
    width: 62,
    minimumViewport: 820,
    align: "right",
    sortable: true,
    sortKind: "number",
    sortValue: (row) => row.dialAttempts,
    render: (row) => row.dialAttempts.toLocaleString(),
  },
  {
    id: "failures",
    label: "Fails",
    width: 62,
    minimumViewport: 880,
    align: "right",
    sortable: true,
    sortKind: "number",
    sortValue: (row) => row.consecutiveFailures,
    render: (row) => row.consecutiveFailures.toLocaleString(),
  },
  {
    id: "failure",
    label: "Last failure",
    width: 112,
    defaultVisible: false,
    sortable: true,
    sortValue: (row) => row.lastFailure,
    render: (row) => (row.lastFailure === null ? "—" : formatLabel(row.lastFailure)),
  },
  {
    id: "firstSeen",
    label: "Known for",
    width: 92,
    defaultVisible: false,
    align: "right",
    sortable: true,
    sortKind: "number",
    sortValue: (row) => row.firstObservedAgeMs,
    render: (row) => formatDuration(row.firstObservedAgeMs),
  },
  {
    id: "lastDial",
    label: "Last dial",
    width: 92,
    defaultVisible: false,
    align: "right",
    sortable: true,
    sortKind: "number",
    sortValue: (row) => row.lastDialAgeMs,
    render: (row) => formatDuration(row.lastDialAgeMs),
  },
  {
    id: "trust",
    label: "Trust",
    width: 64,
    minimumViewport: 960,
    align: "right",
    sortable: true,
    sortKind: "number",
    sortValue: (row) => row.trustPoints,
    render: (row) => row.trustPoints.toLocaleString(),
  },
  {
    id: "parole",
    label: "Parole",
    width: 72,
    minimumViewport: 1_020,
    sortable: true,
    sortValue: (row) => (row.onParole ? 1 : 0),
    sortKind: "number",
    render: (row) => (row.onParole ? "Yes" : "No"),
  },
  {
    id: "integrity",
    label: "Integrity",
    width: 110,
    defaultVisible: false,
    sortable: true,
    sortValue: (row) => `${row.onParole}-${row.hashFailures}-${row.validPieces}`,
    render: (row) =>
      row.onParole
        ? `Parole · ${row.hashFailures} bad`
        : `${row.validPieces.toLocaleString()} valid`,
  },
];

export function SwarmTable({ torrentId }: { readonly torrentId: string }) {
  const swarm = useInspectionStore((state) => state.swarmByTorrent[torrentId]);
  const materialization = useInspectionStore((state) => state.viewStatus.swarm);
  const interfaceSize = useInspectionStore(
    (state) => state.presentation.interfaceSize,
  );
  const rows = useMemo(
    () =>
      (swarm?.order ?? [])
        .map((id) => swarm?.rows[id])
        .filter((row): row is SwarmRow => row !== undefined),
    [swarm],
  );

  return (
    <div className={styles.panel}>
      <SwarmSummary swarm={swarm} />
      <VirtualTable
        tableId="swarm"
        label="Known swarm peers"
        rows={rows}
        getRowId={(row) => row.recordId}
        columns={COLUMNS}
        interfaceSize={interfaceSize}
        emptyMessage={swarmEmptyMessage(materialization, swarm?.state)}
        initialSort={{ columnId: "state", direction: "asc" }}
      />
    </div>
  );
}

function SwarmSummary({ swarm }: { readonly swarm: SwarmSet | undefined }) {
  const counts = swarm?.counts;
  const total = counts?.total ?? 0;
  const capacity = swarm?.maximumRecords ?? 1_000;
  return (
    <div
      className={styles.summary}
      aria-label="Swarm registry summary"
      tabIndex={0}
    >
      <strong>{total.toLocaleString()}</strong>
      <span>known</span>
      <span className={styles.separator} aria-hidden="true" />
      <SummaryCount label="eligible" value={counts?.eligible ?? 0} tone="ready" />
      <SummaryCount
        label="not connectable"
        value={counts?.not_connectable ?? 0}
        tone="blocked"
      />
      <SummaryCount label="dialing" value={counts?.dialing ?? 0} tone="waiting" />
      <SummaryCount label="connected" value={counts?.connected ?? 0} tone="connected" />
      <SummaryCount label="backed off" value={counts?.backed_off ?? 0} tone="waiting" />
      <SummaryCount
        label="failure limited"
        value={counts?.failure_limited ?? 0}
        tone="limited"
      />
      <SummaryCount label="banned" value={counts?.banned ?? 0} tone="limited" />
      <span className={styles.capacity}>{total.toLocaleString()} / {capacity.toLocaleString()}</span>
    </div>
  );
}

function SummaryCount({
  label,
  value,
  tone,
}: {
  readonly label: string;
  readonly value: number;
  readonly tone: string;
}) {
  return (
    <span className={styles.summaryCount} data-tone={tone}>
      <span aria-hidden="true" />
      {value.toLocaleString()} {label}
    </span>
  );
}

function swarmEmptyMessage(
  materialization: ViewMaterialization,
  state: SwarmSet["state"] | undefined,
): string {
  switch (materialization.status) {
    case "not_requested":
      return "Swarm inspection is not requested.";
    case "loading":
      return "Loading known swarm peers…";
    case "unavailable":
    case "unsupported":
    case "stale":
      return materialization.reason;
    case "ready":
      return state === "inactive"
        ? "The peer registry is inactive."
        : state === "torrent_missing"
          ? "This torrent is no longer present."
          : "No peers are known yet.";
  }
}

function formatDuration(milliseconds: number | null): string {
  if (milliseconds === null) return "—";
  if (milliseconds < 1_000) return `${milliseconds}ms`;
  if (milliseconds < 60_000) return `${(milliseconds / 1_000).toFixed(1)}s`;
  if (milliseconds < 3_600_000) return `${Math.round(milliseconds / 60_000)}m`;
  return `${(milliseconds / 3_600_000).toFixed(1)}h`;
}

function formatLabel(value: string): string {
  return value.replaceAll("_", " ");
}

function shortSource(source: SwarmRow["sources"][number]): string {
  switch (source) {
    case "peer_exchange":
      return "PEX";
    case "local_discovery":
      return "LSD";
    case "magnet_hint":
      return "Magnet";
    default:
      return source.toUpperCase();
  }
}

function formatSource(source: SwarmRow["sources"][number]): string {
  return formatLabel(source);
}
