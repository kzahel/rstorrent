import { message as localizedMessage } from "../../localization/runtime";
import { useMemo, useRef, useState, type FormEvent } from "react";

import type {
  ActiveSeedLimit,
  AdvertisedPeerEndpointStatus,
  ClientSettings,
  ClientSettingsPatch,
  ClientSettingsRuntimeView,
  EncryptionPolicy,
  Ipv6PinholeStatus,
  ListenerBindFailureReason,
  ListenerPolicy,
  PortMappingPolicy,
  PortMappingStatus,
  TransferRateLimit,
} from "../../api";
import { useInspectionStore } from "../context";
import type { CommandResult } from "../model";
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
  rateLimitLabel,
  validateRateLimit,
  type RateLimitValidation,
} from "../transfer-rate";
import { useSettingsDraft } from "../use-settings-draft";
import styles from "./SettingsDialog.module.css";

const FIXED_PORT_MINIMUM = 1_024;
const FIXED_PORT_MAXIMUM = 65_535;
const PEER_LIMIT_MINIMUM = 1;
const PEER_LIMIT_MAXIMUM = 2_000;
const UPLOAD_SLOTS_MINIMUM = 0;
const UPLOAD_SLOTS_MAXIMUM = 50;
const ACTIVE_DOWNLOADS_MINIMUM = 1;
const ACTIVE_DOWNLOADS_MAXIMUM = 20;
const ACTIVE_SEEDS_MINIMUM = 0;
const ACTIVE_SEEDS_MAXIMUM = 500;
const SEED_GOAL_MINIMUM = 0;
const SEED_GOAL_MAXIMUM = 2_147_483_647;
const DEFAULT_UPLOAD_RATE_KIB = "1024";
const DEFAULT_DOWNLOAD_RATE_KIB = "4096";

type ListenerMode = "automatic" | "fixed";

interface ConnectionSeedingSettingsSectionProps {
  readonly settings: ClientSettingsRuntimeView;
  readonly manageable: boolean;
  readonly onSave: (patch: ClientSettingsPatch) => Promise<CommandResult>;
}

interface DraftValidation {
  readonly settings: ClientSettings | null;
  readonly preferredPortError: string | null;
  readonly fixedPortError: string | null;
  readonly peerLimitError: string | null;
  readonly uploadSlotsError: string | null;
  readonly activeDownloadsError: string | null;
  readonly activeSeedsError: string | null;
  readonly shareRatioError: string | null;
  readonly finishedDownloadRatioError: string | null;
  readonly finishedTimeError: string | null;
  readonly uploadRateError: string | null;
  readonly downloadRateError: string | null;
}

