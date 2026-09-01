export const PRODUCT_METRICS_KEY = "productMetricsV1";
export const PRODUCT_METRICS_DISCLOSURE_VERSION = 1;
export const UNINSTALL_BASE_URL = "https://jstorrent.com/uninstall.html";
export const PRIVACY_URL = "https://jstorrent.com/privacy.html";
export const MAX_UNINSTALL_URL_LENGTH = 1023;
export const HOSTED_PRODUCT_CONTEXT_READY = false;

const UUID_V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u;
const MAX_U64 = 18_446_744_073_709_551_615n;
const STATE_KEYS = [
  "backendSummary",
  "createdAtMillis",
  "currentVersion",
  "disclosureVersion",
  "everConnected",
  "firstVersion",
  "installationId",
  "schemaVersion",
  "sessions",
  "statisticsEnabled",
];

export class ProductMetricsOwner {
  #queue = Promise.resolve();

  constructor(chromeApi, extensionVersion, now = () => Date.now()) {
    this.chrome = chromeApi;
    this.extensionVersion = validateVersion(extensionVersion);
    this.now = now;
  }

  start() {
    this.chrome.runtime.onInstalled.addListener(() => this.refresh());
    this.chrome.runtime.onStartup.addListener(() => this.refresh());
    this.chrome.storage.onChanged.addListener((changes, area) => {
      if (area === "local" && changes[PRODUCT_METRICS_KEY]) this.refresh();
    });
    this.refresh();
  }

  snapshot() {
    return this.#serialize(async () => (await this.#load()).state);
  }

  acknowledge(statisticsEnabled) {
    return this.#mutate((state) => ({
      ...state,
      disclosureVersion: PRODUCT_METRICS_DISCLOSURE_VERSION,
      statisticsEnabled: Boolean(statisticsEnabled),
      backendSummary: statisticsEnabled ? state.backendSummary : null,
    }));
  }

  setStatisticsEnabled(statisticsEnabled) {
    return this.#mutate((state) => {
      if (state.disclosureVersion !== PRODUCT_METRICS_DISCLOSURE_VERSION) {
        throw new Error("statistics preference requires current disclosure acknowledgement");
      }
      return {
        ...state,
        statisticsEnabled: Boolean(statisticsEnabled),
        backendSummary: statisticsEnabled ? state.backendSummary : null,
      };
    });
  }

  reset() {
    return this.#mutate((state) => ({
      ...state,
      installationId: crypto.randomUUID(),
      createdAtMillis: String(this.now()),
      sessions: "0",
      everConnected: false,
      backendSummary: null,
    }));
  }

  recordSession() {
    return this.#mutate((state) => ({ ...state, sessions: increment(state.sessions) }));
  }

  recordConnected() {
    return this.#mutate((state) => ({ ...state, everConnected: true }));
  }

  cacheBackendSummary(summary, backendPermits) {
    return this.#mutate((state) => ({
      ...state,
      backendSummary:
        state.statisticsEnabled && backendPermits
          ? validateBackendSummary(summary)
          : null,
    }));
  }

  refresh() {
    return this.#serialize(async () => {
      const loaded = await this.#load();
      await this.#register(loaded.malformed ? UNINSTALL_BASE_URL : buildUninstallUrl(loaded.state, this.now()));
      return loaded.state;
    });
  }

  #mutate(change) {
    return this.#serialize(async () => {
      const loaded = await this.#load();
      const state = normalizeState(change(loaded.state), this.extensionVersion);
      await this.chrome.storage.local.set({ [PRODUCT_METRICS_KEY]: state });
      await this.#register(buildUninstallUrl(state, this.now()));
      return state;
    });
  }

  #serialize(operation) {
    const next = this.#queue.then(operation, operation);
    this.#queue = next.catch(() => undefined);
    return next;
  }

  async #load() {
    const stored = (await this.chrome.storage.local.get(PRODUCT_METRICS_KEY))?.[PRODUCT_METRICS_KEY];
    if (stored === undefined) {
      const state = freshState(this.extensionVersion, this.now);
      await this.chrome.storage.local.set({ [PRODUCT_METRICS_KEY]: state });
      return { state, malformed: false };
    }
    try {
      const state = normalizeState(stored, this.extensionVersion);
      if (JSON.stringify(state) !== JSON.stringify(stored)) {
        await this.chrome.storage.local.set({ [PRODUCT_METRICS_KEY]: state });
      }
      return { state, malformed: false };
    } catch {
      const state = freshState(this.extensionVersion, this.now);
      await this.chrome.storage.local.set({ [PRODUCT_METRICS_KEY]: state });
      return { state, malformed: true };
    }
  }

  async #register(url) {
    try {
      await this.chrome.runtime.setUninstallURL(url);
    } catch (error) {
      console.error("product uninstall URL registration failed", String(error).slice(0, 256));
    }
  }
}

