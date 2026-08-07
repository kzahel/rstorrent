import { useEffect, useMemo, useState, type FormEvent } from "react";

import type {
  AdvertisedPeerEndpointStatus,
  ClientSettings,
  ClientSettingsRuntimeView,
  ListenerBindFailureReason,
  ListenerPolicy,
  PortMappingPolicy,
  PortMappingStatus,
} from "../../api";
import styles from "./SettingsDialog.module.css";

const FIXED_PORT_MINIMUM = 1_024;
const FIXED_PORT_MAXIMUM = 65_535;
const PEER_LIMIT_MINIMUM = 1;
const PEER_LIMIT_MAXIMUM = 2_000;
const UPLOAD_SLOTS_MINIMUM = 0;
const UPLOAD_SLOTS_MAXIMUM = 50;

type ListenerMode = "automatic" | "fixed";

interface ConnectionSeedingSettingsSectionProps {
  readonly settings: ClientSettingsRuntimeView;
  readonly manageable: boolean;
  readonly onSave: (settings: ClientSettings) => Promise<void>;
}

interface DraftValidation {
  readonly settings: ClientSettings | null;
  readonly preferredPortError: string | null;
  readonly fixedPortError: string | null;
  readonly peerLimitError: string | null;
  readonly uploadSlotsError: string | null;
}

