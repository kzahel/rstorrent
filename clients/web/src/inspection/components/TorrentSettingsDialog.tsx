import { message as localizedMessage } from "../../localization/runtime";
import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";

import type { TorrentSettingsPatch, TransferRateLimit } from "../../api";
import { useInspectionCommand, useInspectionStore } from "../context";
import type { TorrentRow } from "../model";
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
import { useSettingsDraft } from "../use-settings-draft";
import styles from "./TorrentSettingsDialog.module.css";

export function TorrentSettingsDialog({
  torrent,
  onClose,
  returnFocus,
}: {
  readonly torrent: TorrentRow;
  readonly onClose: () => void;
  readonly returnFocus: () => void;
}) {
  const cancelRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLFormElement>(null);

  useEffect(() => {
    cancelRef.current?.focus();
    return returnFocus;
  }, [returnFocus]);

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLFormElement>) => {
    if (event.key === "Escape" && !transportPending.current) {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== "Tab") return;
    const controls = Array.from(
      dialogRef.current?.querySelectorAll<HTMLElement>(
        'button:not(:disabled), input:not(:disabled)',
      ) ?? [],
    );
    const first = controls[0];
    const last = controls.at(-1);
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  };
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
      setAcceptedMessage(localizedMessage("inspection.components.detail.pane.torrent.peer.transfer.limits.saved"));
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
    <div className={styles.backdrop}>
      <form
        ref={dialogRef}
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="torrent-settings-title"
        aria-describedby="torrent-settings-description"
        onKeyDown={handleKeyDown}
        onSubmit={(event) => void submit(event)}
      >
        <h2 id="torrent-settings-title">
          {localizedMessage("inspection.components.torrent.settings.dialog.title")}
        </h2>
        <p className={styles.torrentName}>{torrent.name}</p>
        <p id="torrent-settings-description">
          {localizedMessage("inspection.components.detail.pane.these.caps.combine.with.the.all.torrents")}
        </p>
        <h3>
          {localizedMessage("inspection.components.detail.pane.peer.transfer.limits")}
        </h3>
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
          <button
            ref={cancelRef}
            type="button"
            onClick={onClose}
            disabled={transportPending.current}
          >
            {localizedMessage("inspection.components.torrent.settings.dialog.cancel")}
          </button>
          <button type="submit" disabled={patch === null || pending}>
            {pending
              ? localizedMessage("inspection.components.detail.pane.saving")
              : localizedMessage("inspection.components.detail.pane.save.torrent.limits")}
          </button>
          {status === null ? null : (
            <output aria-live="polite">{status}</output>
          )}
        </div>
      </form>
    </div>
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
  if (phase === "submitting") return localizedMessage("inspection.components.detail.pane.saving.torrent.limits");
  if (phase === "awaiting_view") return localizedMessage("inspection.components.detail.pane.saved.waiting.for.the.live.view");
  if (phase === "conflict") {
    return localizedMessage("inspection.components.detail.pane.these.limits.changed.elsewhere.your.draft.is");
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
        />{localizedMessage("inspection.components.detail.pane.unlimited")}</label>
      <label htmlFor={id}>{localizedMessage("inspection.components.detail.pane.kib.s")}</label>
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
