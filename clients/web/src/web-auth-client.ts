export type WebAuthState =
  | "unavailable"
  | "initial_window_open"
  | "initial_window_expired"
  | "local_open"
  | "session_required"
  | "session_valid"
  | "recovery_window_open";

export interface WebSession {
  readonly id: string;
  readonly label: string;
  readonly created_at: number;
  readonly last_used_at: number;
  readonly expires_at: number;
  readonly current: boolean;
}

export interface WebAuthStatus {
  readonly available: boolean;
  readonly state: WebAuthState;
  readonly remaining_seconds?: number;
  readonly current_session?: WebSession;
}

export interface PairingTicket {
  readonly code: string;
  readonly expires_at: number;
}

const MAX_AUTH_RESPONSE_BYTES = 64 * 1024;

export class WebAuthClient {
  public constructor(
    private readonly baseUrl: string,
    private readonly fetchImplementation: typeof fetch = globalThis.fetch,
  ) {}

  public status(): Promise<WebAuthStatus> {
    return this.request("GET", "/api/v1/web-auth/status", undefined, isStatus);
  }

  public async setPolicy(
    policy: "local_open" | "paired",
    label?: string,
  ): Promise<void> {
    await this.request(
      "POST",
      "/api/v1/web-auth/policy",
      { policy, ...(label === undefined ? {} : { label }) },
      isOptionalSession,
    );
  }

  public async claimRecovery(label: string): Promise<void> {
    await this.request(
      "POST",
      "/api/v1/web-auth/recovery",
      { label },
      isSession,
    );
  }

  public async redeem(code: string, label: string): Promise<void> {
    await this.request(
      "POST",
      "/api/v1/web-auth/pairing-ticket/redeem",
      { code, label },
      isSession,
    );
  }

  public createPairingTicket(): Promise<PairingTicket> {
    return this.request(
      "POST",
      "/api/v1/web-auth/pairing-ticket",
      undefined,
      isPairingTicket,
    );
  }

  public async sessions(): Promise<readonly WebSession[]> {
    const response = await this.request(
      "GET",
      "/api/v1/web-auth/sessions",
      undefined,
      isSessionsResponse,
    );
    return response.sessions;
  }

  public async revokeSession(sessionId: string): Promise<void> {
    await this.request(
      "DELETE",
      `/api/v1/web-auth/sessions/${encodeURIComponent(sessionId)}`,
      undefined,
      isNoContent,
    );
  }

  public async revokeOtherSessions(): Promise<number> {
    const response = await this.request(
      "DELETE",
      "/api/v1/web-auth/sessions/others",
      undefined,
      isChanged,
    );
    return response.changed;
  }

  public async logout(): Promise<void> {
    await this.request(
      "POST",
      "/api/v1/web-auth/logout",
      undefined,
      isNoContent,
    );
  }

  private async request<T>(
    method: string,
    path: string,
    body: unknown,
    validate: (value: unknown, noContent: boolean) => value is T,
  ): Promise<T> {
    const response = await this.fetchImplementation(new URL(path, this.baseUrl), {
      method,
      credentials: "include",
      headers: {
        Accept: "application/json",
        ...(body === undefined ? {} : { "Content-Type": "application/json" }),
      },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
    const source = await response.text();
    if (new TextEncoder().encode(source).byteLength > MAX_AUTH_RESPONSE_BYTES) {
      throw new Error("Web authentication response exceeded its bound");
    }
    if (!response.ok) {
      throw new Error(authErrorMessage(response.status, source));
    }
    const noContent = response.status === 204;
    const value: unknown = noContent ? undefined : parseJson(source);
    if (!validate(value, noContent)) {
      throw new Error("Web authentication response was malformed");
    }
    return value;
  }
}

function isStatus(value: unknown): value is WebAuthStatus {
  if (!isRecord(value) || typeof value.available !== "boolean") return false;
  if (
    typeof value.state !== "string" ||
    ![
      "unavailable",
      "initial_window_open",
      "initial_window_expired",
      "local_open",
      "session_required",
      "session_valid",
      "recovery_window_open",
    ].includes(value.state)
  ) {
    return false;
  }
  return (
    (value.remaining_seconds === undefined ||
      isNonnegativeInteger(value.remaining_seconds)) &&
    (value.current_session === undefined || isSession(value.current_session, false))
  );
}

function isSession(value: unknown, _noContent = false): value is WebSession {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    /^[0-9a-f]{32}$/.test(value.id) &&
    typeof value.label === "string" &&
    value.label.length > 0 &&
    value.label.length <= 80 &&
    isNonnegativeInteger(value.created_at) &&
    isNonnegativeInteger(value.last_used_at) &&
    isNonnegativeInteger(value.expires_at) &&
    typeof value.current === "boolean"
  );
}

function isOptionalSession(value: unknown, noContent: boolean): value is void {
  return noContent || isSession(value, false);
}

function isPairingTicket(value: unknown): value is PairingTicket {
  return (
    isRecord(value) &&
    typeof value.code === "string" &&
    /^[0-9]{4}$/.test(value.code) &&
    isNonnegativeInteger(value.expires_at)
  );
}

function isSessionsResponse(
  value: unknown,
): value is { readonly sessions: readonly WebSession[] } {
  return (
    isRecord(value) &&
    Array.isArray(value.sessions) &&
    value.sessions.length <= 32 &&
    value.sessions.every((session) => isSession(session, false))
  );
}

function isChanged(value: unknown): value is { readonly changed: number } {
  return isRecord(value) && isNonnegativeInteger(value.changed);
}

function isNoContent(_value: unknown, noContent: boolean): _value is void {
  return noContent;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonnegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function parseJson(source: string): unknown {
  try {
    return JSON.parse(source);
  } catch {
    throw new Error("Web authentication response was not JSON");
  }
}

function authErrorMessage(status: number, source: string): string {
  try {
    const value: unknown = JSON.parse(source);
    if (
      isRecord(value) &&
      isRecord(value.error) &&
      typeof value.error.message === "string"
    ) {
      return value.error.message;
    }
  } catch {
    // Fall back to the bounded status message.
  }
  return `Web authentication request failed (${status})`;
}
