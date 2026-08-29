import { useEffect, useState, type FormEvent } from "react";

import type {
  DesktopRemoteAccess,
  DesktopRemoteAccessState,
  RemoteAuthorizedClient,
  RemoteSecuritySnapshot,
} from "../remote-access/types";
import styles from "./RemoteAccessSettingsSection.module.css";

export interface RemoteAccessSettingsSectionProps {
  readonly remoteAccess: DesktopRemoteAccess;
}

export function RemoteAccessSettingsSection({
  remoteAccess,
}: RemoteAccessSettingsSectionProps) {
  const [state, setState] = useState<DesktopRemoteAccessState | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [username, setUsername] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [newPassphrase, setNewPassphrase] = useState("");
  const [newConfirmation, setNewConfirmation] = useState("");

  const refresh = async () => setState(await remoteAccess.state());

  useEffect(() => {
    void refresh().catch((cause: unknown) => setError(asMessage(cause)));
  }, []);

  const perform = async (
    operation: () => Promise<string | void>,
  ): Promise<void> => {
    setBusy(true);
    setMessage(null);
    setError(null);
    try {
      const result = await operation();
      await refresh();
      if (result !== undefined) setMessage(result);
    } catch (cause) {
      setError(asMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  if (state === null) return <p className={styles.note}>Loading remote access…</p>;
  if (!state.configured) {
    return (
      <div className={styles.panel}>
        <section className={styles.statusCard}>
          <span>Validation mode</span>
          <strong>Not configured for this launch</strong>
          <p>
            Remote access remains off. This internal build only exposes it when
            the desktop process starts with an explicit loopback relay and local
            certificate configuration.
          </p>
        </section>
      </div>
    );
  }

  const security = state.security;
  const local = remoteAccess.scope === "local";
  if (security === null) {
    return <p className={styles.error}>Remote security state is unavailable.</p>;
  }

  const enable = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (passphrase !== confirmation) {
      setError("The password confirmation does not match.");
      return;
    }
    void perform(async () => {
      await remoteAccess.enable(username, passphrase);
      setPassphrase("");
      setConfirmation("");
      return "Remote access is enabled.";
    });
  };

  const changePassphrase = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (newPassphrase !== newConfirmation) {
      setError("The new password confirmation does not match.");
      return;
    }
    void perform(async () => {
      const revoked = await remoteAccess.changePassphrase(newPassphrase);
      setNewPassphrase("");
      setNewConfirmation("");
      return `Password changed and ${revoked} browser authorization${revoked === 1 ? " was" : "s were"} revoked.`;
    });
  };

  return (
    <div className={styles.panel}>
      <section className={styles.statusCard}>
        <div>
          <span>Remote access</span>
          <strong>{security.enabled ? "Enabled" : "Disabled"}</strong>
        </div>
        {security.enabled ? (
          <dl className={styles.identity}>
            <div><dt>Route</dt><dd>{security.route}</dd></div>
            <div><dt>Username</dt><dd>{security.username}</dd></div>
            <div><dt>Relay deployment</dt><dd><code>{security.relay_id}</code></dd></div>
            <div><dt>Host identity</dt><dd><code>{security.host_pin}</code></dd></div>
          </dl>
        ) : (
          <p>
            The host authority is absent. Retained entries below are audit
            evidence only and cannot authorize a connection.
          </p>
        )}
        <button type="button" disabled={busy} onClick={() => void perform(refresh)}>
          Refresh audit
        </button>
      </section>

      {!security.enabled && local ? (
        <section className={styles.section}>
          <h3>Enable local validation access</h3>
          <p className={styles.note}>
            Choose the relay route username and a password of at least 12 characters.
            The password is not stored.
          </p>
          <form className={styles.form} onSubmit={enable}>
            <label>Route username<input required minLength={3} maxLength={32} pattern="[a-z0-9](?:[a-z0-9-]*[a-z0-9])?" value={username} onChange={(event) => setUsername(event.currentTarget.value)} /></label>
            <label>Password<input required type="password" minLength={12} maxLength={256} autoComplete="new-password" value={passphrase} onChange={(event) => setPassphrase(event.currentTarget.value)} /></label>
            <label>Confirm password<input required type="password" minLength={12} maxLength={256} autoComplete="new-password" value={confirmation} onChange={(event) => setConfirmation(event.currentTarget.value)} /></label>
            <button type="submit" disabled={busy}>Enable remote access</button>
          </form>
        </section>
      ) : (
        <>
          <AuthorizationSection
            snapshot={security.authority}
            busy={busy}
            currentClientId={remoteAccess.currentClientId}
            activeClientIds={security.live_circuits.flatMap((circuit) =>
              circuit.client_id === null ? [] : [circuit.client_id]
            )}
            perform={perform}
            remoteAccess={remoteAccess}
          />

          <section className={styles.section}>
            <div className={styles.sectionHeading}>
              <div><h3>Live circuits</h3><p>{security.live_circuits.length} active</p></div>
            </div>
            {security.live_circuits.length === 0 ? (
              <p className={styles.note}>No browser is connected.</p>
            ) : (
              <ul className={styles.records}>
                {security.live_circuits.map((circuit) => (
                  <li key={circuit.circuit_id}>
                    <div>
                      <strong>{circuit.client_id === null ? "Shared browser" : labelForClient(security.authority, circuit.client_id)}</strong>
                      <small>
                        {circuit.authentication_method} · generation {circuit.connection_generation} · started {formatDate(circuit.started)} · active {formatDate(circuit.last_activity)} · {circuit.route}
                      </small>
                      <code>{circuit.circuit_id}</code>
                    </div>
                    <button type="button" disabled={busy} onClick={() => {
                      if (!window.confirm("Close this authenticated remote circuit?")) return;
                      void perform(async () => {
                        await remoteAccess.closeCircuit(circuit.circuit_id);
                        return "The circuit was closed.";
                      });
                    }}>Close</button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className={styles.section}>
            <h3>Authentication and recovery</h3>
            <div className={styles.actions}>
              <button type="button" disabled={busy || (security.authority?.clients.length ?? 0) === 0} onClick={() => {
                if (!window.confirm("Require the password again on every browser and close their authorized circuits?")) return;
                void perform(async () => {
                  const revoked = await remoteAccess.requirePasswordEverywhere();
                  return `Revoked ${revoked} browser authorization${revoked === 1 ? "" : "s"}.`;
                });
              }}>Require password everywhere</button>
              {local ? <button className={styles.danger} type="button" disabled={busy} onClick={() => {
                if (!window.confirm("Disable remote access, revoke every browser, and remove the host authority?")) return;
                void perform(async () => {
                  const outcome = await remoteAccess.disable();
                  return `Remote access disabled. Authority removed: ${yesNo(outcome.authority_file_removed)}; route released: ${yesNo(outcome.route_released)}.`;
                });
              }}>Disable remote access</button> : null}
              {remoteAccess.signOutThisBrowser === undefined ? null : (
                <button className={styles.danger} type="button" disabled={busy} onClick={() => {
                  if (!window.confirm("Sign out this browser and revoke its private authorization, if present?")) return;
                  setBusy(true);
                  setMessage(null);
                  setError(null);
                  void remoteAccess.signOutThisBrowser?.().catch((cause: unknown) => {
                    setError(asMessage(cause));
                    setBusy(false);
                  });
                }}>Sign out this browser</button>
              )}
            </div>
            {local ? <form className={styles.form} onSubmit={changePassphrase}>
              <h4>Change password</h4>
              <p className={styles.note}>Changing it revokes every private browser and closes every remote circuit.</p>
              <label>New password<input required type="password" minLength={12} maxLength={256} autoComplete="new-password" value={newPassphrase} onChange={(event) => setNewPassphrase(event.currentTarget.value)} /></label>
              <label>Confirm new password<input required type="password" minLength={12} maxLength={256} autoComplete="new-password" value={newConfirmation} onChange={(event) => setNewConfirmation(event.currentTarget.value)} /></label>
              <button type="submit" disabled={busy}>Change password</button>
            </form> : null}
          </section>
        </>
      )}

      <AuditHistory
        title="Current security ledger"
        snapshot={security.authority}
        busy={busy}
      />
      <AuditHistory
        title="Retained security history"
        snapshot={security.retained_history}
        busy={busy}
        clear={() => {
          if (!window.confirm("Clear retained remote security history? Current authorizations are not removed.")) return;
          void perform(async () => {
            const cleared = await remoteAccess.clearHistory();
            return cleared ? "Retained history was cleared." : "There was no retained history to clear.";
          });
        }}
      />

      {message === null ? null : <p className={styles.success} role="status">{message}</p>}
      {error === null ? null : <p className={styles.error} role="alert">{error}</p>}
    </div>
  );
}

interface AuthorizationSectionProps {
  readonly snapshot: RemoteSecuritySnapshot | null;
  readonly busy: boolean;
  readonly activeClientIds: readonly string[];
  readonly currentClientId?: string | undefined;
  readonly remoteAccess: DesktopRemoteAccess;
  readonly perform: (operation: () => Promise<string | void>) => Promise<void>;
}

function AuthorizationSection({ snapshot, busy, activeClientIds, currentClientId, remoteAccess, perform }: AuthorizationSectionProps) {
  const clients = snapshot?.clients ?? [];
  return (
    <section className={styles.section}>
      <div className={styles.sectionHeading}>
        <div><h3>Authorized browsers</h3><p>{clients.length} of 32 current authorizations</p></div>
      </div>
      {clients.length === 0 ? <p className={styles.note}>No browser can resume without the password.</p> : (
        <ul className={styles.records}>
          {clients.map((client) => (
            <li key={client.client_id}>
              <div>
                <strong>{client.label}</strong>
                {client.client_id === currentClientId ? <span className={styles.badge}>This browser</span> : null}
                <span className={styles.badge}>{activeClientIds.filter((id) => id === client.client_id).length} live</span>
                <small>{client.state} · added {formatDate(client.created)} · password {formatDate(client.last_full_login)} · resume {formatOptionalDate(client.last_resume)} · seen {formatDate(client.last_seen)}</small>
                <small>Idle expiry {formatDate(client.idle_expires)} · absolute expiry {formatDate(client.absolute_expires)} · build {client.client_build ?? "not reported"}</small>
                <small>Route observation {client.route_observation ?? "not reported"} · browser observation {client.browser_observation ?? "not reported"}</small>
                <code title={client.fingerprint}>{client.fingerprint}</code>
              </div>
              <div className={styles.rowActions}>
                <button type="button" disabled={busy} onClick={() => {
                  const label = window.prompt("Browser name", client.label);
                  if (label === null || label === client.label) return;
                  void perform(async () => {
                    await remoteAccess.rename(client.client_id, label);
                    return "Browser authorization renamed.";
                  });
                }}>Rename</button>
                <button type="button" disabled={busy || clients.length <= 1} onClick={() => {
                  if (!window.confirm(`Revoke every browser except ${client.label}?`)) return;
                  void perform(async () => {
                    const revoked = await remoteAccess.revokeAllOther(client.client_id);
                    return `Revoked ${revoked} other browser${revoked === 1 ? "" : "s"}.`;
                  });
                }}>Keep only this</button>
                <button className={styles.danger} type="button" disabled={busy} onClick={() => {
                  if (!window.confirm(`Revoke ${client.label} and close all of its circuits?`)) return;
                  void perform(async () => {
                    await remoteAccess.revoke(client.client_id);
                    return `${client.label} was revoked.`;
                  });
                }}>Revoke</button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function AuditHistory({ title, snapshot, busy, clear }: { readonly title: string; readonly snapshot: RemoteSecuritySnapshot | null; readonly busy: boolean; readonly clear?: () => void }) {
  if (snapshot === null) return null;
  return (
    <section className={styles.section}>
      <div className={styles.sectionHeading}>
        <div><h3>{title}</h3><p>{snapshot.events.length} owner events · {snapshot.tombstones.length} ended authorizations · {snapshot.failed_attempts.length} failed-attempt buckets</p></div>
        {clear === undefined ? null : <button type="button" disabled={busy || (snapshot.events.length === 0 && snapshot.tombstones.length === 0 && snapshot.failed_attempts.length === 0)} onClick={clear}>Clear history</button>}
      </div>
      <p className={styles.note}>Generation {snapshot.generation}; authorization generation {snapshot.authorization_generation}. Entries below are complete and unfiltered.</p>
      <ol className={styles.audit}>
        {snapshot.events.map((event) => (
          <li key={event.event_id}>
            <strong>{humanize(event.kind)}</strong>
            <small>{formatDate(event.timestamp)} · {event.result} · {event.authentication_method ?? "owner action"} · route {event.route ?? "none"}</small>
            <small>client {event.client_id ?? "none"} · circuit {event.circuit_id ?? "none"} · build {event.client_build ?? "none"} · reason {event.reason_class ?? "none"}</small>
          </li>
        ))}
      </ol>
      {snapshot.tombstones.length === 0 ? null : <details><summary>Ended browser authorizations ({snapshot.tombstones.length})</summary><ul className={styles.audit}>{snapshot.tombstones.map((item) => <li key={`${item.client_id}-${item.ended}`}><strong>{item.label} — {item.state}</strong><small>Added {formatDate(item.created)} · seen {formatDate(item.last_seen)} · ended {formatDate(item.ended)}</small><code>{item.fingerprint}</code></li>)}</ul></details>}
      {snapshot.failed_attempts.length === 0 ? null : <details><summary>Failed authentication pressure ({snapshot.failed_attempts.length} buckets)</summary><ul className={styles.audit}>{snapshot.failed_attempts.map((item) => <li key={`${item.bucket_start}-${item.kind}-${item.route_class}`}><strong>{item.kind}: {item.attempts} attempts</strong><small>{formatDate(item.bucket_start)} · route class {item.route_class}</small></li>)}</ul></details>}
    </section>
  );
}

function labelForClient(snapshot: RemoteSecuritySnapshot | null, clientId: string): string {
  return snapshot?.clients.find((client) => client.client_id === clientId)?.label ?? `Browser ${clientId}`;
}

function formatDate(value: number): string {
  return new Date(value).toLocaleString();
}

function formatOptionalDate(value: number | null): string {
  return value === null ? "never" : formatDate(value);
}

function humanize(value: string): string {
  return value.replaceAll("_", " ");
}

function yesNo(value: boolean): string {
  return value ? "yes" : "no";
}

function asMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