export function ConnectionSeedingSettingsSection({
  settings,
  manageable,
  onSave,
}: ConnectionSeedingSettingsSectionProps) {
  const configured = settings.configured;
  const durableRevision = useInspectionStore((state) => state.durableRevision);
  const authority = clientSettingsDraft(configured);
  const [draftState, dispatchDraft] = useSettingsDraft(
    "client-settings",
    durableRevision,
    authority,
    CLIENT_SETTINGS_COMPARATORS,
  );
  const draft = settingsDraftValue(draftState) ?? authority;
  const transportPending = useRef(false);
  const [acceptedMessage, setAcceptedMessage] = useState<string | null>(null);
  const phase = settingsDraftPhase(draftState);
  const pending = phase === "submitting" || phase === "awaiting_view";

  const validation = useMemo(
    () => {
      const uploadRate = validateRateLimit(
        draft.uploadRate.unlimited,
        draft.uploadRate.valueKiB,
      );
      const downloadRate = validateRateLimit(
        draft.downloadRate.unlimited,
        draft.downloadRate.valueKiB,
      );
      return validateDraft(
        draft.listener.mode,
        draft.preferredPort,
        draft.listener.fixedPort,
        draft.portMapping,
        draft.peerLimit,
        draft.uploadSlots,
        draft.activeDownloads,
        draft.activeSeeds,
        draft.shareRatioLimit,
        draft.finishedDownloadRatioLimit,
        draft.finishedTimeLimit,
        uploadRate,
        downloadRate,
        draft.encryption,
        draft.ipv6Enabled,
        configured.tracker_https_server_authentication,
      );
    },
    [configured.tracker_https_server_authentication, draft],
  );
  const dirtyFields = settingsDraftFields(draftState);
  const patch = clientSettingsPatch(dirtyFields, validation.settings);

  const resetDraft = () => {
    dispatchDraft({ type: "discard" });
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (
      !manageable ||
      patch === null ||
      transportPending.current ||
      draftState.submission !== null
    ) {
      return;
    }
    transportPending.current = true;
    setAcceptedMessage(null);
    dispatchDraft({ type: "submit" });
    try {
      const result = await onSave(patch);
      if (result.resultingRevision === undefined) {
        throw new Error("Settings response did not include a durable revision.");
      }
      setAcceptedMessage(localizedMessage("inspection.components.connection.seeding.settings.section.settings.accepted.and.applying"));
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
    clientDraftStatus(draftState, phase) ??
    (phase === "pristine" ? acceptedMessage : null);

  return (
    <fieldset className={styles.section}>
      <legend>{localizedMessage("inspection.components.connection.seeding.settings.section.connection.seeding")}</legend>
      {!manageable ? (
        <p className={styles.storageNote}>{localizedMessage("inspection.components.connection.seeding.settings.section.connection.and.seeding.settings.are.managed.by")}</p>
      ) : null}
      <p className={styles.sectionIntroduction}>{localizedMessage("inspection.components.connection.seeding.settings.section.incoming.peer.connections.use.ipv4.and.when")}</p>
      <form className={styles.settingsForm} onSubmit={(event) => void submit(event)}>
        <div
          className={styles.settingGroup}
          role="group"
          aria-labelledby="listener-policy-heading"
        >
          <div className={styles.settingHeading}>
            <strong id="listener-policy-heading">{localizedMessage("inspection.components.connection.seeding.settings.section.incoming.tcp.listener")}</strong>
            <span>{localizedMessage("inspection.components.connection.seeding.settings.section.choose.automatic.or.fixed.port.selection")}</span>
          </div>
          <div className={styles.options}>
            <ListenerOption
              mode="automatic"
              label={localizedMessage("inspection.components.connection.seeding.settings.section.automatic.port")}
              description="Choose an available TCP and UDP port automatically."
              selected={draft.listener.mode === "automatic"}
              disabled={!manageable}
              onSelect={() =>
                dispatchDraft({
                  type: "edit",
                  field: "listener",
                  value: { ...draft.listener, mode: "automatic" },
                })
              }
            />
            <ListenerOption
              mode="fixed"
              label={localizedMessage("inspection.components.connection.seeding.settings.section.fixed.port")}
              description="Always use the exact port entered below."
              selected={draft.listener.mode === "fixed"}
              disabled={!manageable}
              onSelect={() =>
                dispatchDraft({
                  type: "edit",
                  field: "listener",
                  value: { ...draft.listener, mode: "fixed" },
                })
              }
            />
          </div>
          {draft.listener.mode === "fixed" ? (
            <NumberField
              id="fixed-listener-port"
              label={localizedMessage("inspection.components.connection.seeding.settings.section.fixed.listener.port")}
              value={draft.listener.fixedPort}
              minimum={FIXED_PORT_MINIMUM}
              maximum={FIXED_PORT_MAXIMUM}
              error={validation.fixedPortError}
              disabled={!manageable}
              onChange={(fixedPort) =>
                dispatchDraft({
                  type: "edit",
                  field: "listener",
                  value: { ...draft.listener, fixedPort },
                })
              }
            />
          ) : null}
        </div>

        <label className={styles.option}>
          <input
            type="checkbox"
            checked={draft.ipv6Enabled}
            disabled={!manageable}
            onChange={(event) =>
              dispatchDraft({
                type: "edit",
                field: "ipv6Enabled",
                value: event.currentTarget.checked,
              })
            }
          />
          <span>
            <strong>{localizedMessage("inspection.components.connection.seeding.settings.section.enable.ipv6")}</strong>
            <small>{localizedMessage("inspection.components.connection.seeding.settings.section.use.ipv6.for.dht.trackers.peer.connections")}</small>
          </span>
        </label>

        <label className={styles.option}>
          <input
            type="checkbox"
            checked={draft.portMapping === "upnp"}
            disabled={!manageable}
            onChange={(event) =>
              dispatchDraft({
                type: "edit",
                field: "portMapping",
                value: event.currentTarget.checked ? "upnp" : "disabled",
              })
            }
          />
          <span>
            <strong>{localizedMessage("inspection.components.connection.seeding.settings.section.map.incoming.tcp.and.utp.with.upnp")}</strong>
            <small>{localizedMessage("inspection.components.connection.seeding.settings.section.request.independent.temporary.tcp.and.udp.igd")}</small>
          </span>
        </label>

        <NumberField
          id="active-downloads"
          label={localizedMessage("inspection.components.connection.seeding.settings.section.simultaneous.downloads")}
          description="Incomplete torrents admitted at once. Additional runnable torrents remain queued and start automatically as capacity opens."
          value={draft.activeDownloads}
          minimum={ACTIVE_DOWNLOADS_MINIMUM}
          maximum={ACTIVE_DOWNLOADS_MAXIMUM}
          error={validation.activeDownloadsError}
          disabled={!manageable}
          onChange={(value) =>
            dispatchDraft({ type: "edit", field: "activeDownloads", value })
          }
        />

        <div
          className={styles.settingGroup}
          role="group"
          aria-labelledby="seeding-priority-heading"
        >
          <div className={styles.settingHeading}>
            <strong id="seeding-priority-heading">{localizedMessage("inspection.components.connection.seeding.settings.section.automatic.seeding.priority")}</strong>
            <span>{localizedMessage("inspection.components.connection.seeding.settings.section.goals.rank.completed.torrents.when.active.seed")}</span>
          </div>
          <label className={styles.option}>
            <input
              type="checkbox"
              checked={draft.activeSeeds.unlimited}
              disabled={!manageable}
              onChange={(event) =>
                dispatchDraft({
                  type: "edit",
                  field: "activeSeeds",
                  value: {
                    ...draft.activeSeeds,
                    unlimited: event.currentTarget.checked,
                  },
                })
              }
            />
            <span>
              <strong>{localizedMessage("inspection.components.connection.seeding.settings.section.unlimited.active.seeds")}</strong>
              <small>{localizedMessage("inspection.components.connection.seeding.settings.section.remove.the.seed.only.limit.the.fixed")}</small>
            </span>
          </label>
          <NumberField
            id="active-seeds"
            label={localizedMessage("inspection.components.connection.seeding.settings.section.active.seeds")}
            description="Completed torrents counted in the active seed queue. Zero keeps eligible seeds queued."
            value={draft.activeSeeds.torrents}
            minimum={ACTIVE_SEEDS_MINIMUM}
            maximum={ACTIVE_SEEDS_MAXIMUM}
            error={validation.activeSeedsError}
            disabled={!manageable || draft.activeSeeds.unlimited}
            onChange={(torrents) =>
              dispatchDraft({
                type: "edit",
                field: "activeSeeds",
                value: { ...draft.activeSeeds, torrents },
              })
            }
          />
          <NumberField
            id="share-ratio-limit"
            label={localizedMessage("inspection.components.connection.seeding.settings.section.share.ratio.priority.goal")}
            description="Uploaded payload as a percentage of the larger of downloaded payload or full torrent size."
            value={draft.shareRatioLimit}
            minimum={SEED_GOAL_MINIMUM}
            maximum={SEED_GOAL_MAXIMUM}
            error={validation.shareRatioError}
            disabled={!manageable}
            onChange={(value) =>
              dispatchDraft({ type: "edit", field: "shareRatioLimit", value })
            }
          />
          <NumberField
            id="finished-download-ratio-limit"
            label={localizedMessage("inspection.components.connection.seeding.settings.section.finished.download.time.priority.goal")}
            description="Finished active time as a percentage of active download time."
            value={draft.finishedDownloadRatioLimit}
            minimum={SEED_GOAL_MINIMUM}
            maximum={SEED_GOAL_MAXIMUM}
            error={validation.finishedDownloadRatioError}
            disabled={!manageable}
            onChange={(value) =>
              dispatchDraft({
                type: "edit",
                field: "finishedDownloadRatioLimit",
                value,
              })
            }
          />
          <NumberField
            id="finished-time-limit"
            label={localizedMessage("inspection.components.connection.seeding.settings.section.finished.time.priority.goal.seconds")}
            description="Cumulative active finished time. Reaching any one priority goal marks the seeding goal met."
            value={draft.finishedTimeLimit}
            minimum={SEED_GOAL_MINIMUM}
            maximum={SEED_GOAL_MAXIMUM}
            error={validation.finishedTimeError}
            disabled={!manageable}
            onChange={(value) =>
              dispatchDraft({ type: "edit", field: "finishedTimeLimit", value })
            }
          />
        </div>

        <div
          className={styles.settingGroup}
          role="group"
          aria-labelledby="peer-transfer-limits-heading"
        >
          <div className={styles.settingHeading}>
            <strong id="peer-transfer-limits-heading">{localizedMessage("inspection.components.connection.seeding.settings.section.all.torrents.peer.transfer.limits")}</strong>
            <span>{localizedMessage("inspection.components.connection.seeding.settings.section.caps.established.bittorrent.peer.traffic.across.the")}</span>
          </div>
          <RateLimitField
            id="all-torrents-upload-rate"
            label={localizedMessage("inspection.components.connection.seeding.settings.section.all.torrents.upload.limit")}
            unlimited={draft.uploadRate.unlimited}
            valueKiB={draft.uploadRate.valueKiB}
            error={validation.uploadRateError}
            disabled={!manageable}
            onUnlimitedChange={(unlimited) =>
              dispatchDraft({
                type: "edit",
                field: "uploadRate",
                value: { ...draft.uploadRate, unlimited },
              })
            }
            onValueChange={(valueKiB) =>
              dispatchDraft({
                type: "edit",
                field: "uploadRate",
                value: { ...draft.uploadRate, valueKiB },
              })
            }
          />
          <RateLimitField
            id="all-torrents-download-rate"
            label={localizedMessage("inspection.components.connection.seeding.settings.section.all.torrents.download.limit")}
            unlimited={draft.downloadRate.unlimited}
            valueKiB={draft.downloadRate.valueKiB}
            error={validation.downloadRateError}
            disabled={!manageable}
            onUnlimitedChange={(unlimited) =>
              dispatchDraft({
                type: "edit",
                field: "downloadRate",
                value: { ...draft.downloadRate, unlimited },
              })
            }
            onValueChange={(valueKiB) =>
              dispatchDraft({
                type: "edit",
                field: "downloadRate",
                value: { ...draft.downloadRate, valueKiB },
              })
            }
          />
        </div>

        <NumberField
          id="peer-connection-limit"
          label={localizedMessage("inspection.components.connection.seeding.settings.section.peer.connection.limit")}
          description="Ordinary outgoing, established, and accepted peer connections across the session. The running process may use a lower safe limit."
          value={draft.peerLimit}
          minimum={PEER_LIMIT_MINIMUM}
          maximum={PEER_LIMIT_MAXIMUM}
          error={validation.peerLimitError}
          disabled={!manageable}
          onChange={(value) =>
            dispatchDraft({ type: "edit", field: "peerLimit", value })
          }
        />

        <div
          className={styles.settingGroup}
          role="group"
          aria-labelledby="encryption-policy-heading"
        >
          <div className={styles.settingHeading}>
            <strong id="encryption-policy-heading">{localizedMessage("inspection.components.connection.seeding.settings.section.protocol.obfuscation.mse.pe")}</strong>
            <span>{localizedMessage("inspection.components.connection.seeding.settings.section.improves.compatibility.with.peers.that.require.mse")}</span>
          </div>
          <div className={styles.options}>
            {ENCRYPTION_OPTIONS.map((option) => (
              <label className={styles.option} key={option.value}>
                <input
                  type="radio"
                  name="encryption-policy"
                  value={option.value}
                  checked={draft.encryption === option.value}
                  disabled={!manageable}
                  onChange={() =>
                    dispatchDraft({
                      type: "edit",
                      field: "encryption",
                      value: option.value,
                    })
                  }
                />
                <span>
                  <strong>{option.label}</strong>
                  <small>{option.description}</small>
                </span>
              </label>
            ))}
          </div>
        </div>

        <NumberField
          id="upload-slots"
          label={localizedMessage("inspection.components.connection.seeding.settings.section.payload.upload.slots")}
          description="Peers allowed to receive piece payload at once. Zero keeps interested peers choked for piece payload; metadata and the listener remain available."
          value={draft.uploadSlots}
          minimum={UPLOAD_SLOTS_MINIMUM}
          maximum={UPLOAD_SLOTS_MAXIMUM}
          error={validation.uploadSlotsError}
          disabled={!manageable}
          onChange={(value) =>
            dispatchDraft({ type: "edit", field: "uploadSlots", value })
          }
        />

        <RuntimeState settings={settings} />

        <div className={styles.formActions}>
          <button
            className={styles.primaryAction}
            type="submit"
            disabled={!manageable || pending || patch === null}
          >
            {pending ? localizedMessage("inspection.components.connection.seeding.settings.section.saving") : localizedMessage("inspection.components.connection.seeding.settings.section.save.settings")}
          </button>
          <button
            className={styles.secondaryAction}
            type="button"
            disabled={!manageable || dirtyFields.length === 0}
            onClick={resetDraft}
          >{localizedMessage("inspection.components.connection.seeding.settings.section.cancel.changes")}</button>
        </div>
        {status === null ? null : (
          <output
            className={
              phase === "failed" || phase === "conflict"
                ? styles.errorStatus
                : styles.successStatus
            }
            role={phase === "failed" || phase === "conflict" ? "alert" : "status"}
            aria-live="polite"
          >
            {status}
          </output>
        )}
      </form>
    </fieldset>
  );
}

interface ListenerOptionProps {
  readonly mode: ListenerMode;
  readonly label: string;
  readonly description: string;
  readonly selected: boolean;
  readonly disabled: boolean;
  readonly onSelect: () => void;
}

function ListenerOption({
  mode,
  label,
  description,
  selected,
  disabled,
  onSelect,
}: ListenerOptionProps) {
  return (
    <label className={styles.option}>
      <input
        type="radio"
        name="listener-policy"
        value={mode}
        checked={selected}
        disabled={disabled}
        onChange={onSelect}
      />
      <span>
        <strong>{label}</strong>
        <small>{description}</small>
      </span>
    </label>
  );
}

interface NumberFieldProps {
  readonly id: string;
  readonly label: string;
  readonly description?: string;
  readonly value: string;
  readonly minimum: number;
  readonly maximum: number;
  readonly error: string | null;
  readonly disabled: boolean;
  readonly onChange: (value: string) => void;
}

function NumberField({
  id,
  label,
  description,
  value,
  minimum,
  maximum,
  error,
  disabled,
  onChange,
}: NumberFieldProps) {
  const descriptionId = `${id}-description`;
  const errorId = `${id}-error`;
  return (
    <label className={styles.numberField} htmlFor={id}>
      <span>
        <strong>{label}</strong>
        {description === undefined ? null : (
          <small id={descriptionId}>{description}</small>
        )}
      </span>
      <input
        id={id}
        aria-label={label}
        type="number"
        inputMode="numeric"
        step={1}
        min={minimum}
        max={maximum}
        required
        value={value}
        disabled={disabled}
        aria-invalid={error !== null}
        aria-describedby={
          [description === undefined ? null : descriptionId, error === null ? null : errorId]
            .filter((part): part is string => part !== null)
            .join(" ") || undefined
        }
        onChange={(event) => onChange(event.currentTarget.value)}
      />
      {error === null ? null : (
        <small id={errorId} className={styles.fieldError}>
          {error}
        </small>
      )}
    </label>
  );
}

interface RateLimitFieldProps {
  readonly id: string;
  readonly label: string;
  readonly unlimited: boolean;
  readonly valueKiB: string;
  readonly error: string | null;
  readonly disabled: boolean;
  readonly onUnlimitedChange: (unlimited: boolean) => void;
  readonly onValueChange: (value: string) => void;
}

function RateLimitField({
  id,
  label,
  unlimited,
  valueKiB,
  error,
  disabled,
  onUnlimitedChange,
  onValueChange,
}: RateLimitFieldProps) {
  const descriptionId = `${id}-description`;
  const errorId = `${id}-error`;
  return (
    <div className={styles.numberField}>
      <span>
        <strong>{label}</strong>
        <small id={descriptionId}>{localizedMessage("inspection.components.connection.seeding.settings.section.kib.s.of.established.peer.traffic")}</small>
      </span>
      <label className={styles.option}>
        <input
          type="checkbox"
          aria-label={`${label} unlimited`}
          checked={unlimited}
          disabled={disabled}
          onChange={(event) => onUnlimitedChange(event.currentTarget.checked)}
        />
        <span><strong>{localizedMessage("inspection.components.connection.seeding.settings.section.unlimited")}</strong></span>
      </label>
      <input
        id={id}
        aria-label={`${label} in KiB per second`}
        type="number"
        inputMode="decimal"
        min={1}
        max={RATE_LIMIT_MAXIMUM_BYTES / 1_024}
        step={1 / 1_024}
        required={!unlimited}
        value={valueKiB}
        disabled={disabled || unlimited}
        aria-invalid={error !== null}
        aria-describedby={`${descriptionId}${error === null ? "" : ` ${errorId}`}`}
        onChange={(event) => onValueChange(event.currentTarget.value)}
      />
      {error === null ? null : (
        <small id={errorId} className={styles.fieldError}>{error}</small>
      )}
    </div>
  );
}

function RuntimeState({ settings }: { readonly settings: ClientSettingsRuntimeView }) {
  const listener = settings.listener_status;
  return (
    <div className={styles.runtimeState} aria-label={localizedMessage("inspection.components.connection.seeding.settings.section.current.runtime.state")}>
      <strong>{localizedMessage("inspection.components.connection.seeding.settings.section.current.runtime")}</strong>
      {listener.type === "disabled" ? (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.incoming.tcp.is.not.running.save.settings")}</span>
      ) : listener.type === "listening" ? (
        settings.effective_listener != null &&
        isLoopbackListener(settings.effective_listener.listener) ? (
          <span>{localizedMessage("inspection.components.connection.seeding.settings.section.incoming.tcp.is.using.a.development.only")}{" "}{listener.port}{localizedMessage("inspection.components.connection.seeding.settings.section.save.settings.to.listen.on.all.ipv4")}</span>
        ) : (
          <span>{localizedMessage("inspection.components.connection.seeding.settings.section.incoming.tcp.is.listening.on.all.ipv4")}{" "}{listener.port}.</span>
        )
      ) : (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.listener.could.not.start")}{bindFailureLabel(listener.reason)}):{" "}
          {listener.detail}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.save.settings.to.retry")}</span>
      )}
      {settings.session_udp_status.type === "bound" ? (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.session.udp.is.bound.on")}{" "}{settings.session_udp_status.address}:
          {settings.session_udp_status.port}
          {settings.session_udp_status.coordinated_with_tcp
            ? localizedMessage("inspection.components.connection.seeding.settings.section.with.the.tcp.listener")
            : localizedMessage("inspection.components.connection.seeding.settings.section.independently.from.tcp")}
        </span>
      ) : (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.session.udp.is.not.available.in.this")}</span>
      )}
      {settings.effective_listener == null ? (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.no.incoming.listener.policy.has.converged.yet")}</span>
      ) : (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.effective.listener.policy")}{" "}{listenerPolicyLabel(settings.effective_listener.listener)}.
        </span>
      )}
      <span>{localizedMessage("inspection.components.connection.seeding.settings.section.effective.gateway.mapping.policy")}{" "}{settings.effective_port_mapping === "upnp" ? localizedMessage("inspection.components.connection.seeding.settings.section.upnp.igd.v2") : localizedMessage("inspection.components.connection.seeding.settings.section.off")}.
      </span>
      <PortMappingRuntime label={localizedMessage("inspection.components.connection.seeding.settings.section.tcp")} status={settings.port_mapping_status} />
      <PortMappingRuntime
        label={localizedMessage("inspection.components.connection.seeding.settings.section.udp.utp")}
        status={settings.udp_port_mapping_status}
      />
      <Ipv6PinholeRuntime status={settings.ipv6_pinhole_status} />
      <AdvertisedEndpointRuntime status={settings.advertised_peer_endpoint} />
      {settings.transport_families.map((family) => (
        <span key={family.family}>
          {family.family === "ipv4" ? localizedMessage("inspection.components.connection.seeding.settings.section.ipv4") : localizedMessage("inspection.components.connection.seeding.settings.section.ipv6")}: {family.configured ? localizedMessage("inspection.components.connection.seeding.settings.section.configured") : localizedMessage("inspection.components.connection.seeding.settings.section.disabled")}
          {family.tcp_endpoint === null ? "" : ` · TCP ${family.tcp_endpoint}`}
          {family.udp_endpoint === null ? "" : ` · UDP ${family.udp_endpoint}`}
          {family.advertised_endpoint === null
            ? localizedMessage("inspection.components.connection.seeding.settings.section.outbound.only.advertisement")
            : ` · advertises ${family.advertised_endpoint}`}
        </span>
      ))}
      <ApplicationState label={localizedMessage("inspection.components.connection.seeding.settings.section.transport")} state={settings.transport_application} />
      <ApplicationState
        label={localizedMessage("inspection.components.connection.seeding.settings.section.port.mapping")}
        state={settings.port_mapping_application}
      />
      <ApplicationState
        label={localizedMessage("inspection.components.connection.seeding.settings.section.peer.connections")}
        state={settings.peer_connections_application}
      />
      <ApplicationState
        label={localizedMessage("inspection.components.connection.seeding.settings.section.upload.slots")}
        state={settings.upload_slots_application}
      />
      <ApplicationState label={localizedMessage("inspection.components.connection.seeding.settings.section.peer.transfer.limits")} state={settings.bandwidth_application} />
      <ApplicationState
        label={localizedMessage("inspection.components.connection.seeding.settings.section.protocol.obfuscation")}
        state={settings.encryption_application}
      />
      {settings.effective_peer_connection_limit <
      settings.configured.peer_connection_limit ? (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.the.configured")}{" "}{settings.configured.peer_connection_limit}{localizedMessage("inspection.components.connection.seeding.settings.section.peer.setting.is.safely.limited.to")}{" "}{settings.effective_peer_connection_limit}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.by.available.file.descriptors")}</span>
      ) : (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.effective.peer.connection.limit")}{" "}
          {settings.effective_peer_connection_limit}.
        </span>
      )}
      <span>{localizedMessage("inspection.components.connection.seeding.settings.section.effective.payload.upload.slots")}{" "}{settings.effective_upload_slots}.</span>
      <span>{localizedMessage("inspection.components.connection.seeding.settings.section.effective.peer.upload.limit")}{" "}{rateLimitLabel(settings.effective_upload_rate_limit)}.
      </span>
      <span>{localizedMessage("inspection.components.connection.seeding.settings.section.effective.peer.download.limit")}{" "}{rateLimitLabel(settings.effective_download_rate_limit)}.
      </span>
      <span>{localizedMessage("inspection.components.connection.seeding.settings.section.active.downloads")}{" "}{settings.active_download_count}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.of")}{" "}{settings.effective_active_downloads}
        {settings.checking_count === 0
          ? "."
          : ` · ${settings.checking_count} checking.`}
      </span>
      <span>{localizedMessage("inspection.components.connection.seeding.settings.section.active.seeds.604a98b")}{" "}{settings.active_seed_count}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.counted.of")}{" "}{activeSeedLimitLabel(settings.effective_active_seeds)}
        {settings.inactive_seed_count === 0
          ? "."
          : ` · ${settings.inactive_seed_count} active but inactive-exempt.`}
      </span>
      {settings.active_downloads_clamp_reason === "platform_limit" ? (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.the.configured")}{" "}{settings.configured.active_downloads}{localizedMessage("inspection.components.connection.seeding.settings.section.download.setting.is.limited.to")}{" "}{settings.effective_active_downloads}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.on.this.platform")}</span>
      ) : null}
      <span>{localizedMessage("inspection.components.connection.seeding.settings.section.effective.protocol.obfuscation.policy")}{" "}{settings.effective_encryption}.
      </span>
      <span>{localizedMessage("inspection.components.connection.seeding.settings.section.effective.ipv6.policy")}{" "}{settings.effective_ipv6_enabled ? localizedMessage("inspection.components.connection.seeding.settings.section.enabled") : localizedMessage("inspection.components.connection.seeding.settings.section.disabled")}.
      </span>
    </div>
  );
}

