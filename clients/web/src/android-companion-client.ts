import type {
  AddTorrentBytesRequest,
  ApiBackendIdentity,
  ApiHello,
  ChooseDownloadRootRequest,
  OpenViewSetRequest,
  OpenViewSetResponse,
  RequestEnvelope,
  ResponseEnvelope,
  StorageRootSnapshot,
  UpdateBatch,
  UpdateViewSetRequest,
} from "./api/generated/v1";
import type {
  ApplicationUpdateStream,
  ApplicationViewClient,
} from "./api/client";
import { decodeChooseDownloadRootResponse } from "./validation";
import {
  WebSocketApplicationViewClient,
  type ApplicationWebSocketPlatformClient,
} from "./websocket-view-client";

const PORTS = [3030, 3031, 3032, 3033, 3034] as const;
const ENDPOINTS = [
  "http://100.115.92.2:3030",
  "http://100.115.92.2:3031",
  "http://100.115.92.2:3032",
  "http://100.115.92.2:3033",
  "http://100.115.92.2:3034",
] as const;
const API_ROOT = "/rstorrent/companion/v1";
const STORAGE_KEY = "rstorrentAndroidCompanionV1";
const EXTENSION_ORIGINS = [
  "chrome-extension://gcgoepclopkgijmclmlheafaglmbjlcc",
  "chrome-extension://dbokmlpefliilbjldladbimlcfgbolhk",
] as const;
const PROBE_TIMEOUT_MILLIS = 2_000;
const PROBE_INTERVAL_MILLIS = 2_000;
const MAX_BOOTSTRAP_RESPONSE_BYTES = 64 * 1024;
const REQUIRED_PROFILE = [
  "android_saf_acquisition",
  "retained_storage_roots",
  "one_current_root",
  "joined_platform_root_removal",
] as const;

interface CompanionHello {
  readonly product: string;
  readonly backend: string;
  readonly protocol_min: number;
  readonly protocol_max: number;
  readonly port: number;
  readonly nonce: string;
  readonly paired: boolean;
}

interface PairingPending {
  readonly request_id: string;
  readonly expires_in_seconds: number;
}

interface PairingPoll {
  readonly status: "pending" | "approved" | "rejected" | "expired";
  readonly credential?: string;
}

interface StoredCompanion {
  readonly installationId: string;
  readonly credential?: string;
  readonly backend?: ApiBackendIdentity;
}

interface ChromeStorageArea {
  get(key: string): Promise<Record<string, unknown>>;
  set(items: Record<string, unknown>): Promise<void>;
  remove(key: string): Promise<void>;
}

interface ChromeApi {
  readonly storage: { readonly local: ChromeStorageArea };
}

export interface AndroidCompanionConnection {
  readonly client: ApplicationViewClient;
  readonly hello: ApiHello;
  readonly endpoint: string;
}

export type AndroidCompanionStatus = (message: string) => void;

export async function connectAndroidCompanion(
  status: AndroidCompanionStatus,
  signal?: AbortSignal,
): Promise<AndroidCompanionConnection> {
  const stored = await readStoredCompanion();
  const installationId = stored?.installationId ?? randomIdentifier();
  const endpoint = await probeUntilAvailable(installationId, status, signal);
  let credential = stored?.credential;
  if (credential === undefined) {
    credential = await pair(endpoint, installationId, status, signal);
  }
  let connected: {
    readonly client: ApplicationViewClient;
    readonly hello: ApiHello;
  };
  try {
    connected = await openApplication(
      endpoint,
      installationId,
      credential,
      signal,
    );
  } catch (error) {
    if (signal?.aborted) throw error;
    status("The saved pairing is no longer accepted. Approve a replacement in Android.");
    credential = await pair(endpoint, installationId, status, signal);
    connected = await openApplication(endpoint, installationId, credential, signal);
  }
  const backend = validateAndroidBackend(connected.hello);
  if (stored?.backend !== undefined && !sameBackendIdentity(stored.backend, backend)) {
    await connected.client.close();
    await clearCredential(installationId);
    throw new Error(
      "The Android backend identity changed. Select Connect Android app again and approve a new pairing.",
    );
  }
  await writeStoredCompanion({ installationId, credential, backend });
  return { ...connected, endpoint };
}

export async function forgetAndroidCompanion(): Promise<void> {
  await chromeApi().storage.local.remove(STORAGE_KEY);
}

async function probeUntilAvailable(
  installationId: string,
  status: AndroidCompanionStatus,
  signal?: AbortSignal,
): Promise<string> {
  while (!signal?.aborted) {
    status("Looking for the RSTorrent Android service…");
    for (const endpoint of ENDPOINTS) {
      try {
        await hello(endpoint, installationId, signal);
        return endpoint;
      } catch (error) {
        if (signal?.aborted) throw error;
      }
    }
    await delay(PROBE_INTERVAL_MILLIS, signal);
  }
  throw signal.reason ?? new Error("Android companion connection canceled");
}

