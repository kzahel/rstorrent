import { useMemo, useRef, useState, type FormEvent } from "react";

import type {
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
      setAcceptedMessage("Settings accepted and applying.");
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
      <legend>Connection &amp; seeding</legend>
      {!manageable ? (
        <p className={styles.storageNote}>
          Connection and seeding settings are managed by the live application.
        </p>
      ) : null}
      <p className={styles.sectionIntroduction}>
        Incoming peer connections use IPv4 and, when available, IPv6.
        Automatic port selection is recommended; choose a fixed port only when
        your network requires one.
      </p>
      <form className={styles.settingsForm} onSubmit={(event) => void submit(event)}>
        <div
          className={styles.settingGroup}
          role="group"
          aria-labelledby="listener-policy-heading"
        >
          <div className={styles.settingHeading}>
            <strong id="listener-policy-heading">Incoming TCP listener</strong>
            <span>Choose automatic or fixed port selection.</span>
          </div>
          <div className={styles.options}>
            <ListenerOption
              mode="automatic"
              label="Automatic port"
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
              label="Fixed port"
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
              label="Fixed listener port"
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
            <strong>Enable IPv6</strong>
            <small>
              Use IPv6 for DHT, trackers, peer connections, and the incoming
              listener when this device has an eligible address.
            </small>
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
            <strong>Map incoming TCP and uTP with UPnP</strong>
            <small>
              Request independent temporary TCP and UDP IGD v2 mappings when
              a compatible gateway is available.
            </small>
          </span>
        </label>

        <NumberField
          id="active-downloads"
          label="Simultaneous downloads"
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
          aria-labelledby="peer-transfer-limits-heading"
        >
          <div className={styles.settingHeading}>
            <strong id="peer-transfer-limits-heading">All torrents peer transfer limits</strong>
            <span>
              Caps established BitTorrent peer traffic across the session. Trackers, DHT,
              connection handshakes, and network headers are not counted.
            </span>
          </div>
          <RateLimitField
            id="all-torrents-upload-rate"
            label="All torrents upload limit"
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
            label="All torrents download limit"
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
          label="Peer connection limit"
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
            <strong id="encryption-policy-heading">
              Protocol obfuscation (MSE/PE)
            </strong>
            <span>
              Improves compatibility with peers that require MSE/PE. This is
              protocol obfuscation, not privacy or security.
            </span>
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
          label="Payload upload slots"
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
            {pending ? "Saving…" : "Save settings"}
          </button>
          <button
            className={styles.secondaryAction}
            type="button"
            disabled={!manageable || dirtyFields.length === 0}
            onClick={resetDraft}
          >
            Cancel changes
          </button>
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
        <small id={descriptionId}>KiB/s of established peer traffic.</small>
      </span>
      <label className={styles.option}>
        <input
          type="checkbox"
          aria-label={`${label} unlimited`}
          checked={unlimited}
          disabled={disabled}
          onChange={(event) => onUnlimitedChange(event.currentTarget.checked)}
        />
        <span><strong>Unlimited</strong></span>
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
    <div className={styles.runtimeState} aria-label="Current runtime state">
      <strong>Current runtime</strong>
      {listener.type === "disabled" ? (
        <span>Incoming TCP is not running. Save settings to enable the ordinary listener.</span>
      ) : listener.type === "listening" ? (
        settings.effective_listener != null &&
        isLoopbackListener(settings.effective_listener.listener) ? (
          <span>
            Incoming TCP is using a development-only loopback listener at port {listener.port}.
            Save settings to listen on all IPv4 interfaces.
          </span>
        ) : (
          <span>Incoming TCP is listening on all IPv4 interfaces at port {listener.port}.</span>
        )
      ) : (
        <span className={styles.runtimeWarning}>
          Listener could not start ({bindFailureLabel(listener.reason)}):{" "}
          {listener.detail} Save settings to retry.
        </span>
      )}
      {settings.session_udp_status.type === "bound" ? (
        <span>
          Session UDP is bound on {settings.session_udp_status.address}:
          {settings.session_udp_status.port}
          {settings.session_udp_status.coordinated_with_tcp
            ? " with the TCP listener."
            : " independently from TCP."}
        </span>
      ) : (
        <span className={styles.runtimeWarning}>
          Session UDP is not available in this snapshot.
        </span>
      )}
      {settings.effective_listener == null ? (
        <span className={styles.runtimeWarning}>
          No incoming-listener policy has converged yet.
        </span>
      ) : (
        <span>
          Effective listener policy: {listenerPolicyLabel(settings.effective_listener.listener)}.
        </span>
      )}
      <span>
        Effective gateway mapping policy: {settings.effective_port_mapping === "upnp" ? "UPnP IGD v2" : "off"}.
      </span>
      <PortMappingRuntime label="TCP" status={settings.port_mapping_status} />
      <PortMappingRuntime
        label="UDP/uTP"
        status={settings.udp_port_mapping_status}
      />
      <Ipv6PinholeRuntime status={settings.ipv6_pinhole_status} />
      <AdvertisedEndpointRuntime status={settings.advertised_peer_endpoint} />
      {settings.transport_families.map((family) => (
        <span key={family.family}>
          {family.family === "ipv4" ? "IPv4" : "IPv6"}: {family.configured ? "configured" : "disabled"}
          {family.tcp_endpoint === null ? "" : ` · TCP ${family.tcp_endpoint}`}
          {family.udp_endpoint === null ? "" : ` · UDP ${family.udp_endpoint}`}
          {family.advertised_endpoint === null
            ? " · outbound-only advertisement"
            : ` · advertises ${family.advertised_endpoint}`}
        </span>
      ))}
      <ApplicationState label="Transport" state={settings.transport_application} />
      <ApplicationState
        label="Port mapping"
        state={settings.port_mapping_application}
      />
      <ApplicationState
        label="Peer connections"
        state={settings.peer_connections_application}
      />
      <ApplicationState
        label="Upload slots"
        state={settings.upload_slots_application}
      />
      <ApplicationState label="Peer transfer limits" state={settings.bandwidth_application} />
      <ApplicationState
        label="Protocol obfuscation"
        state={settings.encryption_application}
      />
      {settings.effective_peer_connection_limit <
      settings.configured.peer_connection_limit ? (
        <span>
          The configured {settings.configured.peer_connection_limit}-peer setting is
          safely limited to {settings.effective_peer_connection_limit} by
          available file descriptors.
        </span>
      ) : (
        <span>
          Effective peer connection limit:{" "}
          {settings.effective_peer_connection_limit}.
        </span>
      )}
      <span>Effective payload upload slots: {settings.effective_upload_slots}.</span>
      <span>
        Effective peer upload limit: {rateLimitLabel(settings.effective_upload_rate_limit)}.
      </span>
      <span>
        Effective peer download limit: {rateLimitLabel(settings.effective_download_rate_limit)}.
      </span>
      <span>
        Active downloads: {settings.active_download_count} of {settings.effective_active_downloads}
        {settings.checking_count === 0
          ? "."
          : ` · ${settings.checking_count} checking.`}
      </span>
      {settings.active_downloads_clamp_reason === "platform_limit" ? (
        <span>
          The configured {settings.configured.active_downloads}-download setting is limited to {settings.effective_active_downloads} on this platform.
        </span>
      ) : null}
      <span>
        Effective protocol obfuscation policy: {settings.effective_encryption}.
      </span>
      <span>
        Effective IPv6 policy: {settings.effective_ipv6_enabled ? "enabled" : "disabled"}.
      </span>
    </div>
  );
}