function activeSeedLimitLabel(limit: ActiveSeedLimit): string {
  return limit.type === "unlimited" ? "Unlimited" : String(limit.torrents);
}

function listenerPolicyLabel(listener: ListenerPolicy): string {
  switch (listener.type) {
    case "disabled":
      return localizedMessage("inspection.components.connection.seeding.settings.section.development.only.disabled.mode");
    case "automatic_loopback":
      return localizedMessage("inspection.components.connection.seeding.settings.section.development.only.loopback.mode");
    case "fixed_loopback":
      return `development-only loopback port ${listener.port}`;
    case "automatic_local_network":
      return localizedMessage("inspection.components.connection.seeding.settings.section.automatic.port.c61a158");
    case "fixed_local_network":
      return `fixed port ${listener.port}`;
  }
}

function ApplicationState({
  label,
  state,
}: {
  readonly label: string;
  readonly state: ClientSettingsRuntimeView["transport_application"];
}) {
  if (state.type === "applied") return null;
  if (state.type === "applying") {
    return <span>{label}{localizedMessage("inspection.components.connection.seeding.settings.section.applying")}</span>;
  }
  return (
    <span className={styles.runtimeWarning}>
      {label}{localizedMessage("inspection.components.connection.seeding.settings.section.degraded")}{state.detail}{localizedMessage("inspection.components.connection.seeding.settings.section.save.settings.to.retry.0312e9f")}</span>
  );
}

