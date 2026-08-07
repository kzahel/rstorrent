import { useEffect, useState } from "react";

import type {
  PairingTicket,
  WebAuthClient,
  WebAuthStatus,
  WebSession,
} from "../../web-auth-client";
import styles from "./WebAccessSettingsSection.module.css";

export interface WebAccessSettingsSectionProps {
  readonly client: WebAuthClient;
  readonly onSignedOut: () => void;
}

export function WebAccessSettingsSection({
  client,
  onSignedOut,
}: WebAccessSettingsSectionProps) {
  const [status, setStatus] = useState<WebAuthStatus | null>(null);
  const [sessions, setSessions] = useState<readonly WebSession[]>([]);
  const [ticket, setTicket] = useState<PairingTicket | null>(null);
  const [ticketRemaining, setTicketRemaining] = useState(0);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    const nextStatus = await client.status();
    setStatus(nextStatus);
    setSessions(
      nextStatus.state === "session_valid" ? await client.sessions() : [],
    );
  };

  useEffect(() => {
    void refresh().catch((cause) => setError(asMessage(cause)));
  }, []);

  useEffect(() => {
    if (ticket === null) return;
    const update = () =>
      setTicketRemaining(
        Math.max(0, ticket.expires_at - Math.floor(Date.now() / 1_000)),
      );
    update();
    const interval = window.setInterval(update, 1_000);
    return () => window.clearInterval(interval);
  }, [ticket]);

  const perform = async (operation: () => Promise<void>) => {
    setBusy(true);
    setMessage(null);
    setError(null);
    try {
      await operation();
    } catch (cause) {
      setError(asMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  if (status === null) {
    return <p className={styles.note}>Loading web access…</p>;
  }

  const paired = status.state === "session_valid";
  return (
    <div className={styles.panel}>
      <section className={styles.statusCard}>
        <div>
          <span>Current policy</span>
          <strong>{paired ? "Approved browsers" : "Localhost open"}</strong>
        </div>
        <p>
          {paired
            ? "This browser is approved. New browser profiles need a four-digit code or an explicit restart pairing window."
            : "Any browser on this computer can use this loopback RSTorrent service."}
        </p>
        <button
          type="button"
          disabled={busy}
          onClick={() =>
            void perform(async () => {
              if (paired) {
                await client.setPolicy("local_open");
                setMessage("Localhost access is now open.");
              } else {
                await client.setPolicy("paired", browserLabel());
                setMessage("This browser is now remembered.");
              }
              setTicket(null);
              await refresh();
            })
          }
        >
          {paired ? "Keep localhost open" : "Require browser approval"}
        </button>
      </section>

      {paired ? (
        <>
          <section className={styles.section}>
            <div className={styles.sectionHeading}>
              <div>
                <h3>Approve another browser</h3>
                <p>The code works once, for 10 minutes, with five attempts.</p>
              </div>
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  void perform(async () => {
                    const next = await client.createPairingTicket();
                    setTicket(next);
                    setMessage("A new pairing code is ready.");
                  })
                }
              >
                Generate code
              </button>
            </div>
            {ticket === null ? null : (
              <div className={styles.ticket} role="status">
                <strong aria-label={`Pairing code ${ticket.code.split("").join(" ")}`}>
                  {ticket.code}
                </strong>
                <span>
                  {ticketRemaining > 0
                    ? `Expires in ${formatRemaining(ticketRemaining)}`
                    : "Expired — generate a new code"}
                </span>
              </div>
            )}
          </section>

          <section className={styles.section}>
            <div className={styles.sectionHeading}>
              <div>
                <h3>Authorized browsers</h3>
                <p>{sessions.length} of 32 remembered sessions</p>
              </div>
              <button
                type="button"
                disabled={busy || sessions.length <= 1}
                onClick={() => {
                  if (!window.confirm("Revoke every other authorized browser?")) return;
                  void perform(async () => {
                    const changed = await client.revokeOtherSessions();
                    setMessage(`Revoked ${changed} other browser${changed === 1 ? "" : "s"}.`);
                    await refresh();
                  });
                }}
              >
                Revoke all others
              </button>
            </div>
            <ul className={styles.sessions}>
              {sessions.map((session) => (
                <li key={session.id}>
                  <div>
                    <strong>{session.label}</strong>
                    {session.current ? <span className={styles.current}>This browser</span> : null}
                    <small>
                      Added {formatDate(session.created_at)} · Last used {formatDate(session.last_used_at)}
                    </small>
                  </div>
                  {session.current ? null : (
                    <button
                      type="button"
                      disabled={busy}
                      aria-label={`Revoke ${session.label}`}
                      onClick={() => {
                        if (!window.confirm(`Revoke ${session.label}?`)) return;
                        void perform(async () => {
                          await client.revokeSession(session.id);
                          setMessage(`${session.label} was revoked.`);
                          await refresh();
                        });
                      }}
                    >
                      Revoke
                    </button>
                  )}
                </li>
              ))}
            </ul>
          </section>

          <section className={styles.section}>
            <h3>Recovery and sign out</h3>
            <p className={styles.note}>
              If every authorized cookie or browser profile is gone, stop the
              server and restart the same command with <code>--pairing-window</code>.
              The first explicit browser approval consumes that 10-minute window.
            </p>
            <button
              className={styles.danger}
              type="button"
              disabled={busy}
              onClick={() => {
                if (!window.confirm("Sign out this browser?")) return;
                void perform(async () => {
                  await client.logout();
                  onSignedOut();
                });
              }}
            >
              Sign out this browser
            </button>
          </section>
        </>
      ) : null}

      {message === null ? null : <p className={styles.success} role="status">{message}</p>}
      {error === null ? null : <p className={styles.error} role="alert">{error}</p>}
    </div>
  );
}

function browserLabel(): string {
  const platform = navigator.platform.trim();
  return platform === "" ? "Browser" : `Browser on ${platform}`.slice(0, 80);
}

function formatRemaining(seconds: number): string {
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

function formatDate(unixSeconds: number): string {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(unixSeconds * 1_000));
}

function asMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
