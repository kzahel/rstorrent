import { remoteCryptoOperationEntropy } from "./remote-crypto-entropy";
import type {
  ApplicationWebSocket,
  ApplicationWebSocketFactory,
} from "./websocket-view-client";

const SOCKET_CONNECTING = 0;
const SOCKET_OPEN = 1;
const SOCKET_CLOSING = 2;
const SOCKET_CLOSED = 3;
const HANDSHAKE_TIMEOUT_MILLIS = 20_000;
const REMOTE_FAILURE_CLOSE_CODE = 4_008;
const CLIENT_SELECT_MAGIC = bytes("RSC1");
const PAIRED_CONTROL = bytes("RSP1");
const HOST_GREETING = bytes("RHG1");
const LOGIN_REQUEST = bytes("RSL1");
const LOGIN_RESPONSE = bytes("RSL2");
const LOGIN_FINALIZATION = bytes("RSL3");
const RESUME_REQUEST = bytes("RSR1");
const RESUME_RESPONSE = bytes("RSR2");
const RESUME_FINALIZATION = bytes("RSR3");
const AUTHENTICATED_READY = bytes("RSA2");
const AUTHORIZATION_CHOICE = bytes("RSA3");
const AUTHENTICATION_SUCCEEDED = bytes("RSA4");

export type RemoteConnectionFailure =
  | "connection_failed"
  | "host_identity_changed"
  | "resume_rejected";

export interface RemoteHostIdentity {
  readonly relayId: Uint8Array;
  readonly hostId: Uint8Array;
  readonly hostPin: Uint8Array;
}

export interface RemoteAuthorization extends RemoteHostIdentity {
  readonly username: string;
  readonly hostResumePublicKey: Uint8Array;
  readonly clientId: Uint8Array;
  readonly clientPublicKey: Uint8Array;
  readonly authorizationGeneration: bigint;
  readonly clientGeneration: bigint;
  readonly protocolFloor: number;
  readonly label: string;
}

export interface PrivateBrowserCredential {
  readonly key: CryptoKey;
  readonly clientId: Uint8Array;
  readonly clientPublicKey: Uint8Array;
  readonly label: string;
  readonly clientBuild?: string;
  readonly routeObservation?: string;
  readonly browserObservation?: string;
}

export type RemoteAuthentication =
  | {
      readonly type: "password";
      readonly passphrase: Uint8Array;
      readonly choice:
        | { readonly type: "shared"; readonly clientBuild?: string }
        | {
            readonly type: "private";
            readonly credential: PrivateBrowserCredential;
          };
      readonly expectedIdentity?: RemoteHostIdentity;
    }
  | {
      readonly type: "resume";
      readonly authorization: RemoteAuthorization;
      readonly key: CryptoKey;
    };

export interface AuthenticationReady {
  readonly protocol_version: number;
  readonly host_build: string;
  readonly host_pin: string;
  readonly host_resume_public_key: string;
  readonly authorization_generation: number;
  readonly authorization_challenge: string;
  readonly protocol_floor: number;
}

export interface WasmOpenedRecord {
  readonly plaintext: Uint8Array;
  readonly isClose: boolean;
}

export interface WasmClientSession {
  take_finalization(): Uint8Array;
  host_pin(): Uint8Array;
  seal(plaintext: Uint8Array): Uint8Array;
  seal_close(): Uint8Array;
  open(record: Uint8Array): WasmOpenedRecord;
}

export interface WasmClientLogin {
  request(): Uint8Array;
  finish(
    passphrase: Uint8Array,
    relayId: Uint8Array,
    username: string,
    hostId: Uint8Array,
    expectedPin: Uint8Array,
    response: Uint8Array,
    entropy: Uint8Array,
  ): WasmClientSession;
}

export interface WasmClientResumeProof {
  signature_input(): Uint8Array;
  complete(signature: Uint8Array): WasmClientSession;
}

export interface WasmClientResume {
  request(): Uint8Array;
  finish(challenge: Uint8Array): WasmClientResumeProof;
}