function AdvertisedEndpointRuntime({
  status,
}: {
  readonly status: AdvertisedPeerEndpointStatus;
}) {
  switch (status.type) {
    case "unavailable":
      return <span>{localizedMessage("inspection.components.connection.seeding.settings.section.peer.advertisement.endpoint.is.not.available.yet")}</span>;
    case "outbound_only":
      return (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.tracker.discovery.is.outbound.only.no.connectable")}</span>
      );
    case "local":
      return (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.peer.protocols.may.advertise.local.tcp.port")}{" "}{status.port}{localizedMessage("inspection.components.connection.seeding.settings.section.external.reachability.is.unverified")}{status.incoming_observed ? localizedMessage("inspection.components.connection.seeding.settings.section.but.an.incoming.peer.was.observed") : "."}
        </span>
      );
    case "mapped":
      return (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.peer.protocols.may.advertise.mapped.tcp.port")}{" "}{status.external_port}
          {status.incoming_observed
            ? localizedMessage("inspection.components.connection.seeding.settings.section.an.incoming.peer.was.observed")
            : localizedMessage("inspection.components.connection.seeding.settings.section.external.reachability.is.not.yet.observed")}
        </span>
      );
    case "renewal_unhealthy":
      return (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.mapped.tcp.port")}{" "}{status.external_port}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.remains.valid.for.up.to")}{" "}
          {status.lease_seconds_remaining}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.seconds.but.renewal.is.unhealthy")}{" "}
          {status.detail}
        </span>
      );
    case "stopping":
      return <span>{localizedMessage("inspection.components.connection.seeding.settings.section.peer.advertisement.is.stopping")}</span>;
  }
}

