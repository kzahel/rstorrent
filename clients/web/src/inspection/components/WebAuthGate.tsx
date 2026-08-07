import { useEffect, useState, type FormEvent } from "react";

import type {
  WebAuthClient,
  WebAuthStatus,
} from "../../web-auth-client";
import styles from "./WebAuthGate.module.css";

export interface WebAuthGateProps {
  readonly client: WebAuthClient;
  readonly initialStatus: WebAuthStatus;
  readonly onAuthorized: () => Promise<void>;
}

export function WebAuthGate({
  client,
  initialStatus,
  onAuthorized,
}: WebAuthGateProps) {
  const [status, setStatus] = useState(initialStatus);
  const [remaining, setRemaining] = useState(
    initialStatus.remaining_seconds ?? 0,
  );
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setRemaining(status.remaining_seconds ?? 0);
    if (status.remaining_seconds === undefined) return;
    const interval = window.setInterval(
      () => setRemaining((value) => Math.max(0, value - 1)),
      1_000,
    );
    return () => window.clearInterval(interval);
  }, [status]);

  const perform = async (operation: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await operation();
      await onAuthorized();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      try {
        setStatus(await client.status());
      } catch {
        // Retain the actionable operation error.
      }
    } finally {
      setBusy(false);
    }
  };

  const redeem = (event: FormEvent) => {
    event.preventDefault();
    if (!/^[0-9]{4}$/.test(code)) {
      setError("Enter the four-digit code shown by an approved browser.");
      return;
    }
    void perform(() => client.redeem(code, browserLabel()));
  };

  return (
    <main className={styles.page}>
      <section className={styles.card} aria-labelledby="web-auth-title">
        <div className={styles.brand} aria-hidden="true">RS</div>
        {status.state === "initial_window_open" ? (
          <>
            <p className={styles.eyebrow}>First launch</p>
            <h1 id="web-auth-title">Choose web access</h1>
            <p>
              Initial setup is open on this computer for {formatRemaining(remaining)}.
              Choose how future browser profiles should connect.
            </p>
            <div className={styles.choices}>
              <button
                type="button"
                disabled={busy}
                onClick={() => void perform(() => client.setPolicy("local_open"))}
              >
                <strong>Keep localhost open</strong>
                <span>Any browser on this computer can open RSTorrent.</span>
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  void perform(() => client.setPolicy("paired", browserLabel()))
                }
              >
                <strong>Remember this browser</strong>
                <span>New browser profiles will need your approval.</span>
              </button>
            </div>
          </>
        ) : status.state === "initial_window_expired" ? (
          <>
            <p className={styles.eyebrow}>Setup closed</p>
            <h1 id="web-auth-title">Restart to finish setup</h1>
            <p>
              Initial setup was available for 10 minutes and has now closed.
              Restart RSTorrent with the same profile to open a new 10-minute
              setup window.
            </p>
          </>
        ) : status.state === "recovery_window_open" ? (
          <>
            <p className={styles.eyebrow}>Recovery window</p>
            <h1 id="web-auth-title">Approve this browser</h1>
            <p>
              RSTorrent was restarted with browser pairing enabled. This window
              closes in {formatRemaining(remaining)} and the first approval
              consumes it.
            </p>
            <button
              className={styles.primary}
              type="button"
              disabled={busy}
              onClick={() => void perform(() => client.claimRecovery(browserLabel()))}
            >
              Approve this browser
            </button>
          </>
        ) : (
          <>
            <p className={styles.eyebrow}>Browser approval</p>
            <h1 id="web-auth-title">This browser is not approved</h1>
            <p>
              In an approved browser, open Settings → Web access → Approve
              another browser. Enter the displayed four-digit code here.
            </p>
            <form className={styles.codeForm} onSubmit={redeem}>
              <label htmlFor="pairing-code">Four-digit code</label>
              <input
                id="pairing-code"
                value={code}
                inputMode="numeric"
                autoComplete="one-time-code"
                pattern="[0-9]{4}"
                maxLength={4}
                disabled={busy}
                onChange={(event) =>
                  setCode(event.currentTarget.value.replace(/[^0-9]/g, ""))
                }
              />
              <button className={styles.primary} type="submit" disabled={busy}>
                Authorize browser
              </button>
            </form>
            <p className={styles.help}>
              If every approved browser profile or cookie is gone, stop the
              server and restart the same command with <code>--pairing-window</code>.
            </p>
          </>
        )}
        {error === null ? null : <p className={styles.error} role="alert">{error}</p>}
      </section>
    </main>
  );
}

function browserLabel(): string {
  const platform = navigator.platform.trim();
  return platform === "" ? "Browser" : `Browser on ${platform}`.slice(0, 80);
}

function formatRemaining(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return `${minutes}:${String(remainder).padStart(2, "0")}`;
}