export function ConnectionSeedingSettingsSection({
  settings,
  manageable,
  onSave,
}: ConnectionSeedingSettingsSectionProps) {
  const configured = settings.configured;
  const [listenerMode, setListenerMode] = useState<ListenerMode>(
    productListenerMode(configured.listener),
  );
  const [fixedPort, setFixedPort] = useState(
    isFixedListener(configured.listener)
      ? String(configured.listener.port)
      : "",
  );
  const [preferredPort, setPreferredPort] = useState(
    String(configured.preferred_listen_port),
  );
  const [portMapping, setPortMapping] = useState<PortMappingPolicy>(
    configured.port_mapping,
  );
  const [peerLimit, setPeerLimit] = useState(
    String(configured.peer_connection_limit),
  );
  const [uploadSlots, setUploadSlots] = useState(
    String(configured.upload_slots),
  );
  const [pending, setPending] = useState(false);
  const [saveStatus, setSaveStatus] = useState<
    { readonly type: "success" | "error"; readonly message: string } | null
  >(null);

  useEffect(() => {
    setListenerMode(productListenerMode(configured.listener));
    setFixedPort(
      isFixedListener(configured.listener)
        ? String(configured.listener.port)
        : "",
    );
    setPortMapping(configured.port_mapping);
    setPreferredPort(String(configured.preferred_listen_port));
    setPeerLimit(String(configured.peer_connection_limit));
    setUploadSlots(String(configured.upload_slots));
  }, [
    configured.listener.type,
    isFixedListener(configured.listener)
      ? configured.listener.port
      : null,
    configured.port_mapping,
    configured.preferred_listen_port,
    configured.peer_connection_limit,
    configured.upload_slots,
  ]);

  const validation = useMemo(
    () =>
      validateDraft(
        listenerMode,
        preferredPort,
        fixedPort,
        portMapping,
        peerLimit,
        uploadSlots,
        configured.tracker_https_server_authentication,
      ),
    [
      configured.tracker_https_server_authentication,
      fixedPort,
      listenerMode,
      peerLimit,
      portMapping,
      preferredPort,
      uploadSlots,
    ],
  );
  const dirty =
    validation.settings !== null &&
    !sameClientSettings(validation.settings, configured);

  const updateDraft = (update: () => void) => {
    setSaveStatus(null);
    update();
  };

  const resetDraft = () => {
    setListenerMode(productListenerMode(configured.listener));
    setFixedPort(
      isFixedListener(configured.listener)
        ? String(configured.listener.port)
        : "",
    );
    setPortMapping(configured.port_mapping);
    setPreferredPort(String(configured.preferred_listen_port));
    setPeerLimit(String(configured.peer_connection_limit));
    setUploadSlots(String(configured.upload_slots));
    setSaveStatus(null);
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!manageable || !dirty || validation.settings === null) return;
    const nextSettings = validation.settings;
    setPending(true);
    setSaveStatus(null);
    try {
      await onSave(nextSettings);
      setSaveStatus({
        type: "success",
        message: "Settings accepted and applying.",
      });
    } catch (error) {
      setSaveStatus({
        type: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setPending(false);
    }
  };

  return (
    <fieldset className={styles.section}>
      <legend>Connection &amp; seeding</legend>
      {!manageable ? (
        <p className={styles.storageNote}>
          Connection and seeding settings are managed by the live application.
        </p>
      ) : null}
      <p className={styles.sectionIntroduction}>
        Incoming peer connections are enabled on all IPv4 network interfaces.
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
              selected={listenerMode === "automatic"}
              disabled={!manageable || pending}
              onSelect={() =>
                updateDraft(() => setListenerMode("automatic"))
              }
            />
            <ListenerOption
              mode="fixed"
              label="Fixed port"
              description="Always use the exact port entered below."
              selected={listenerMode === "fixed"}
              disabled={!manageable || pending}
              onSelect={() => updateDraft(() => setListenerMode("fixed"))}
            />
          </div>
          {listenerMode === "fixed" ? (
            <NumberField
              id="fixed-listener-port"
              label="Fixed listener port"
              value={fixedPort}
              minimum={FIXED_PORT_MINIMUM}
              maximum={FIXED_PORT_MAXIMUM}
              error={validation.fixedPortError}
              disabled={!manageable || pending}
              onChange={(value) => updateDraft(() => setFixedPort(value))}
            />
          ) : null}
        </div>

        <label className={styles.option}>
          <input
            type="checkbox"
            checked={portMapping === "upnp"}
            disabled={!manageable || pending}
            onChange={(event) =>
              updateDraft(() =>
                setPortMapping(event.currentTarget.checked ? "upnp" : "disabled"),
              )
            }
          />
          <span>
            <strong>Map incoming TCP with UPnP</strong>
            <small>
              Request a temporary IGD v2 gateway mapping when a compatible
              gateway is available.
            </small>
          </span>
        </label>

        <NumberField
          id="peer-connection-limit"
          label="Peer connection limit"
          description="Ordinary outgoing, established, and accepted peer connections across the session. The running process may use a lower safe limit."
          value={peerLimit}
          minimum={PEER_LIMIT_MINIMUM}
          maximum={PEER_LIMIT_MAXIMUM}
          error={validation.peerLimitError}
          disabled={!manageable || pending}
          onChange={(value) => updateDraft(() => setPeerLimit(value))}
        />

        <NumberField
          id="upload-slots"
          label="Payload upload slots"
          description="Peers allowed to receive piece payload at once. Zero keeps interested peers choked for piece payload; metadata and the listener remain available."
          value={uploadSlots}
          minimum={UPLOAD_SLOTS_MINIMUM}
          maximum={UPLOAD_SLOTS_MAXIMUM}
          error={validation.uploadSlotsError}
          disabled={!manageable || pending}
          onChange={(value) => updateDraft(() => setUploadSlots(value))}
        />

        <RuntimeState settings={settings} />

        <div className={styles.formActions}>
          <button
            className={styles.primaryAction}
            type="submit"
            disabled={
              !manageable || pending || validation.settings === null || !dirty
            }
          >
            {pending ? "Saving…" : "Save settings"}
          </button>
          <button
            className={styles.secondaryAction}
            type="button"
            disabled={!manageable || pending || !dirty}
            onClick={resetDraft}
          >
            Cancel changes
          </button>
        </div>
        {saveStatus === null ? null : (
          <output
            className={
              saveStatus.type === "error"
                ? styles.errorStatus
                : styles.successStatus
            }
            role={saveStatus.type === "error" ? "alert" : "status"}
            aria-live="polite"
          >
            {saveStatus.message}
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
      <PortMappingRuntime status={settings.port_mapping_status} />
      <AdvertisedEndpointRuntime status={settings.advertised_peer_endpoint} />
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

function PortMappingRuntime({ status }: { readonly status: PortMappingStatus }) {
  switch (status.type) {
    case "disabled":
      return <span>Automatic gateway mapping is off.</span>;
    case "ineligible":
      return (
        <span className={styles.runtimeWarning}>
          Gateway mapping requires an active incoming listener and a usable local network.
        </span>
      );
    case "discovering":
      return <span>Discovering a compatible UPnP IGD v2 gateway…</span>;
    case "mapping":
      return <span>Requesting and verifying the temporary TCP mapping…</span>;
    case "mapped":
      return (
        <span>
          UPnP mapped {status.external_address}:{status.external_port} to{" "}
          {status.local_address}:{status.local_port} for {status.lease_seconds} seconds.
        </span>
      );
    case "failed":
      return (
        <span className={styles.runtimeWarning}>
          Gateway mapping failed during {status.stage.replaceAll("_", " ")}: {status.detail}
        </span>
      );
    case "renewal_failed":
      return (
        <span className={styles.runtimeWarning}>
          The mapping at {status.external_address}:{status.external_port} could not be renewed: {status.detail}
        </span>
      );
    case "cleanup_failed":
      return (
        <span className={styles.runtimeWarning}>
          The old mapping at {status.external_address}:{status.external_port} could not be confirmed removed and may remain for {status.remaining_lease_seconds} seconds: {status.detail}
        </span>
      );
    case "stopping":
      return <span>Removing the gateway mapping…</span>;
  }
}

function validateDraft(
  listenerMode: ListenerMode,
  preferredPort: string,
  fixedPort: string,
  portMapping: PortMappingPolicy,
  peerLimit: string,
  uploadSlots: string,
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
  if (
    preferredPortError !== null ||
    fixedPortError !== null ||
    peerLimitError !== null ||
    uploadSlotsError !== null
  ) {
    return {
      settings: null,
      preferredPortError,
      fixedPortError,
      peerLimitError,
      uploadSlotsError,
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
      tracker_https_server_authentication: trackerHttpsServerAuthentication,
    },
    preferredPortError: null,
    fixedPortError: null,
    peerLimitError: null,
    uploadSlotsError: null,
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

function sameClientSettings(left: ClientSettings, right: ClientSettings): boolean {
  return (
    left.listener.type === right.listener.type &&
    (!isFixedListener(left.listener) ||
      (isFixedListener(right.listener) &&
        left.listener.port === right.listener.port)) &&
    left.port_mapping === right.port_mapping &&
    left.preferred_listen_port === right.preferred_listen_port &&
    left.peer_connection_limit === right.peer_connection_limit &&
    left.upload_slots === right.upload_slots &&
    left.tracker_https_server_authentication ===
      right.tracker_https_server_authentication
  );
}

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
