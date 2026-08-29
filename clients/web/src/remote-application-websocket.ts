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
const REGISTRATION_REQUEST = bytes("RSG1");
const REGISTRATION_RESPONSE = bytes("RSG2");
const REGISTRATION_UPLOAD = bytes("RSG3");
const REGISTRATION_COMPLETE = bytes("RSG4");
const LOGIN_REQUEST = bytes("RSL1");
const LOGIN_RESPONSE = bytes("RSL2");
const LOGIN_FINALIZATION = bytes("RSL3");
const AUTHENTICATED_READY = bytes("RSA1");

export type RemoteConnectionFailure =
  | "connection_failed"
  | "host_identity_changed";

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

export interface WasmClientRegistration {
  request(): Uint8Array;
  finish(
    passphrase: Uint8Array,
    relayId: Uint8Array,
    username: string,
    hostId: Uint8Array,
    response: Uint8Array,
    entropy: Uint8Array,
  ): Uint8Array;
}

export interface RemoteCryptoWasmModule {
  readonly ClientLogin: new (
    passphrase: Uint8Array,
    entropy: Uint8Array,
  ) => WasmClientLogin;
  readonly ClientRegistration: new (
    passphrase: Uint8Array,
    entropy: Uint8Array,
  ) => WasmClientRegistration;
}

export interface RemoteConnectionOptions {
  readonly relayUrl: string;
  readonly relayId: Uint8Array;
  readonly username: string;
  readonly passphrase: Uint8Array;
  readonly hostId: Uint8Array;
  readonly expectedHostPin?: Uint8Array;
  readonly crypto: RemoteCryptoWasmModule;
  readonly socketFactory?: ApplicationWebSocketFactory;
  readonly entropy?: () => Uint8Array;
  readonly onHostPin?: (pin: Uint8Array) => void;
  readonly onFailure?: (failure: RemoteConnectionFailure) => void;
}

/**
 * ApplicationWebSocket-compatible OPAQUE and record transport.
 *
 * The existing application client sees a text-only WebSocket. This adapter
 * does not report it open until the relay pair, OPAQUE login and an
 * authenticated host-ready record all succeed.
 */
export class RemoteApplicationWebSocket implements ApplicationWebSocket {
  public binaryType: BinaryType = "arraybuffer";
  public onopen: ((event: Event) => void) | null = null;
  public onmessage: ((event: MessageEvent<unknown>) => void) | null = null;
  public onerror: ((event: Event) => void) | null = null;
  public onclose: ((event: CloseEvent) => void) | null = null;

  private state = SOCKET_CONNECTING;
  private phase: "pair" | "login" | "ready" = "pair";
  private readonly socket: ApplicationWebSocket;
  private readonly passphrase: Uint8Array;
  private readonly expectedHostPin: Uint8Array;
  private login: WasmClientLogin | undefined;
  private session: WasmClientSession | undefined;
  private candidateHostPin: Uint8Array | undefined;
  private readonly timer: ReturnType<typeof setTimeout>;
  private failed = false;

  public constructor(private readonly options: RemoteConnectionOptions) {
    validateOptions(options);
    this.passphrase = options.passphrase.slice();
    this.expectedHostPin = options.expectedHostPin?.slice() ?? new Uint8Array();
    const factory = options.socketFactory ?? ((url) => new WebSocket(url));
    this.socket = factory(clientRelayUrl(options.relayUrl));
    this.socket.binaryType = "arraybuffer";
    this.socket.onopen = () => {
      try {
        this.socket.send(encodeClientSelect(options.username).buffer);
      } catch (error) {
        this.fail(error);
      }
    };
    this.socket.onmessage = (event) => this.receive(event.data);
    this.socket.onerror = () => this.fail(new Error("relay transport failed"));
    this.socket.onclose = (event) => this.closed(event);
    this.timer = setTimeout(
      () => this.fail(new Error("remote login timed out")),
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
    const record = this.session.seal(new TextEncoder().encode(data));
    this.socket.send(exactBuffer(record));
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
        // The channel may already be terminal after hostile input.
      }
    }
    this.socket.close(code, reason);
  }

  private receive(data: unknown): void {
    try {
      const message = binaryMessage(data);
      if (this.phase === "pair") {
        if (!equalBytes(message, PAIRED_CONTROL)) throw new Error("pair failed");
        this.login = new this.options.crypto.ClientLogin(
          this.passphrase,
          this.operationEntropy(),
        );
        this.socket.send(
          exactBuffer(framed(LOGIN_REQUEST, this.login.request())),
        );
        this.phase = "login";
        return;
      }
      if (this.phase === "login") {
        const response = payload(message, LOGIN_RESPONSE);
        const login = this.login;
        if (login === undefined) throw new Error("login state is unavailable");
        const session = login.finish(
          this.passphrase,
          this.options.relayId,
          this.options.username,
          this.options.hostId,
          this.expectedHostPin,
          response,
          this.operationEntropy(),
        );
        this.login = undefined;
        this.session = session;
        this.candidateHostPin = session.host_pin().slice();
        this.socket.send(
          exactBuffer(framed(LOGIN_FINALIZATION, session.take_finalization())),
        );
        this.phase = "ready";
        return;
      }
      const session = this.session;
      if (session === undefined) throw new Error("record state is unavailable");
      const opened = session.open(message);
      if (opened.isClose) {
        this.close(1_000, "authenticated remote close");
        return;
      }
      if (this.state === SOCKET_CONNECTING) {
        if (!equalBytes(opened.plaintext, AUTHENTICATED_READY)) {
          throw new Error("authenticated host readiness is invalid");
        }
        clearTimeout(this.timer);
        this.eraseHandshakeSecrets();
        this.state = SOCKET_OPEN;
        if (this.candidateHostPin !== undefined) {
          this.options.onHostPin?.(this.candidateHostPin.slice());
          this.candidateHostPin.fill(0);
          this.candidateHostPin = undefined;
        }
        this.onopen?.(new Event("open"));
        return;
      }
      const text = new TextDecoder("utf-8", { fatal: true }).decode(
        opened.plaintext,
      );
      this.onmessage?.(new MessageEvent("message", { data: text }));
    } catch (error) {
      this.fail(error);
    }
  }

  private fail(error: unknown): void {
    if (this.failed || this.state === SOCKET_CLOSED) return;
    this.failed = true;
    const failure: RemoteConnectionFailure = String(error).includes(
      "host identity changed",
    )
      ? "host_identity_changed"
      : "connection_failed";
    this.options.onFailure?.(failure);
    this.onerror?.(new Event("error"));
    this.close(REMOTE_FAILURE_CLOSE_CODE, "remote connection failed");
  }

  private closed(event: CloseEvent): void {
    clearTimeout(this.timer);
    this.eraseHandshakeSecrets();
    this.candidateHostPin?.fill(0);
    this.candidateHostPin = undefined;
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
  }
}