function PortMappingRuntime({
  label,
  status,
}: {
  readonly label: string;
  readonly status: PortMappingStatus;
}) {
  switch (status.type) {
    case "disabled":
      return <span>{localizedMessage("inspection.components.connection.seeding.settings.section.automatic")}{" "}{label}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.gateway.mapping.is.off")}</span>;
    case "ineligible":
      return (
        <span className={styles.runtimeWarning}>
          {label}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.gateway.mapping.requires.an.active.listener.and")}</span>
      );
    case "discovering":
      return <span>{localizedMessage("inspection.components.connection.seeding.settings.section.discovering.a.compatible.upnp.igd.v2.gateway")}{" "}{label}…</span>;
    case "mapping":
      return <span>{localizedMessage("inspection.components.connection.seeding.settings.section.requesting.and.verifying.the.temporary")}{" "}{label}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.mapping")}</span>;
    case "mapped":
      return (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.upnp")}{" "}{label}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.mapped")}{" "}{status.external_address}:{status.external_port}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.to")}{" "}
          {status.local_address}:{status.local_port}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.for")}{" "}{status.lease_seconds}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.seconds")}</span>
      );
    case "failed":
      return (
        <span className={styles.runtimeWarning}>
          {label}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.gateway.mapping.failed.during")}{" "}{status.stage.replaceAll("_", " ")}: {status.detail}
        </span>
      );
    case "renewal_failed":
      return (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.the")}{" "}{label}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.mapping.at")}{" "}{status.external_address}:{status.external_port}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.could.not.be.renewed")}{" "}{status.detail}
        </span>
      );
    case "cleanup_failed":
      return (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.the.old")}{" "}{label}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.mapping.at")}{" "}{status.external_address}:{status.external_port}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.could.not.be.confirmed.removed.and.may")}{" "}{status.remaining_lease_seconds}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.seconds.a157a77")}{" "}{status.detail}
        </span>
      );
    case "stopping":
      return <span>{localizedMessage("inspection.components.connection.seeding.settings.section.removing.the")}{" "}{label}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.gateway.mapping")}</span>;
  }
}

