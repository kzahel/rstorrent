import type {
  PrivateBrowserCredential,
  RemoteAuthorization,
  RemoteHostIdentity,
} from "./remote-application-websocket";

const DATABASE_NAME = "rstorrent-remote-client-v1";
const DATABASE_VERSION = 1;
const HOST_STORE = "hosts";

interface PersistedAuthorization {
  readonly hostResumePublicKey: ArrayBuffer;
  readonly clientId: ArrayBuffer;
  readonly clientPublicKey: ArrayBuffer;
  readonly authorizationGeneration: string;
  readonly clientGeneration: string;
  readonly protocolFloor: number;
  readonly label: string;
  readonly key: CryptoKey;
}

interface PersistedHost {
  readonly username: string;
  readonly relayId: ArrayBuffer;
  readonly hostId: ArrayBuffer;
  readonly hostPin: ArrayBuffer;
  readonly authorization: PersistedAuthorization | null;
}

export interface StoredRemoteClient {
  readonly identity: RemoteHostIdentity;
  readonly authorization?: RemoteAuthorization;
  readonly key?: CryptoKey;
}

export interface RemoteClientStore {
  load(username: string): Promise<StoredRemoteClient | undefined>;
  saveTrust(username: string, identity: RemoteHostIdentity): Promise<void>;
  saveAuthorization(
    authorization: RemoteAuthorization,
    key: CryptoKey,
  ): Promise<void>;
  clearAuthorization(username: string): Promise<void>;
  clearHost(username: string): Promise<void>;
}

export class RemoteHostTrustChanged extends Error {
  public constructor() {
    super("the stored remote host identity does not match the authenticated host");
  }
}

/** Dedicated-origin durable trust and private-browser authorization store. */
export class IndexedDbRemoteClientStore implements RemoteClientStore {
  public constructor(
    private readonly databases: IDBFactory = globalThis.indexedDB,
    private readonly databaseName = DATABASE_NAME,
  ) {}

  public async load(username: string): Promise<StoredRemoteClient | undefined> {
    validateUsername(username);
    const database = await this.open();
    try {
      const transaction = database.transaction(HOST_STORE, "readonly");
      const value = await request<PersistedHost | undefined>(
        transaction.objectStore(HOST_STORE).get(username),
      );
      await transactionDone(transaction);
      return value === undefined ? undefined : decodeHost(value);
    } finally {
      database.close();
    }
  }

  public async saveTrust(
    username: string,
    identity: RemoteHostIdentity,
  ): Promise<void> {
    validateUsername(username);
    validateIdentity(identity);
    const database = await this.open();
    try {
      const transaction = database.transaction(HOST_STORE, "readwrite");
      const store = transaction.objectStore(HOST_STORE);
      const current = await request<PersistedHost | undefined>(store.get(username));
      if (current !== undefined && !sameIdentity(decodeHost(current).identity, identity)) {
        transaction.abort();
        throw new RemoteHostTrustChanged();
      }
      store.put(encodeHost(username, identity, current?.authorization ?? null));
      await transactionDone(transaction);
    } finally {
      database.close();
    }
  }

  public async saveAuthorization(
    authorization: RemoteAuthorization,
    key: CryptoKey,
  ): Promise<void> {
    validateAuthorization(authorization);
    validatePrivateKey(key);
    const database = await this.open();
    try {
      const transaction = database.transaction(HOST_STORE, "readwrite");
      const store = transaction.objectStore(HOST_STORE);
      const current = await request<PersistedHost | undefined>(
        store.get(authorization.username),
      );
      if (
        current !== undefined &&
        !sameIdentity(decodeHost(current).identity, authorization)
      ) {
        transaction.abort();
        throw new RemoteHostTrustChanged();
      }
      store.put({
        ...encodeHost(authorization.username, authorization, null),
        authorization: {
          hostResumePublicKey: exactBuffer(authorization.hostResumePublicKey),
          clientId: exactBuffer(authorization.clientId),
          clientPublicKey: exactBuffer(authorization.clientPublicKey),
          authorizationGeneration: authorization.authorizationGeneration.toString(),
          clientGeneration: authorization.clientGeneration.toString(),
          protocolFloor: authorization.protocolFloor,
          label: authorization.label,
          key,
        },
      } satisfies PersistedHost);
      await transactionDone(transaction);
    } finally {
      database.close();
    }
  }