async function pair(
  endpoint: string,
  installationId: string,
  status: AndroidCompanionStatus,
  signal?: AbortSignal,
): Promise<string> {
  const greeting = await hello(endpoint, installationId, signal);
  const extensionNonce = randomIdentifier();
  const pending =
    (await companionJson(endpoint, "/pairing/request", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        hello_nonce: greeting.nonce,
        installation_id: installationId,
        extension_nonce: extensionNonce,
      }),
      ...(signal === undefined ? {} : { signal }),
    })) as PairingPending;
  boundedIdentifier(pending.request_id, "pairing request ID");
  status("Approve the JSTorrent Beta pairing request in the Android app.");
  while (!signal?.aborted) {
    await delay(500, signal);
    const poll =
      (await companionJson(endpoint, "/pairing/poll", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          request_id: pending.request_id,
          installation_id: installationId,
          extension_nonce: extensionNonce,
        }),
        ...(signal === undefined ? {} : { signal }),
      })) as PairingPoll;
    if (poll.status === "pending") continue;
    if (poll.status === "approved" && typeof poll.credential === "string") {
      boundedCredential(poll.credential);
      return poll.credential;
    }
    throw new Error(
      poll.status === "rejected"
        ? "The Android pairing request was rejected."
        : "The Android pairing request expired.",
    );
  }
  throw signal.reason ?? new Error("Android pairing canceled");
}

async function hello(
  endpoint: string,
  installationId: string,
  signal?: AbortSignal,
): Promise<CompanionHello> {
  const timeout = AbortSignal.timeout(PROBE_TIMEOUT_MILLIS);
  const combined =
    signal === undefined ? timeout : AbortSignal.any([signal, timeout]);
  const value =
    (await companionJson(endpoint, "/hello", {
      headers: { "X-RSTorrent-Installation": installationId },
      signal: combined,
    })) as CompanionHello;
  if (
    value.product !== "rstorrent" ||
    value.backend !== "android" ||
    value.protocol_min > 1 ||
    value.protocol_max < 1 ||
    !PORTS.includes(value.port as (typeof PORTS)[number])
  ) {
    throw new Error("The ARC endpoint is not a compatible RSTorrent Android service");
  }
  boundedIdentifier(value.nonce, "hello nonce");
  return value;
}

async function openApplication(
  endpoint: string,
  installationId: string,
  credential: string,
  signal?: AbortSignal,
): Promise<{ readonly client: ApplicationViewClient; readonly hello: ApiHello }> {
  const platform = new AndroidPlatformClient(
    endpoint,
    installationId,
    credential,
  );
  const socket = new WebSocketApplicationViewClient(
    endpoint,
    credential,
    undefined,
    installationId,
    {
      connectPath: `${API_ROOT}/connect`,
      platformClient: platform,
    },
  );
  const client = new AndroidCompanionApplicationClient(socket);
  try {
    const greeting = await client.hello(signal);
    return { client, hello: greeting };
  } catch (error) {
    await client.close();
    throw error;
  }
}

class AndroidPlatformClient implements ApplicationWebSocketPlatformClient {
  public constructor(
    private readonly endpoint: string,
    private readonly installationId: string,
    private readonly credential: string,
  ) {}

  public async chooseDownloadRoot(
    request: ChooseDownloadRootRequest,
    signal?: AbortSignal,
  ): Promise<StorageRootSnapshot | null> {
    const source = await companionText(
      new URL(`${API_ROOT}/platform/download-root`, this.endpoint),
      {
        method: "POST",
        credentials: "omit",
        headers: {
          Accept: "application/json",
          Authorization: `Bearer ${this.credential}`,
          "Content-Type": "application/json",
          "X-RSTorrent-Installation": this.installationId,
        },
        body: JSON.stringify(request),
        ...(signal === undefined ? {} : { signal }),
      },
    );
    return decodeChooseDownloadRootResponse(source).root;
  }

  public async close(): Promise<void> {}
}

class AndroidCompanionApplicationClient implements ApplicationViewClient {
  public constructor(private readonly client: WebSocketApplicationViewClient) {}

  public hello(signal?: AbortSignal): Promise<ApiHello> {
    return this.client.hello(signal);
  }

  public dispatch(
    request: RequestEnvelope,
    signal?: AbortSignal,
  ): Promise<ResponseEnvelope> {
    return this.client.dispatch(request, signal);
  }

  public addTorrentBytes(
    request: AddTorrentBytesRequest,
    source: ArrayBuffer,
    signal?: AbortSignal,
  ): Promise<ResponseEnvelope> {
    return this.client.addTorrentBytes(request, source, signal);
  }

  public chooseDownloadRoot(
    request: ChooseDownloadRootRequest,
    signal?: AbortSignal,
  ): Promise<StorageRootSnapshot | null> {
    return this.client.chooseDownloadRoot(request, signal);
  }

  public openViewSet(
    request: OpenViewSetRequest,
    signal?: AbortSignal,
  ): Promise<OpenViewSetResponse> {
    return this.client.openViewSet(request, signal);
  }