function Ipv6PinholeRuntime({ status }: { readonly status: Ipv6PinholeStatus }) {
  switch (status.type) {
    case "disabled":
      return <span>{localizedMessage("inspection.components.connection.seeding.settings.section.automatic.ipv6.firewall.pinhole.control.is.off")}</span>;
    case "ineligible":
      return (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.ipv6.pinhole.control.requires.a.global.unicast")}</span>
      );
    case "discovering":
      return <span>{localizedMessage("inspection.components.connection.seeding.settings.section.discovering.ipv6.firewall.control.on.the.upnp")}</span>;
    case "service_unavailable":
      return (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.the.gateway.does.not.advertise.ipv6.firewall")}</span>
      );
    case "action_unavailable":
      return (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.the.gateway.ipv6.firewall.service.is.incomplete")}{" "}{status.detail}
        </span>
      );
    case "inbound_pinhole_disallowed":
      return (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.the.gateway.firewall.is.enabled.but.does")}</span>
      );
    case "unfiltered":
      return (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.the.gateway.reports.ipv6.filtering.disabled.for")}{" "}{status.internal_address}:
          {status.internal_port}{localizedMessage("inspection.components.connection.seeding.settings.section.this.is.gateway.state.not.an.observed")}</span>
      );
    case "creating":
      return (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.requesting.an.ipv6.firewall.pinhole.for")}{" "}{status.internal_address}:
          {status.internal_port}…
        </span>
      );
    case "pinholed":
      return (
        <span>{localizedMessage("inspection.components.connection.seeding.settings.section.the.gateway.accepted.an.ipv6.pinhole.for")}{" "}{status.internal_address}:
          {status.internal_port}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.for")}{" "}{status.lease_seconds}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.seconds.this.does.not.mean.an.incoming")}</span>
      );
    case "failed":
      return (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.ipv6.pinhole.control.failed.during")}{" "}{status.stage.replaceAll("_", " ")}: {status.detail}
        </span>
      );
    case "renewal_failed":
      return (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.the.ipv6.pinhole.for")}{" "}{status.internal_address}:{status.internal_port}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.could.not.be.renewed")}{" "}{status.detail}
        </span>
      );
    case "cleanup_failed":
      return (
        <span className={styles.runtimeWarning}>{localizedMessage("inspection.components.connection.seeding.settings.section.the.old.ipv6.pinhole.for")}{" "}{status.internal_address}:{status.internal_port}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.could.not.be.confirmed.removed.and.may")}{" "}{status.remaining_lease_seconds}{" "}{localizedMessage("inspection.components.connection.seeding.settings.section.seconds.a157a77")}{" "}{status.detail}
        </span>
      );
    case "stopping":
      return <span>{localizedMessage("inspection.components.connection.seeding.settings.section.removing.the.ipv6.firewall.pinhole")}</span>;
  }
}

interface RateLimitDraftField {
  readonly unlimited: boolean;
  readonly valueKiB: string;
}

interface ActiveSeedDraftField {
  readonly unlimited: boolean;
  readonly torrents: string;
}

interface ListenerDraftField {
  readonly mode: ListenerMode;
  readonly fixedPort: string;
}

interface ClientSettingsDraft {
  readonly listener: ListenerDraftField;
  readonly preferredPort: string;
  readonly portMapping: PortMappingPolicy;
  readonly peerLimit: string;
  readonly uploadSlots: string;
  readonly activeDownloads: string;
  readonly activeSeeds: ActiveSeedDraftField;
  readonly shareRatioLimit: string;
  readonly finishedDownloadRatioLimit: string;
  readonly finishedTimeLimit: string;
  readonly uploadRate: RateLimitDraftField;
  readonly downloadRate: RateLimitDraftField;
  readonly encryption: EncryptionPolicy;
  readonly ipv6Enabled: boolean;
}

const CLIENT_SETTINGS_COMPARATORS: SettingsDraftComparators<ClientSettingsDraft> = {
  listener: (left, right) =>
    left.mode === right.mode &&
    (left.mode === "automatic" || left.fixedPort === right.fixedPort),
  preferredPort: Object.is,
  portMapping: Object.is,
  peerLimit: Object.is,
  uploadSlots: Object.is,
  activeDownloads: Object.is,
  activeSeeds: (left, right) =>
    left.unlimited === right.unlimited &&
    (left.unlimited || left.torrents === right.torrents),
  shareRatioLimit: Object.is,
  finishedDownloadRatioLimit: Object.is,
  finishedTimeLimit: Object.is,
  uploadRate: sameRateLimitDraftField,
  downloadRate: sameRateLimitDraftField,
  encryption: Object.is,
  ipv6Enabled: Object.is,
};