  public async clearAuthorization(username: string): Promise<void> {
    validateUsername(username);
    const database = await this.open();
    try {
      const transaction = database.transaction(HOST_STORE, "readwrite");
      const store = transaction.objectStore(HOST_STORE);
      const current = await request<PersistedHost | undefined>(store.get(username));
      if (current !== undefined) store.put({ ...current, authorization: null });
      await transactionDone(transaction);
    } finally {
      database.close();
    }
  }

  public async clearHost(username: string): Promise<void> {
    validateUsername(username);
    const database = await this.open();
    try {
      const transaction = database.transaction(HOST_STORE, "readwrite");
      transaction.objectStore(HOST_STORE).delete(username);
      await transactionDone(transaction);
    } finally {
      database.close();
    }
  }

  private async open(): Promise<IDBDatabase> {
    if (this.databases === undefined) {
      throw new Error("durable browser storage is unavailable");
    }
    return new Promise((resolve, reject) => {
      let blocked = false;
      const opening = this.databases.open(this.databaseName, DATABASE_VERSION);
      opening.onupgradeneeded = () => {
        if (!opening.result.objectStoreNames.contains(HOST_STORE)) {
          opening.result.createObjectStore(HOST_STORE, { keyPath: "username" });
        }
      };
      opening.onsuccess = () => {
        if (blocked) opening.result.close();
        else resolve(opening.result);
      };
      opening.onerror = () => reject(opening.error ?? new Error("browser storage failed"));
      opening.onblocked = () => {
        blocked = true;
        reject(new Error("browser storage upgrade is blocked"));
      };
    });
  }
}

export async function createPrivateBrowserCredential(
  label: string,
  clientBuild?: string,
  subtle: SubtleCrypto = globalThis.crypto.subtle,
  random: Pick<Crypto, "getRandomValues"> = globalThis.crypto,
): Promise<PrivateBrowserCredential> {
  validateLabel(label);
  if (clientBuild !== undefined && (clientBuild.length < 1 || clientBuild.length > 160)) {
    throw new Error("client build must contain 1..=160 characters");
  }
  const pair = (await subtle.generateKey(
    { name: "ECDSA", namedCurve: "P-256" },
    false,
    ["sign", "verify"],
  )) as CryptoKeyPair;
  validatePrivateKey(pair.privateKey);
  const publicKey = new Uint8Array(await subtle.exportKey("raw", pair.publicKey));
  if (publicKey.byteLength !== 65) {
    publicKey.fill(0);
    throw new Error("browser P-256 public key encoding is unsupported");
  }
  const clientId = new Uint8Array(16);
  try {
    random.getRandomValues(clientId);
  } catch {
    publicKey.fill(0);
    clientId.fill(0);
    throw new Error("secure browser randomness is unavailable");
  }
  return {
    key: pair.privateKey,
    clientId,
    clientPublicKey: publicKey,
    label,
    ...(clientBuild === undefined ? {} : { clientBuild }),
  };
}

function encodeHost(
  username: string,
  identity: RemoteHostIdentity,
  authorization: PersistedAuthorization | null,
): PersistedHost {
  return {
    username,
    relayId: exactBuffer(identity.relayId),
    hostId: exactBuffer(identity.hostId),
    hostPin: exactBuffer(identity.hostPin),
    authorization,
  };
}