export function buildUninstallUrl(state, now = () => Date.now()) {
  const normalized = normalizeState(state, state.currentVersion);
  const url = new URL(UNINSTALL_BASE_URL);
  url.searchParams.set("v", normalized.currentVersion);
  if (
    HOSTED_PRODUCT_CONTEXT_READY &&
    normalized.disclosureVersion === PRODUCT_METRICS_DISCLOSURE_VERSION &&
    normalized.statisticsEnabled
  ) {
    url.searchParams.set("id", normalized.installationId);
    url.searchParams.set("days", String(daysSince(normalized.createdAtMillis, now())));
    url.searchParams.set("connected", normalized.everConnected ? "1" : "0");
    if (normalized.backendSummary !== null) {
      url.searchParams.set("downloads", normalized.backendSummary.completed);
      url.searchParams.set("added", normalized.backendSummary.added);
    }
    url.searchParams.set("sessions", normalized.sessions);
  }
  const result = url.toString();
  if (result.length > MAX_UNINSTALL_URL_LENGTH) {
    throw new Error("uninstall URL exceeds Chrome's 1,023-character limit");
  }
  return result;
}

function freshState(extensionVersion, now) {
  return {
    schemaVersion: 1,
    installationId: crypto.randomUUID(),
    createdAtMillis: String(now()),
    firstVersion: extensionVersion,
    currentVersion: extensionVersion,
    sessions: "0",
    everConnected: false,
    disclosureVersion: 0,
    statisticsEnabled: true,
    backendSummary: null,
  };
}

function normalizeState(value, extensionVersion) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid metrics state");
  if (JSON.stringify(Object.keys(value).sort()) !== JSON.stringify(STATE_KEYS)) throw new Error("metrics state shape changed");
  if (value.schemaVersion !== 1 || !UUID_V4.test(value.installationId)) throw new Error("invalid metrics identity");
  const createdAtMillis = validateU64(value.createdAtMillis);
  const firstVersion = validateVersion(value.firstVersion);
  validateVersion(value.currentVersion);
  const sessions = validateU64(value.sessions);
  if (typeof value.everConnected !== "boolean" || typeof value.statisticsEnabled !== "boolean") throw new Error("invalid metrics flags");
  if (![0, PRODUCT_METRICS_DISCLOSURE_VERSION].includes(value.disclosureVersion)) throw new Error("invalid disclosure version");
  const backendSummary = value.backendSummary === null ? null : validateBackendSummary(value.backendSummary);
  return {
    schemaVersion: 1,
    installationId: value.installationId,
    createdAtMillis,
    firstVersion,
    currentVersion: validateVersion(extensionVersion),
    sessions,
    everConnected: value.everConnected,
    disclosureVersion: value.disclosureVersion,
    statisticsEnabled: value.statisticsEnabled,
    backendSummary: value.statisticsEnabled ? backendSummary : null,
  };
}

function validateBackendSummary(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid backend summary");
  if (JSON.stringify(Object.keys(value).sort()) !== JSON.stringify(["added", "completed"])) throw new Error("backend summary shape changed");
  return { added: validateU64(value.added), completed: validateU64(value.completed) };
}

function validateVersion(value) {
  if (typeof value !== "string" || value.length < 1 || value.length > 128 || /[\u0000-\u001f\u007f]/u.test(value)) throw new Error("invalid product version");
  return value;
}

function validateU64(value) {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]{0,19})$/u.test(value)) throw new Error("invalid unsigned counter");
  const number = BigInt(value);
  if (number > MAX_U64) throw new Error("unsigned counter exceeds u64");
  return String(number);
}

function increment(value) {
  const current = BigInt(validateU64(value));
  return String(current === MAX_U64 ? MAX_U64 : current + 1n);
}

function daysSince(createdAtMillis, now) {
  const created = BigInt(validateU64(createdAtMillis));
  const observed = BigInt(Math.max(0, Math.trunc(now)));
  return observed <= created ? 0 : Number((observed - created) / 86_400_000n);
}