export interface RemoteCryptoWasmModule {
  readonly ClientLogin: new (
    passphrase: Uint8Array,
    entropy: Uint8Array,
  ) => WasmClientLogin;
  readonly ClientResume: new (
    relayId: Uint8Array,
    username: string,
    hostId: Uint8Array,
    hostPin: Uint8Array,
    hostResumePublicKey: Uint8Array,
    clientId: Uint8Array,
    clientPublicKey: Uint8Array,
    authorizationGeneration: bigint,
    clientGeneration: bigint,
    protocolFloor: number,
    entropy: Uint8Array,
  ) => WasmClientResume;
  authorizationTranscript(
    relayId: Uint8Array,
    username: string,
    hostId: Uint8Array,
    hostPin: Uint8Array,
    hostResumePublicKey: Uint8Array,
    authorizationGeneration: bigint,
    authorizationChallenge: Uint8Array,
    clientPublicKey: Uint8Array,
    label: string,
    clientBuild?: string,
    routeObservation?: string,
    browserObservation?: string,
  ): Uint8Array;
}

export interface RemoteConnectionOptions {
  readonly relayUrl: string;
  readonly username: string;
  readonly authentication: RemoteAuthentication;
  readonly crypto: RemoteCryptoWasmModule;
  readonly socketFactory?: ApplicationWebSocketFactory;
  readonly entropy?: () => Uint8Array;
  readonly subtle?: SubtleCrypto;
  readonly onHostPin?: (identity: RemoteHostIdentity) => void | Promise<void>;
  readonly onAuthorization?: (
    authorization: RemoteAuthorization,
  ) => void | Promise<void>;
  readonly onFailure?: (failure: RemoteConnectionFailure) => void;
}

type HandshakePhase =
  | "pair"
  | "greeting"
  | "login_response"
  | "password_ready"
  | "resume_response"
  | "resume_outcome"
  | "password_outcome"
  | "application";

interface HostGreetingValue {
  readonly relayId: Uint8Array;
  readonly hostId: Uint8Array;
  readonly protocolVersion: number;
}

interface AuthenticationSucceeded {
  readonly protocol_version: number;
  readonly authorization: null | {
    readonly client_id: string;
    readonly fingerprint: string;
  };
}

class HostIdentityChanged extends Error {}

/** ApplicationWebSocket-compatible encrypted product relay transport. */
export class RemoteApplicationWebSocket implements ApplicationWebSocket {
  public binaryType: BinaryType = "arraybuffer";
  public onopen: ((event: Event) => void) | null = null;
  public onmessage: ((event: MessageEvent<unknown>) => void) | null = null;
  public onerror: ((event: Event) => void) | null = null;
  public onclose: ((event: CloseEvent) => void) | null = null;

  private state = SOCKET_CONNECTING;
  private phase: HandshakePhase = "pair";
  private readonly socket: ApplicationWebSocket;
  private readonly passphrase: Uint8Array;
  private readonly expectedHostPin: Uint8Array;
  private login: WasmClientLogin | undefined;
  private resume: WasmClientResume | undefined;
  private session: WasmClientSession | undefined;
  private greeting: HostGreetingValue | undefined;
  private ready: AuthenticationReady | undefined;
  private readonly timer: ReturnType<typeof setTimeout>;
  private failed = false;

  public constructor(private readonly options: RemoteConnectionOptions) {
    validateOptions(options);
    this.passphrase =
      options.authentication.type === "password"
        ? options.authentication.passphrase.slice()
        : new Uint8Array();
    this.expectedHostPin = expectedIdentity(options)?.hostPin.slice() ?? new Uint8Array();
    const factory = options.socketFactory ?? ((url) => new WebSocket(url));
    this.socket = factory(clientRelayUrl(options.relayUrl));
    this.socket.binaryType = "arraybuffer";
    this.socket.onopen = () => {
      try {
        this.socket.send(exactBuffer(encodeClientSelect(options.username)));
      } catch (error) {
        this.fail(error);
      }
    };
    this.socket.onmessage = (event) => {
      void this.receive(event.data).catch((error: unknown) => this.fail(error));
    };
    this.socket.onerror = () => this.fail(new Error("relay transport failed"));
    this.socket.onclose = (event) => {
      if (this.state === SOCKET_CONNECTING && !this.failed) {
        const resumeRejected =
          this.options.authentication.type === "resume" &&
          this.phase !== "pair" &&
          this.phase !== "greeting";
        this.failed = true;
        this.options.onFailure?.(
          resumeRejected ? "resume_rejected" : "connection_failed",
        );
        this.onerror?.(new Event("error"));
      }
      this.closed(event);
    };
    this.timer = setTimeout(
      () => this.fail(new Error("remote authentication timed out")),
      HANDSHAKE_TIMEOUT_MILLIS,
    );
  }