function clientSettingsDraft(settings: ClientSettings): ClientSettingsDraft {
  return {
    listener: {
      mode: productListenerMode(settings.listener),
      fixedPort: isFixedListener(settings.listener)
        ? String(settings.listener.port)
        : "",
    },
    preferredPort: String(settings.preferred_listen_port),
    portMapping: settings.port_mapping,
    peerLimit: String(settings.peer_connection_limit),
    uploadSlots: String(settings.upload_slots),
    activeDownloads: String(settings.active_downloads),
    activeSeeds: {
      unlimited: settings.active_seeds.type === "unlimited",
      torrents:
        settings.active_seeds.type === "limited"
          ? String(settings.active_seeds.torrents)
          : "5",
    },
    shareRatioLimit: String(settings.share_ratio_limit_percent),
    finishedDownloadRatioLimit: String(
      settings.finished_download_ratio_limit_percent,
    ),
    finishedTimeLimit: String(settings.finished_time_limit_seconds),
    uploadRate: {
      unlimited: settings.upload_rate_limit.type === "unlimited",
      valueKiB: rateLimitDraftValue(
        settings.upload_rate_limit,
        DEFAULT_UPLOAD_RATE_KIB,
      ),
    },
    downloadRate: {
      unlimited: settings.download_rate_limit.type === "unlimited",
      valueKiB: rateLimitDraftValue(
        settings.download_rate_limit,
        DEFAULT_DOWNLOAD_RATE_KIB,
      ),
    },
    encryption: settings.encryption,
    ipv6Enabled: settings.ipv6_enabled,
  };
}

function sameRateLimitDraftField(
  left: RateLimitDraftField,
  right: RateLimitDraftField,
): boolean {
  return left.unlimited === right.unlimited &&
    (left.unlimited || left.valueKiB === right.valueKiB);
}

function clientSettingsPatch(
  fields: readonly (keyof ClientSettingsDraft)[],
  settings: ClientSettings | null,
): ClientSettingsPatch | null {
  if (settings === null || fields.length === 0) return null;
  return {
    ...(fields.includes("listener") ? { listener: settings.listener } : {}),
    ...(fields.includes("preferredPort")
      ? { preferred_listen_port: settings.preferred_listen_port }
      : {}),
    ...(fields.includes("portMapping")
      ? { port_mapping: settings.port_mapping }
      : {}),
    ...(fields.includes("peerLimit")
      ? { peer_connection_limit: settings.peer_connection_limit }
      : {}),
    ...(fields.includes("uploadSlots")
      ? { upload_slots: settings.upload_slots }
      : {}),
    ...(fields.includes("activeDownloads")
      ? { active_downloads: settings.active_downloads }
      : {}),
    ...(fields.includes("activeSeeds")
      ? { active_seeds: settings.active_seeds }
      : {}),
    ...(fields.includes("shareRatioLimit")
      ? { share_ratio_limit_percent: settings.share_ratio_limit_percent }
      : {}),
    ...(fields.includes("finishedDownloadRatioLimit")
      ? {
          finished_download_ratio_limit_percent:
            settings.finished_download_ratio_limit_percent,
        }
      : {}),
    ...(fields.includes("finishedTimeLimit")
      ? { finished_time_limit_seconds: settings.finished_time_limit_seconds }
      : {}),
    ...(fields.includes("uploadRate")
      ? { upload_rate_limit: settings.upload_rate_limit }
      : {}),
    ...(fields.includes("downloadRate")
      ? { download_rate_limit: settings.download_rate_limit }
      : {}),
    ...(fields.includes("encryption")
      ? { encryption: settings.encryption }
      : {}),
    ...(fields.includes("ipv6Enabled")
      ? { ipv6_enabled: settings.ipv6_enabled }
      : {}),
  };
}

function clientDraftStatus(
  state: SettingsDraftState<ClientSettingsDraft>,
  phase: SettingsDraftPhase,
): string | null {
  if (phase === "submitting") return localizedMessage("inspection.components.connection.seeding.settings.section.saving.settings");
  if (phase === "awaiting_view") {
    return localizedMessage("inspection.components.connection.seeding.settings.section.settings.accepted.waiting.for.the.live.view");
  }
  if (phase === "conflict") {
    return localizedMessage("inspection.components.connection.seeding.settings.section.one.or.more.settings.changed.elsewhere.your");
  }
  return state.failure;
}