function decodeHost(value: PersistedHost): StoredRemoteClient {
  if (typeof value !== "object" || value === null) throw corruptStore();
  validateUsername(value.username);
  const identity = {
    relayId: decodeBytes(value.relayId, 32),
    hostId: decodeBytes(value.hostId, 32),
    hostPin: decodeBytes(value.hostPin, 64),
  };
  if (value.authorization === null) return { identity };
  const authorization = value.authorization;
  if (typeof authorization !== "object") throw corruptStore();
  const decoded: RemoteAuthorization = {
    ...identity,
    username: value.username,
    hostResumePublicKey: decodeBytes(authorization.hostResumePublicKey, 65),
    clientId: decodeBytes(authorization.clientId, 16),
    clientPublicKey: decodeBytes(authorization.clientPublicKey, 65),
    authorizationGeneration: decodeGeneration(authorization.authorizationGeneration),
    clientGeneration: decodeGeneration(authorization.clientGeneration),
    protocolFloor: authorization.protocolFloor,
    label: authorization.label,
  };
  validateAuthorization(decoded);
  validatePrivateKey(authorization.key);
  return { identity, authorization: decoded, key: authorization.key };
}

function validateIdentity(identity: RemoteHostIdentity): void {
  if (
    identity.relayId.byteLength !== 32 ||
    identity.hostId.byteLength !== 32 ||
    identity.hostPin.byteLength !== 64
  ) {
    throw new Error("remote host identity is invalid");
  }
}

function validateAuthorization(authorization: RemoteAuthorization): void {
  validateIdentity(authorization);
  validateUsername(authorization.username);
  validateLabel(authorization.label);
  if (
    authorization.hostResumePublicKey.byteLength !== 65 ||
    authorization.clientId.byteLength !== 16 ||
    authorization.clientPublicKey.byteLength !== 65 ||
    authorization.authorizationGeneration < 1n ||
    authorization.clientGeneration < 1n ||
    !Number.isSafeInteger(authorization.protocolFloor) ||
    authorization.protocolFloor < 1
  ) {
    throw corruptStore();
  }
}

function validatePrivateKey(key: CryptoKey): void {
  const algorithm = key?.algorithm as EcKeyAlgorithm | undefined;
  if (
    key?.type !== "private" ||
    key.extractable ||
    algorithm?.name !== "ECDSA" ||
    algorithm.namedCurve !== "P-256" ||
    key.usages.length !== 1 ||
    key.usages[0] !== "sign"
  ) {
    throw new Error("browser resume key is not a non-extractable P-256 signing key");
  }
}

function validateUsername(username: string): void {
  if (
    username.length < 3 ||
    username.length > 32 ||
    !/^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/.test(username)
  ) {
    throw new Error("invalid remote username");
  }
}

function validateLabel(label: string): void {
  if (label.trim() !== label || label.length < 1 || label.length > 80) {
    throw new Error("browser label must contain 1..=80 trimmed characters");
  }
}

function decodeGeneration(value: unknown): bigint {
  if (typeof value !== "string" || !/^[1-9][0-9]{0,19}$/.test(value)) {
    throw corruptStore();
  }
  return BigInt(value);
}

function decodeBytes(value: unknown, length: number): Uint8Array {
  if (!(value instanceof ArrayBuffer) || value.byteLength !== length) {
    throw corruptStore();
  }
  return new Uint8Array(value.slice(0));
}

function exactBuffer(value: Uint8Array): ArrayBuffer {
  return value.slice().buffer;
}

function sameIdentity(left: RemoteHostIdentity, right: RemoteHostIdentity): boolean {
  return (
    equalBytes(left.relayId, right.relayId) &&
    equalBytes(left.hostId, right.hostId) &&
    equalBytes(left.hostPin, right.hostPin)
  );
}

function equalBytes(left: Uint8Array, right: Uint8Array): boolean {
  if (left.byteLength !== right.byteLength) return false;
  let difference = 0;
  for (let index = 0; index < left.byteLength; index += 1) {
    difference |= left[index]! ^ right[index]!;
  }
  return difference === 0;
}

function request<T>(value: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    value.onsuccess = () => resolve(value.result);
    value.onerror = () => reject(value.error ?? new Error("browser storage failed"));
  });
}

function transactionDone(transaction: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    transaction.oncomplete = () => resolve();
    transaction.onabort = () => reject(transaction.error ?? new Error("browser storage aborted"));
    transaction.onerror = () => reject(transaction.error ?? new Error("browser storage failed"));
  });
}

function corruptStore(): Error {
  return new Error("stored remote authorization is invalid");
}