  public updateViewSet(
    viewSetId: string,
    request: UpdateViewSetRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.client.updateViewSet(viewSetId, request, signal);
  }

  public streamUpdates(
    viewSetId: string,
    after: string,
    signal?: AbortSignal,
  ): Promise<ApplicationUpdateStream> {
    return this.client.streamUpdates(viewSetId, after, signal);
  }

  public closeViewSet(viewSetId: string, signal?: AbortSignal): Promise<void> {
    return this.client.closeViewSet(viewSetId, signal);
  }

  public close(): Promise<void> {
    return this.client.close();
  }
}

export function validateAndroidBackend(hello: ApiHello): ApiBackendIdentity {
  const backend = hello.backend;
  if (backend?.kind !== "android") {
    throw new Error("The authenticated application did not identify as Android");
  }
  boundedIdentifier(backend.instance_id, "backend instance ID");
  boundedText(backend.profile_id, "profile ID", 128);
  boundedText(backend.product_version, "product version", 64);
  if (
    !REQUIRED_PROFILE.every((capability) =>
      backend.capability_profile.includes(capability),
    )
  ) {
    throw new Error("The Android application capability profile is incomplete");
  }
  if (hello.capabilities.includes("torrent_media")) {
    throw new Error("The Android companion unexpectedly advertised media delivery");
  }
  return backend;
}

export function sameBackendIdentity(
  left: ApiBackendIdentity,
  right: ApiBackendIdentity,
): boolean {
  return (
    left.kind === right.kind &&
    left.instance_id === right.instance_id &&
    left.profile_id === right.profile_id
  );
}

async function companionJson(
  endpoint: string,
  path: string,
  init: RequestInit,
): Promise<unknown> {
  const source = await companionText(new URL(`${API_ROOT}${path}`, endpoint), {
    ...init,
    credentials: "omit",
    headers: { Accept: "application/json", ...init.headers },
  });
  return JSON.parse(source) as unknown;
}

async function companionText(url: URL, init: RequestInit): Promise<string> {
  const response = await fetch(
    url,
    withCompanionOrigin(init, globalThis.location.origin),
  );
  const source = await response.text();
  if (new TextEncoder().encode(source).byteLength > MAX_BOOTSTRAP_RESPONSE_BYTES) {
    throw new Error("Android companion response exceeds its bound");
  }
  if (!response.ok) {
    throw new Error(`Android companion request failed (${response.status})`);
  }
  return source;
}

export function withCompanionOrigin(
  init: RequestInit,
  pageOrigin: string,
): RequestInit {
  if (!EXTENSION_ORIGINS.includes(pageOrigin as (typeof EXTENSION_ORIGINS)[number])) {
    throw new Error("Android companion requests require a recognized extension origin");
  }
  const headers = new Headers(init.headers);
  headers.set("Origin", pageOrigin);
  return { ...init, headers };
}

async function readStoredCompanion(): Promise<StoredCompanion | undefined> {
  const value = (await chromeApi().storage.local.get(STORAGE_KEY))[STORAGE_KEY];
  if (typeof value !== "object" || value === null) return undefined;
  const record = value as Record<string, unknown>;
  if (typeof record.installationId !== "string") return undefined;
  boundedIdentifier(record.installationId, "installation ID");
  if (record.credential !== undefined) boundedCredential(record.credential);
  return record as unknown as StoredCompanion;
}

async function writeStoredCompanion(value: StoredCompanion): Promise<void> {
  await chromeApi().storage.local.set({ [STORAGE_KEY]: value });
}

async function clearCredential(installationId: string): Promise<void> {
  await writeStoredCompanion({ installationId });
}

function chromeApi(): ChromeApi {
  const api = (globalThis as typeof globalThis & { chrome?: ChromeApi }).chrome;
  if (api?.storage?.local === undefined) {
    throw new Error("Chrome extension storage is unavailable");
  }
  return api;
}

function randomIdentifier(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function boundedIdentifier(
  value: unknown,
  label: string,
): asserts value is string {
  if (
    typeof value !== "string" ||
    value.length < 16 ||
    value.length > 64 ||
    !/^[A-Za-z0-9_-]+$/u.test(value)
  ) {
    throw new Error(`${label} is invalid`);
  }
}

function boundedCredential(value: unknown): asserts value is string {
  boundedText(value, "pairing credential", 128);
}

function boundedText(
  value: unknown,
  label: string,
  maximum: number,
): asserts value is string {
  if (typeof value !== "string" || value.length === 0 || value.length > maximum) {
    throw new Error(`${label} is invalid`);
  }
}

function delay(milliseconds: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const finish = () => {
      signal?.removeEventListener("abort", abort);
      resolve();
    };
    const timer = window.setTimeout(finish, milliseconds);
    const abort = () => {
      window.clearTimeout(timer);
      signal?.removeEventListener("abort", abort);
      reject(signal?.reason ?? new Error("Android companion connection canceled"));
    };
    if (signal?.aborted) abort();
    else signal?.addEventListener("abort", abort, { once: true });
  });
}