  public get readyState(): number {
    return this.state;
  }

  public send(data: string | ArrayBuffer): void {
    if (this.state !== SOCKET_OPEN || this.session === undefined) {
      throw new Error("remote application connection is not open");
    }
    if (data instanceof ArrayBuffer) {
      throw new Error("remote torrent byte attachments are unsupported");
    }
    rejectUnsupportedApplicationBreadth(data);
    this.socket.send(exactBuffer(this.session.seal(new TextEncoder().encode(data))));
  }

  public close(code = 1_000, reason = ""): void {
    if (this.state === SOCKET_CLOSED || this.state === SOCKET_CLOSING) return;
    this.state = SOCKET_CLOSING;
    clearTimeout(this.timer);
    this.eraseHandshakeSecrets();
    if (this.session !== undefined) {
      try {
        this.socket.send(exactBuffer(this.session.seal_close()));
      } catch {
        // The authenticated record channel may already be terminal.
      }
    }
    this.socket.close(code, reason);
  }

  private async receive(data: unknown): Promise<void> {
    const message = binaryMessage(data);
    switch (this.phase) {
      case "pair":
        if (!equalBytes(message, PAIRED_CONTROL)) throw new Error("pair failed");
        this.phase = "greeting";
        return;
      case "greeting":
        this.beginAuthentication(decodeHostGreeting(message));
        return;
      case "login_response":
        this.finishPasswordLogin(payload(message, LOGIN_RESPONSE));
        return;
      case "password_ready":
        await this.acceptPasswordReady(message);
        return;
      case "resume_response":
        await this.acceptResumeChallenge(payload(message, RESUME_RESPONSE));
        return;
      case "resume_outcome":
      case "password_outcome":
        await this.acceptAuthenticationOutcome(message);
        return;
      case "application":
        this.acceptApplicationRecord(message);
        return;
    }
  }

  private beginAuthentication(greeting: HostGreetingValue): void {
    if (greeting.protocolVersion !== 1) throw new Error("unsupported host protocol");
    const expected = expectedIdentity(this.options);
    if (
      expected !== undefined &&
      (!equalBytes(expected.relayId, greeting.relayId) ||
        !equalBytes(expected.hostId, greeting.hostId))
    ) {
      throw new HostIdentityChanged("host identity changed");
    }
    this.greeting = greeting;
    if (this.options.authentication.type === "password") {
      this.login = new this.options.crypto.ClientLogin(
        this.passphrase,
        this.operationEntropy(),
      );
      this.socket.send(exactBuffer(framed(LOGIN_REQUEST, this.login.request())));
      this.phase = "login_response";
      return;
    }
    const authorization = this.options.authentication.authorization;
    this.resume = new this.options.crypto.ClientResume(
      greeting.relayId,
      this.options.username,
      greeting.hostId,
      authorization.hostPin,
      authorization.hostResumePublicKey,
      authorization.clientId,
      authorization.clientPublicKey,
      authorization.authorizationGeneration,
      authorization.clientGeneration,
      authorization.protocolFloor,
      this.operationEntropy(),
    );
    this.socket.send(
      exactBuffer(
        framed(
          RESUME_REQUEST,
          concatenate(authorization.clientId, this.resume.request()),
        ),
      ),
    );
    this.phase = "resume_response";
  }