/** Perform the explicit one-circuit registration ceremony used by the proof. */
export function provisionRemotePassword(
  options: RemoteConnectionOptions,
): Promise<void> {
  validateOptions(options);
  const passphrase = options.passphrase.slice();
  const factory = options.socketFactory ?? ((url) => new WebSocket(url));
  const socket = factory(clientRelayUrl(options.relayUrl));
  socket.binaryType = "arraybuffer";
  return new Promise((resolve, reject) => {
    let phase: "pair" | "response" | "complete" = "pair";
    let registration: WasmClientRegistration | undefined;
    let settled = false;
    const entropy = () => {
      const value = options.entropy?.() ?? remoteCryptoOperationEntropy();
      if (value.byteLength !== 32) throw new Error("secure randomness failed");
      return value;
    };
    const finish = (error?: unknown) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      passphrase.fill(0);
      registration = undefined;
      socket.close(error === undefined ? 1_000 : REMOTE_FAILURE_CLOSE_CODE);
      if (error === undefined) resolve();
      else reject(new Error("remote provisioning failed"));
    };
    const timer = setTimeout(
      () => finish(new Error("provisioning timed out")),
      HANDSHAKE_TIMEOUT_MILLIS,
    );
    socket.onopen = () => {
      try {
        socket.send(encodeClientSelect(options.username).buffer);
      } catch (error) {
        finish(error);
      }
    };
    socket.onmessage = (event) => {
      try {
        const message = binaryMessage(event.data);
        if (phase === "pair") {
          if (!equalBytes(message, PAIRED_CONTROL)) throw new Error("pair failed");
          registration = new options.crypto.ClientRegistration(
            passphrase,
            entropy(),
          );
          socket.send(
            exactBuffer(framed(REGISTRATION_REQUEST, registration.request())),
          );
          phase = "response";
        } else if (phase === "response") {
          const active = registration;
          if (active === undefined) throw new Error("registration unavailable");
          const upload = active.finish(
            passphrase,
            options.relayId,
            options.username,
            options.hostId,
            payload(message, REGISTRATION_RESPONSE),
            entropy(),
          );
          registration = undefined;
          socket.send(exactBuffer(framed(REGISTRATION_UPLOAD, upload)));
          phase = "complete";
        } else {
          if (!equalBytes(message, REGISTRATION_COMPLETE)) {
            throw new Error("registration acknowledgement is invalid");
          }
          finish();
        }
      } catch (error) {
        finish(error);
      }
    };
    socket.onerror = () => finish(new Error("relay transport failed"));
    socket.onclose = () => finish(new Error("relay transport closed"));
  });
}

function rejectUnsupportedApplicationBreadth(encoded: string): void {
  let frame: unknown;
  try {
    frame = JSON.parse(encoded);
  } catch {
    throw new Error("remote application frame is invalid JSON");
  }
  if (typeof frame !== "object" || frame === null) return;
  const candidate = frame as {
    type?: unknown;
    operation?: { type?: unknown };
  };
  if (candidate.type === "begin_torrent_upload") {
    throw new Error("remote torrent byte attachments are unsupported");
  }
  if (
    candidate.type === "call" &&
    candidate.operation?.type === "create_media_url"
  ) {
    throw new Error("remote media capabilities are unsupported");
  }
}

function validateOptions(options: RemoteConnectionOptions): void {
  if (options.relayId.byteLength !== 32 || options.hostId.byteLength !== 32) {
    throw new Error("remote relay and host IDs must contain 32 bytes");
  }
  if (
    options.expectedHostPin !== undefined &&
    options.expectedHostPin.byteLength !== 64
  ) {
    throw new Error("remote host pin must contain 64 bytes");
  }
  encodeClientSelect(options.username);
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
  if (message.byteLength < magic.byteLength) throw new Error("invalid message");
  if (!equalBytes(message.subarray(0, magic.byteLength), magic)) {
    throw new Error("invalid message");
  }
  return message.subarray(magic.byteLength);
}

function framed(magic: Uint8Array, body: Uint8Array): Uint8Array<ArrayBuffer> {
  const message = new Uint8Array(magic.byteLength + body.byteLength);
  message.set(magic);
  message.set(body, magic.byteLength);
  return message;
}

function binaryMessage(data: unknown): Uint8Array {
  if (!(data instanceof ArrayBuffer)) throw new Error("relay message is not binary");
  return new Uint8Array(data);
}

function exactBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.slice().buffer;
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
