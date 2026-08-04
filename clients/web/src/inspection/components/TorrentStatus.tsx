import { useInspectionStore } from "../context";
import type { TorrentRow } from "../model";
import styles from "./TorrentTable.module.css";

export function TorrentStatus({
  row,
  label = row.status,
}: {
  readonly row: TorrentRow;
  readonly label?: string;
}) {
  const openTorrentErrorDetail = useInspectionStore(
    (state) => state.openTorrentErrorDetail,
  );

  if (row.error === null) {
    return (
      <span className={styles.status} data-status={row.status}>
        {label}
      </span>
    );
  }

  return (
    <button
      type="button"
      className={`${styles.status} ${styles.statusButton}`}
      data-status={row.status}
      title={`${row.error}\nOpen General details.`}
      aria-label={`${label}: ${row.error}. Open General details`}
      onPointerDown={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.stopPropagation();
        openTorrentErrorDetail(row.id);
      }}
    >
      <span>{label}</span>
      <span aria-hidden="true">↗</span>
    </button>
  );
}