  private finishPasswordLogin(response: Uint8Array): void {
    const greeting = required(this.greeting, "host greeting");
    const login = required(this.login, "login state");
    let session: WasmClientSession;
    try {
      session = login.finish(
        this.passphrase,
        greeting.relayId,
        this.options.username,
        greeting.hostId,
        this.expectedHostPin,
        response,
        this.operationEntropy(),
      );
    } catch (error) {
      if (errorMessage(error) === "host identity changed") {
        throw new HostIdentityChanged("host identity changed");
      }
      throw error;
    }
    this.login = undefined;
    this.passphrase.fill(0);
    this.expectedHostPin.fill(0);
    this.session = session;
    this.socket.send(
      exactBuffer(framed(LOGIN_FINALIZATION, session.take_finalization())),
    );
    this.phase = "password_ready";
  }

  private async acceptPasswordReady(record: Uint8Array): Promise<void> {
    const session = required(this.session, "record state");
    const opened = session.open(record);
    if (opened.isClose) throw new Error("authenticated host closed");
    const ready = decodeJsonRecord<AuthenticationReady>(
      opened.plaintext,
      AUTHENTICATED_READY,
    );
    validateReady(ready);
    const pin = decodeId(ready.host_pin, 64, "host pin");
    if (!equalBytes(pin, session.host_pin())) {
      throw new Error("authenticated host pin is inconsistent");
    }
    this.ready = ready;
    const authentication = this.options.authentication;
    if (authentication.type !== "password") throw new Error("invalid authentication state");
    let choice: unknown;
    if (authentication.choice.type === "shared") {
      choice = {
        choice: "shared",
        ...(authentication.choice.clientBuild === undefined
          ? {}
          : { client_build: authentication.choice.clientBuild }),
      };
    } else {
      const greeting = required(this.greeting, "host greeting");
      const credential = authentication.choice.credential;
      const transcript = this.options.crypto.authorizationTranscript(
        greeting.relayId,
        this.options.username,
        greeting.hostId,
        pin,
        decodeId(ready.host_resume_public_key, 65, "host resume public key"),
        BigInt(ready.authorization_generation),
        decodeId(ready.authorization_challenge, 32, "authorization challenge"),
        credential.clientPublicKey,
        credential.label,
        credential.clientBuild,
        credential.routeObservation,
        credential.browserObservation,
      );
      const signature = await signP256(
        this.options.subtle ?? crypto.subtle,
        credential.key,
        transcript,
      );
      transcript.fill(0);
      choice = {
        choice: "private",
        client_id: encodeId(credential.clientId),
        client_public_key: encodeId(credential.clientPublicKey),
        signature: encodeId(signature),
        label: credential.label,
        client_build: credential.clientBuild ?? null,
        route_observation: credential.routeObservation ?? null,
        browser_observation: credential.browserObservation ?? null,
      };
      signature.fill(0);
    }
    const plaintext = encodeJsonRecord(AUTHORIZATION_CHOICE, choice);
    this.socket.send(exactBuffer(session.seal(plaintext)));
    this.phase = "password_outcome";
  }

  private async acceptResumeChallenge(challenge: Uint8Array): Promise<void> {
    const resume = required(this.resume, "resume state");
    const proof = resume.finish(challenge);
    this.resume = undefined;
    const signatureInput = proof.signature_input();
    const authentication = this.options.authentication;
    if (authentication.type !== "resume") throw new Error("invalid resume state");
    const signature = await signP256(
      this.options.subtle ?? crypto.subtle,
      authentication.key,
      signatureInput,
    );
    signatureInput.fill(0);
    const session = proof.complete(signature);
    signature.fill(0);
    this.session = session;
    this.socket.send(
      exactBuffer(framed(RESUME_FINALIZATION, session.take_finalization())),
    );
    this.phase = "resume_outcome";
  }