function validateDraft(
  listenerMode: ListenerMode,
  preferredPort: string,
  fixedPort: string,
  portMapping: PortMappingPolicy,
  peerLimit: string,
  uploadSlots: string,
  activeDownloads: string,
  activeSeeds: ActiveSeedDraftField,
  shareRatioLimit: string,
  finishedDownloadRatioLimit: string,
  finishedTimeLimit: string,
  uploadRate: RateLimitValidation,
  downloadRate: RateLimitValidation,
  encryption: EncryptionPolicy,
  ipv6Enabled: boolean,
  trackerHttpsServerAuthentication: ClientSettings["tracker_https_server_authentication"],
): DraftValidation {
  const preferred = parseBoundedInteger(
    preferredPort,
    FIXED_PORT_MINIMUM,
    FIXED_PORT_MAXIMUM,
  );
  const port = parseBoundedInteger(
    fixedPort,
    FIXED_PORT_MINIMUM,
    FIXED_PORT_MAXIMUM,
  );
  const peers = parseBoundedInteger(
    peerLimit,
    PEER_LIMIT_MINIMUM,
    PEER_LIMIT_MAXIMUM,
  );
  const slots = parseBoundedInteger(
    uploadSlots,
    UPLOAD_SLOTS_MINIMUM,
    UPLOAD_SLOTS_MAXIMUM,
  );
  const downloads = parseBoundedInteger(
    activeDownloads,
    ACTIVE_DOWNLOADS_MINIMUM,
    ACTIVE_DOWNLOADS_MAXIMUM,
  );
  const seeds = parseBoundedInteger(
    activeSeeds.torrents,
    ACTIVE_SEEDS_MINIMUM,
    ACTIVE_SEEDS_MAXIMUM,
  );
  const shareRatio = parseBoundedInteger(
    shareRatioLimit,
    SEED_GOAL_MINIMUM,
    SEED_GOAL_MAXIMUM,
  );
  const finishedDownloadRatio = parseBoundedInteger(
    finishedDownloadRatioLimit,
    SEED_GOAL_MINIMUM,
    SEED_GOAL_MAXIMUM,
  );
  const finishedTime = parseBoundedInteger(
    finishedTimeLimit,
    SEED_GOAL_MINIMUM,
    SEED_GOAL_MAXIMUM,
  );
  const fixedPortError =
    !isFixedListenerMode(listenerMode)
      ? null
      : port === null
        ? `Enter a whole number from ${FIXED_PORT_MINIMUM} to ${FIXED_PORT_MAXIMUM}.`
        : null;
  const preferredPortError =
    preferred === null
      ? `Enter a whole number from ${FIXED_PORT_MINIMUM} to ${FIXED_PORT_MAXIMUM}.`
      : null;
  const peerLimitError =
    peers === null
      ? `Enter a whole number from ${PEER_LIMIT_MINIMUM} to ${PEER_LIMIT_MAXIMUM}.`
      : null;
  const uploadSlotsError =
    slots === null
      ? `Enter a whole number from ${UPLOAD_SLOTS_MINIMUM} to ${UPLOAD_SLOTS_MAXIMUM}.`
      : null;
  const activeDownloadsError =
    downloads === null
      ? `Enter a whole number from ${ACTIVE_DOWNLOADS_MINIMUM} to ${ACTIVE_DOWNLOADS_MAXIMUM}.`
      : null;
  const activeSeedsError =
    activeSeeds.unlimited || seeds !== null
      ? null
      : `Enter a whole number from ${ACTIVE_SEEDS_MINIMUM} to ${ACTIVE_SEEDS_MAXIMUM}.`;
  const shareRatioError =
    shareRatio === null
      ? `Enter a whole number from ${SEED_GOAL_MINIMUM} to ${SEED_GOAL_MAXIMUM}.`
      : null;
  const finishedDownloadRatioError =
    finishedDownloadRatio === null
      ? `Enter a whole number from ${SEED_GOAL_MINIMUM} to ${SEED_GOAL_MAXIMUM}.`
      : null;
  const finishedTimeError =
    finishedTime === null
      ? `Enter a whole number from ${SEED_GOAL_MINIMUM} to ${SEED_GOAL_MAXIMUM}.`
      : null;
  if (
    preferredPortError !== null ||
    fixedPortError !== null ||
    peerLimitError !== null ||
    uploadSlotsError !== null ||
    activeDownloadsError !== null ||
    activeSeedsError !== null ||
    shareRatioError !== null ||
    finishedDownloadRatioError !== null ||
    finishedTimeError !== null ||
    uploadRate.error !== null ||
    downloadRate.error !== null
  ) {
    return {
      settings: null,
      preferredPortError,
      fixedPortError,
      peerLimitError,
      uploadSlotsError,
      activeDownloadsError,
      activeSeedsError,
      shareRatioError,
      finishedDownloadRatioError,
      finishedTimeError,
      uploadRateError: uploadRate.error,
      downloadRateError: downloadRate.error,
    };
  }
  const listener: ListenerPolicy = listenerMode === "automatic"
    ? { type: "automatic_local_network" }
    : { type: "fixed_local_network", port: port as number };
  return {
    settings: {
      listener,
      preferred_listen_port: preferred as number,
      port_mapping: portMapping,
      peer_connection_limit: peers as number,
      upload_slots: slots as number,
      active_downloads: downloads as number,
      active_seeds: activeSeeds.unlimited
        ? { type: "unlimited" }
        : { type: "limited", torrents: seeds as number },
      share_ratio_limit_percent: shareRatio as number,
      finished_download_ratio_limit_percent: finishedDownloadRatio as number,
      finished_time_limit_seconds: finishedTime as number,
      upload_rate_limit: uploadRate.limit as TransferRateLimit,
      download_rate_limit: downloadRate.limit as TransferRateLimit,
      encryption,
      ipv6_enabled: ipv6Enabled,
      tracker_https_server_authentication: trackerHttpsServerAuthentication,
    },
    preferredPortError: null,
    fixedPortError: null,
    peerLimitError: null,
    uploadSlotsError: null,
    activeDownloadsError: null,
    activeSeedsError: null,
    shareRatioError: null,
    finishedDownloadRatioError: null,
    finishedTimeError: null,
    uploadRateError: null,
    downloadRateError: null,
  };
}

function parseBoundedInteger(
  value: string,
  minimum: number,
  maximum: number,
): number | null {
  if (!/^\d+$/.test(value)) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= minimum && parsed <= maximum
    ? parsed
    : null;
}

const ENCRYPTION_OPTIONS: ReadonlyArray<{
  readonly value: EncryptionPolicy;
  readonly label: string;
  readonly description: string;
}> = [
  {
    value: "disabled",
    label: localizedMessage("inspection.components.connection.seeding.settings.section.disabled.75081b5"),
    description: localizedMessage("inspection.components.connection.seeding.settings.section.use.ordinary.plaintext.peer.handshakes.only"),
  },
  {
    value: "allow",
    label: localizedMessage("inspection.components.connection.seeding.settings.section.allow"),
    description: localizedMessage("inspection.components.connection.seeding.settings.section.accept.mse.pe.while.initiating.ordinary.connections"),
  },
  {
    value: "prefer",
    label: localizedMessage("inspection.components.connection.seeding.settings.section.prefer"),
    description: localizedMessage("inspection.components.connection.seeding.settings.section.try.mse.pe.first.and.use.a"),
  },
  {
    value: "required",
    label: localizedMessage("inspection.components.connection.seeding.settings.section.required"),
    description: localizedMessage("inspection.components.connection.seeding.settings.section.connect.only.when.the.peer.negotiates.mse"),
  },
];

function isFixedListenerMode(mode: ListenerMode): boolean {
  return mode === "fixed";
}

function productListenerMode(listener: ListenerPolicy): ListenerMode {
  return listener.type === "fixed_loopback" ||
    listener.type === "fixed_local_network"
    ? "fixed"
    : "automatic";
}

function isFixedListener(
  listener: ListenerPolicy,
): listener is Extract<
  ListenerPolicy,
  { type: "fixed_loopback" | "fixed_local_network" }
> {
  return listener.type === "fixed_loopback" ||
    listener.type === "fixed_local_network";
}

function isLoopbackListener(listener: ListenerPolicy): boolean {
  return listener.type === "automatic_loopback" ||
    listener.type === "fixed_loopback";
}

function bindFailureLabel(reason: ListenerBindFailureReason): string {
  switch (reason) {
    case "address_in_use":
      return localizedMessage("inspection.components.connection.seeding.settings.section.port.already.in.use");
    case "permission_denied":
      return localizedMessage("inspection.components.connection.seeding.settings.section.permission.denied");
    case "address_unavailable":
      return localizedMessage("inspection.components.connection.seeding.settings.section.address.unavailable");
    case "other":
      return localizedMessage("inspection.components.connection.seeding.settings.section.other.system.error");
  }
}
