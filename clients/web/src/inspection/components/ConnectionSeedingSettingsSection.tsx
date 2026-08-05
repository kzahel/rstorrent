import { useEffect, useMemo, useState, type FormEvent } from "react";

import type {
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

type ListenerMode = ListenerPolicy["type"];

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
    configured.listener.type,
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
    setListenerMode(configured.listener.type);
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
      ),
    [fixedPort, listenerMode, peerLimit, portMapping, preferredPort, uploadSlots],
  );
  const dirty =
    validation.settings !== null &&
    !sameClientSettings(validation.settings, configured);

  const updateDraft = (update: () => void) => {
    setSaveStatus(null);
    update();
  };

  const resetDraft = () => {
    setListenerMode(configured.listener.type);
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
        message: sameClientSettings(nextSettings, settings.active)
          ? "Settings saved."
          : "Settings saved. Restart the application to apply these changes.",
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
        Device-only listeners accept peers from this application host. Local
        network listeners can also be mapped through a compatible gateway for
        public incoming TCP.
      </p>
      <form className={styles.settingsForm} onSubmit={(event) => void submit(event)}>
        <div
          className={styles.settingGroup}
          role="group"
          aria-labelledby="listener-policy-heading"
        >
          <div className={styles.settingHeading}>
            <strong id="listener-policy-heading">Incoming TCP listener</strong>
            <span>Choose how this application listens for local peers.</span>
          </div>
          <div className={styles.options}>
            <ListenerOption
              mode="disabled"
              label="Off"
              description="Do not accept incoming TCP connections."
              selected={listenerMode === "disabled"}
              disabled={!manageable || pending}
              onSelect={() => updateDraft(() => setListenerMode("disabled"))}
            />
            <ListenerOption
              mode="automatic_loopback"
              label="Automatic device-only port"
              description="Start with the preferred port, then use bounded fallback if it is occupied."
              selected={listenerMode === "automatic_loopback"}
              disabled={!manageable || pending}
              onSelect={() =>
                updateDraft(() => setListenerMode("automatic_loopback"))
              }
            />
            <ListenerOption
              mode="fixed_loopback"
              label="Fixed device-only port"
              description="Use the exact loopback port entered below at startup."
              selected={listenerMode === "fixed_loopback"}
              disabled={!manageable || pending}
              onSelect={() =>
                updateDraft(() => setListenerMode("fixed_loopback"))
              }
            />
            <ListenerOption
              mode="automatic_local_network"
              label="Automatic local-network port"
              description="Listen on the IPv4 address selected for this network at startup."
              selected={listenerMode === "automatic_local_network"}
              disabled={!manageable || pending}
              onSelect={() =>
                updateDraft(() => setListenerMode("automatic_local_network"))
              }
            />
            <ListenerOption
              mode="fixed_local_network"
              label="Fixed local-network port"
              description="Use the exact port on the selected local-network IPv4 address."
              selected={listenerMode === "fixed_local_network"}
              disabled={!manageable || pending}
              onSelect={() =>
                updateDraft(() => setListenerMode("fixed_local_network"))
              }
            />
          </div>
          <NumberField
            id="preferred-listener-port"
            label="Preferred automatic port"
            description="First TCP and UDP candidate for automatic listening. The running ports may differ after bounded conflict fallback."
            value={preferredPort}
            minimum={FIXED_PORT_MINIMUM}
            maximum={FIXED_PORT_MAXIMUM}
            error={validation.preferredPortError}
            disabled={!manageable || pending}
            onChange={(value) => updateDraft(() => setPreferredPort(value))}
          />
          {isFixedListenerMode(listenerMode) ? (
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
              Request a temporary IGD v2 gateway mapping at startup. This is
              eligible only with a local-network listener and remains off by default.
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
        <span>Incoming TCP is off for this application generation.</span>
      ) : listener.type === "listening" ? (
        <span>
          Listening on {listener.address}:{listener.port}.
        </span>
      ) : (
        <span className={styles.runtimeWarning}>
          Listener could not start ({bindFailureLabel(listener.reason)}):{" "}
          {listener.detail} Save a replacement and restart to try again.
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
      <PortMappingRuntime status={settings.port_mapping_status} />
      {settings.restart_required ? (
        <span className={styles.runtimeWarning}>
          Saved settings differ from the running application. Restart is
          required before they take effect.
        </span>
      ) : null}
      {settings.effective_peer_connection_limit <
      settings.active.peer_connection_limit ? (
        <span>
          The active {settings.active.peer_connection_limit}-peer setting is
          safely limited to {settings.effective_peer_connection_limit} by
          available file descriptors.
        </span>
      ) : (
        <span>
          Effective peer connection limit:{" "}
          {settings.effective_peer_connection_limit}.
        </span>
      )}
    </div>
  );
}

function PortMappingRuntime({ status }: { readonly status: PortMappingStatus }) {
  switch (status.type) {
    case "disabled":
      return <span>Automatic gateway mapping is off.</span>;
    case "ineligible":
      return (
        <span className={styles.runtimeWarning}>
          Gateway mapping requires an active local-network listener.
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
  const listener: ListenerPolicy =
    listenerMode === "disabled"
      ? { type: "disabled" }
      : listenerMode === "automatic_loopback"
        ? { type: "automatic_loopback" }
        : listenerMode === "fixed_loopback"
          ? { type: "fixed_loopback", port: port as number }
          : listenerMode === "automatic_local_network"
            ? { type: "automatic_local_network" }
            : { type: "fixed_local_network", port: port as number };
  return {
    settings: {
      listener,
      preferred_listen_port: preferred as number,
      port_mapping: portMapping,
      peer_connection_limit: peers as number,
      upload_slots: slots as number,
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
    left.upload_slots === right.upload_slots
  );
}

function isFixedListenerMode(mode: ListenerMode): boolean {
  return mode === "fixed_loopback" || mode === "fixed_local_network";
}

function isFixedListener(
  listener: ListenerPolicy,
): listener is Extract<
  ListenerPolicy,
  { type: "fixed_loopback" | "fixed_local_network" }
> {
  return isFixedListenerMode(listener.type);
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