  private async acceptAuthenticationOutcome(record: Uint8Array): Promise<void> {
    const session = required(this.session, "record state");
    const opened = session.open(record);
    if (opened.isClose) throw new Error("authenticated host closed");
    const outcome = decodeJsonRecord<AuthenticationSucceeded>(
      opened.plaintext,
      AUTHENTICATION_SUCCEEDED,
    );
    validateOutcome(outcome);
    const authentication = this.options.authentication;
    if (authentication.type === "resume") {
      if (
        outcome.authorization === null ||
        outcome.authorization.client_id !== encodeId(authentication.authorization.clientId)
      ) {
        throw new Error("resume authorization acknowledgement is invalid");
      }
    } else if (authentication.choice.type === "private") {
      const credential = authentication.choice.credential;
      if (
        outcome.authorization === null ||
        outcome.authorization.client_id !== encodeId(credential.clientId)
      ) {
        throw new Error("private authorization acknowledgement is invalid");
      }
      const greeting = required(this.greeting, "host greeting");
      const ready = required(this.ready, "authenticated readiness");
      const identity = {
        relayId: greeting.relayId.slice(),
        hostId: greeting.hostId.slice(),
        hostPin: session.host_pin().slice(),
      };
      await this.options.onAuthorization?.({
        ...identity,
        username: this.options.username,
        hostResumePublicKey: decodeId(
          ready.host_resume_public_key,
          65,
          "host resume public key",
        ),
        clientId: credential.clientId.slice(),
        clientPublicKey: credential.clientPublicKey.slice(),
        authorizationGeneration: BigInt(ready.authorization_generation),
        clientGeneration: 1n,
        protocolFloor: ready.protocol_floor,
        label: credential.label,
      });
      await this.options.onHostPin?.(identity);
    } else {
      if (outcome.authorization !== null) {
        throw new Error("shared browser unexpectedly received authorization");
      }
      const greeting = required(this.greeting, "host greeting");
      await this.options.onHostPin?.({
        relayId: greeting.relayId.slice(),
        hostId: greeting.hostId.slice(),
        hostPin: session.host_pin().slice(),
      });
    }
    clearTimeout(this.timer);
    this.eraseHandshakeSecrets();
    this.state = SOCKET_OPEN;
    this.phase = "application";
    this.onopen?.(new Event("open"));
  }

  private acceptApplicationRecord(record: Uint8Array): void {
    const session = required(this.session, "record state");
    const opened = session.open(record);
    if (opened.isClose) {
      this.close(1_000, "authenticated remote close");
      return;
    }
    const text = new TextDecoder("utf-8", { fatal: true }).decode(opened.plaintext);
    this.onmessage?.(new MessageEvent("message", { data: text }));
  }

  private fail(error: unknown): void {
    if (this.failed || this.state === SOCKET_CLOSED) return;
    this.failed = true;
    this.options.onFailure?.(
      error instanceof HostIdentityChanged
        ? "host_identity_changed"
        : this.options.authentication.type === "resume" &&
            this.phase !== "pair" &&
            this.phase !== "greeting"
          ? "resume_rejected"
          : "connection_failed",
    );
    this.onerror?.(new Event("error"));
    this.close(REMOTE_FAILURE_CLOSE_CODE, "remote connection failed");
  }

  private closed(event: CloseEvent): void {
    clearTimeout(this.timer);
    this.eraseHandshakeSecrets();
    this.state = SOCKET_CLOSED;
    this.onclose?.(event);
  }

  private operationEntropy(): Uint8Array {
    const entropy = this.options.entropy?.() ?? remoteCryptoOperationEntropy();
    if (entropy.byteLength !== 32) {
      entropy.fill(0);
      throw new Error("secure browser randomness is unavailable");
    }
    return entropy;
  }

  private eraseHandshakeSecrets(): void {
    this.passphrase.fill(0);
    this.expectedHostPin.fill(0);
    this.login = undefined;
    this.resume = undefined;
  }
}

async function signP256(
  subtle: SubtleCrypto,
  key: CryptoKey,
  message: Uint8Array,
): Promise<Uint8Array> {
  const signature = new Uint8Array(
    await subtle.sign({ name: "ECDSA", hash: "SHA-256" }, key, exactBuffer(message)),
  );
  if (signature.byteLength !== 64) {
    signature.fill(0);
    throw new Error("browser ECDSA signature encoding is unsupported");
  }
  return signature;
}