function listenerPolicyLabel(listener: ListenerPolicy): string {
  switch (listener.type) {
    case "disabled":
      return "development-only disabled mode";
    case "automatic_loopback":
      return "development-only loopback mode";
    case "fixed_loopback":
      return `development-only loopback port ${listener.port}`;
    case "automatic_local_network":
      return "automatic port";
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
    return <span>{label}: applying…</span>;
  }
  return (
    <span className={styles.runtimeWarning}>
      {label}: degraded ({state.detail}). Save settings to retry.
    </span>
  );
}

function AdvertisedEndpointRuntime({
  status,
}: {
  readonly status: AdvertisedPeerEndpointStatus;
}) {
  switch (status.type) {
    case "unavailable":
      return <span>Peer advertisement endpoint is not available yet.</span>;
    case "outbound_only":
      return (
        <span>
          Tracker discovery is outbound-only; no connectable peer port is
          advertised.
        </span>
      );
    case "local":
      return (
        <span>
          Peer protocols may advertise local TCP port {status.port}; external
          reachability is unverified
          {status.incoming_observed ? ", but an incoming peer was observed." : "."}
        </span>
      );
    case "mapped":
      return (
        <span>
          Peer protocols may advertise mapped TCP port {status.external_port}
          {status.incoming_observed
            ? "; an incoming peer was observed."
            : "; external reachability is not yet observed."}
        </span>
      );
    case "renewal_unhealthy":
      return (
        <span className={styles.runtimeWarning}>
          Mapped TCP port {status.external_port} remains valid for up to{" "}
          {status.lease_seconds_remaining} seconds, but renewal is unhealthy:{" "}
          {status.detail}
        </span>
      );
    case "stopping":
      return <span>Peer advertisement is stopping.</span>;
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
      return <span>Automatic {label} gateway mapping is off.</span>;
    case "ineligible":
      return (
        <span className={styles.runtimeWarning}>
          {label} gateway mapping requires an active listener and a usable local network.
        </span>
      );
    case "discovering":
      return <span>Discovering a compatible UPnP IGD v2 gateway for {label}…</span>;
    case "mapping":
      return <span>Requesting and verifying the temporary {label} mapping…</span>;
    case "mapped":
      return (
        <span>
          UPnP {label} mapped {status.external_address}:{status.external_port} to{" "}
          {status.local_address}:{status.local_port} for {status.lease_seconds} seconds.
        </span>
      );
    case "failed":
      return (
        <span className={styles.runtimeWarning}>
          {label} gateway mapping failed during {status.stage.replaceAll("_", " ")}: {status.detail}
        </span>
      );
    case "renewal_failed":
      return (
        <span className={styles.runtimeWarning}>
          The {label} mapping at {status.external_address}:{status.external_port} could not be renewed: {status.detail}
        </span>
      );
    case "cleanup_failed":
      return (
        <span className={styles.runtimeWarning}>
          The old {label} mapping at {status.external_address}:{status.external_port} could not be confirmed removed and may remain for {status.remaining_lease_seconds} seconds: {status.detail}
        </span>
      );
    case "stopping":
      return <span>Removing the {label} gateway mapping…</span>;
  }
}