function expectedIdentity(options: RemoteConnectionOptions): RemoteHostIdentity | undefined {
  return options.authentication.type === "resume"
    ? options.authentication.authorization
    : options.authentication.expectedIdentity;
}

function decodeHostGreeting(message: Uint8Array): HostGreetingValue {
  if (message.byteLength !== 70 || !equalBytes(message.subarray(0, 4), HOST_GREETING)) {
    throw new Error("invalid host greeting");
  }
  return {
    relayId: message.slice(4, 36),
    hostId: message.slice(36, 68),
    protocolVersion: (message[68]! << 8) | message[69]!,
  };
}

function validateReady(value: AuthenticationReady): void {
  if (
    !hasExactKeys(value, [
      "authorization_challenge",
      "authorization_generation",
      "host_build",
      "host_pin",
      "host_resume_public_key",
      "protocol_floor",
      "protocol_version",
    ]) ||
    value.protocol_version !== 1 ||
    !Number.isSafeInteger(value.authorization_generation) ||
    value.authorization_generation < 1 ||
    !Number.isSafeInteger(value.protocol_floor) ||
    value.protocol_floor < 1 ||
    typeof value.host_build !== "string" ||
    value.host_build.length < 1 ||
    value.host_build.length > 160
  ) {
    throw new Error("invalid authenticated readiness");
  }
  decodeId(value.host_pin, 64, "host pin");
  decodeId(value.host_resume_public_key, 65, "host resume public key");
  decodeId(value.authorization_challenge, 32, "authorization challenge");
}

function validateOutcome(value: AuthenticationSucceeded): void {
  if (
    !hasExactKeys(value, ["authorization", "protocol_version"]) ||
    value.protocol_version !== 1
  ) {
    throw new Error("invalid authentication result");
  }
  if (value.authorization !== null) {
    if (!hasExactKeys(value.authorization, ["client_id", "fingerprint"])) {
      throw new Error("invalid authentication result");
    }
    decodeId(value.authorization.client_id, 16, "client ID");
    if (
      typeof value.authorization.fingerprint !== "string" ||
      value.authorization.fingerprint.length < 1 ||
      value.authorization.fingerprint.length > 128
    ) {
      throw new Error("invalid authorization fingerprint");
    }
  }
}

function hasExactKeys(value: unknown, expected: readonly string[]): value is object {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const keys = Object.keys(value).sort();
  return keys.length === expected.length && keys.every((key, index) => key === expected[index]);
}

function encodeJsonRecord(magic: Uint8Array, value: unknown): Uint8Array {
  return framed(magic, new TextEncoder().encode(JSON.stringify(value)));
}

function decodeJsonRecord<T>(message: Uint8Array, magic: Uint8Array): T {
  const encoded = payload(message, magic);
  if (encoded.byteLength > 2 * 1024) throw new Error("JSON record exceeds size limit");
  return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(encoded)) as T;
}

function rejectUnsupportedApplicationBreadth(encoded: string): void {
  let frame: unknown;
  try {
    frame = JSON.parse(encoded);
  } catch {
    throw new Error("remote application frame is invalid JSON");
  }
  if (typeof frame !== "object" || frame === null) return;
  const candidate = frame as { type?: unknown; operation?: { type?: unknown } };
  if (candidate.type === "begin_torrent_upload") {
    throw new Error("remote torrent byte attachments are unsupported");
  }
  if (candidate.type === "call" && candidate.operation?.type === "create_media_url") {
    throw new Error("remote media capabilities are unsupported");
  }
}

function validateOptions(options: RemoteConnectionOptions): void {
  encodeClientSelect(options.username);
  const identity = expectedIdentity(options);
  if (
    identity !== undefined &&
    (identity.relayId.byteLength !== 32 ||
      identity.hostId.byteLength !== 32 ||
      identity.hostPin.byteLength !== 64)
  ) {
    throw new Error("remote host identity has invalid lengths");
  }
  if (options.authentication.type === "resume") {
    const authorization = options.authentication.authorization;
    if (
      authorization.username !== options.username ||
      authorization.hostResumePublicKey.byteLength !== 65 ||
      authorization.clientId.byteLength !== 16 ||
      authorization.clientPublicKey.byteLength !== 65 ||
      authorization.authorizationGeneration < 1n ||
      authorization.clientGeneration < 1n ||
      authorization.protocolFloor < 1
    ) {
      throw new Error("remote authorization is invalid");
    }
  } else if (options.authentication.choice.type === "private") {
    const credential = options.authentication.choice.credential;
    if (credential.clientId.byteLength !== 16 || credential.clientPublicKey.byteLength !== 65) {
      throw new Error("private browser credential is invalid");
    }
  }
}

function encodeClientSelect(username: string): Uint8Array<ArrayBuffer> {
  const encoded = new TextEncoder().encode(username);
  if (
    encoded.byteLength < 3 ||
    encoded.byteLength > 32 ||
    !/^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/.test(username)
  ) {
    throw new Error("invalid remote username");
  }
  const message = new Uint8Array(5 + encoded.byteLength);
  message.set(CLIENT_SELECT_MAGIC);
  message[4] = encoded.byteLength;
  message.set(encoded, 5);
  return message;
}

function clientRelayUrl(relayUrl: string): string {
  const url = new URL(relayUrl);
  if (url.protocol !== "ws:" && url.protocol !== "wss:") {
    throw new Error("remote relay URL must use WebSocket transport");
  }
  url.username = "";
  url.password = "";
  url.search = "";
  url.hash = "";
  url.pathname = "/client";
  return url.href;
}

function payload(message: Uint8Array, magic: Uint8Array): Uint8Array {
  if (message.byteLength <= magic.byteLength) throw new Error("invalid message");
  if (!equalBytes(message.subarray(0, magic.byteLength), magic)) {
    throw new Error("invalid message");
  }
  return message.subarray(magic.byteLength);
}

function framed(magic: Uint8Array, body: Uint8Array): Uint8Array<ArrayBuffer> {
  return concatenate(magic, body);
}

function concatenate(...values: readonly Uint8Array[]): Uint8Array<ArrayBuffer> {
  const length = values.reduce((total, value) => total + value.byteLength, 0);
  const output = new Uint8Array(length);
  let offset = 0;
  for (const value of values) {
    output.set(value, offset);
    offset += value.byteLength;
  }
  return output;
}

function binaryMessage(data: unknown): Uint8Array {
  if (!(data instanceof ArrayBuffer)) throw new Error("relay message is not binary");
  return new Uint8Array(data);
}

function exactBuffer(value: Uint8Array): ArrayBuffer {
  return value.slice().buffer;
}

function equalBytes(left: Uint8Array, right: Uint8Array): boolean {
  if (left.byteLength !== right.byteLength) return false;
  let difference = 0;
  for (let index = 0; index < left.byteLength; index += 1) {
    difference |= left[index]! ^ right[index]!;
  }
  return difference === 0;
}

function bytes(value: string): Uint8Array<ArrayBuffer> {
  return new TextEncoder().encode(value);
}

function encodeId(value: Uint8Array): string {
  let binary = "";
  for (const byte of value) binary += String.fromCharCode(byte);
  return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}

function decodeId(value: string, length: number, label: string): Uint8Array {
  if (!/^[A-Za-z0-9_-]+$/.test(value)) throw new Error(`invalid ${label}`);
  const standard = value.replaceAll("-", "+").replaceAll("_", "/");
  const padded = standard + "=".repeat((4 - (standard.length % 4)) % 4);
  let decoded: string;
  try {
    decoded = atob(padded);
  } catch {
    throw new Error(`invalid ${label}`);
  }
  const decodedBytes = Uint8Array.from(decoded, (character) => character.charCodeAt(0));
  if (decodedBytes.byteLength !== length) throw new Error(`invalid ${label}`);
  return decodedBytes;
}

function required<T>(value: T | undefined, label: string): T {
  if (value === undefined) throw new Error(`${label} is unavailable`);
  return value;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