function Ipv6PinholeRuntime({ status }: { readonly status: Ipv6PinholeStatus }) {
  switch (status.type) {
    case "disabled":
      return <span>Automatic IPv6 firewall pinhole control is off.</span>;
    case "ineligible":
      return (
        <span className={styles.runtimeWarning}>
          IPv6 pinhole control requires a global-unicast IPv6 listener and UPnP gateway control.
        </span>
      );
    case "discovering":
      return <span>Discovering IPv6 firewall control on the UPnP gateway…</span>;
    case "service_unavailable":
      return (
        <span className={styles.runtimeWarning}>
          The gateway does not advertise IPv6 firewall control. The IPv6 listener remains
          advertised as global unicast; gateway filtering is unknown.
        </span>
      );
    case "action_unavailable":
      return (
        <span className={styles.runtimeWarning}>
          The gateway IPv6 firewall service is incomplete: {status.detail}
        </span>
      );
    case "inbound_pinhole_disallowed":
      return (
        <span className={styles.runtimeWarning}>
          The gateway firewall is enabled but does not allow requested inbound pinholes.
        </span>
      );
    case "unfiltered":
      return (
        <span>
          The gateway reports IPv6 filtering disabled for {status.internal_address}:
          {status.internal_port}. This is gateway state, not an observed incoming peer.
        </span>
      );
    case "creating":
      return (
        <span>
          Requesting an IPv6 firewall pinhole for {status.internal_address}:
          {status.internal_port}…
        </span>
      );
    case "pinholed":
      return (
        <span>
          The gateway accepted an IPv6 pinhole for {status.internal_address}:
          {status.internal_port} for {status.lease_seconds} seconds. This does not mean an
          incoming peer has connected.
        </span>
      );
    case "failed":
      return (
        <span className={styles.runtimeWarning}>
          IPv6 pinhole control failed during {status.stage.replaceAll("_", " ")}: {status.detail}
        </span>
      );
    case "renewal_failed":
      return (
        <span className={styles.runtimeWarning}>
          The IPv6 pinhole for {status.internal_address}:{status.internal_port} could not be
          renewed: {status.detail}
        </span>
      );
    case "cleanup_failed":
      return (
        <span className={styles.runtimeWarning}>
          The old IPv6 pinhole for {status.internal_address}:{status.internal_port} could not be
          confirmed removed and may remain for {status.remaining_lease_seconds} seconds: {status.detail}
        </span>
      );
    case "stopping":
      return <span>Removing the IPv6 firewall pinhole…</span>;
  }
}

interface RateLimitDraftField {
  readonly unlimited: boolean;
  readonly valueKiB: string;
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
  if (phase === "submitting") return "Saving settings…";
  if (phase === "awaiting_view") {
    return "Settings accepted; waiting for the live view…";
  }
  if (phase === "conflict") {
    return "One or more settings changed elsewhere. Your draft is preserved for review.";
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
  if (
    preferredPortError !== null ||
    fixedPortError !== null ||
    peerLimitError !== null ||
    uploadSlotsError !== null
    || activeDownloadsError !== null || uploadRate.error !== null || downloadRate.error !== null
  ) {
    return {
      settings: null,
      preferredPortError,
      fixedPortError,
      peerLimitError,
      uploadSlotsError,
      activeDownloadsError,
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
    label: "Disabled",
    description: "Use ordinary plaintext peer handshakes only.",
  },
  {
    value: "allow",
    label: "Allow",
    description: "Accept MSE/PE while initiating ordinary connections.",
  },
  {
    value: "prefer",
    label: "Prefer",
    description: "Try MSE/PE first and use a bounded plaintext fallback.",
  },
  {
    value: "required",
    label: "Required",
    description: "Connect only when the peer negotiates MSE/PE.",
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
      return "port already in use";
    case "permission_denied":
      return "permission denied";
    case "address_unavailable":
      return "address unavailable";
    case "other":
      return "other system error";
  }
}
