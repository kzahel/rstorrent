import { describe, expect, it } from "vitest";

import {
  RemoteApplicationWebSocket,
  type RemoteAuthorization,
  type RemoteConnectionFailure,
  type RemoteCryptoWasmModule,
  type WasmClientLogin,
  type WasmClientResume,
  type WasmClientResumeProof,
  type WasmClientSession,
  type WasmOpenedRecord,
} from "./remote-application-websocket";
import type { ApplicationWebSocket } from "./websocket-view-client";

const text = new TextEncoder();
const relayId = new Uint8Array(32).fill(1);
const hostId = new Uint8Array(32).fill(2);
const hostPin = new Uint8Array(64).fill(3);
const hostResumePublicKey = new Uint8Array(65).fill(4);
const clientId = new Uint8Array(16).fill(5);
const clientPublicKey = new Uint8Array(65).fill(6);

class FakeSocket implements ApplicationWebSocket {
  public readyState = 0;
  public binaryType: BinaryType = "blob";
  public onopen: ((event: Event) => void) | null = null;
  public onmessage: ((event: MessageEvent<unknown>) => void) | null = null;
  public onerror: ((event: Event) => void) | null = null;
  public onclose: ((event: CloseEvent) => void) | null = null;
  public readonly sent: Uint8Array[] = [];
  public readonly closes: Array<{ code: number; reason: string }> = [];

  public constructor(public readonly url: string) {}

  public open(): void {
    this.readyState = 1;
    this.onopen?.(new Event("open"));
  }

  public send(data: string | ArrayBuffer): void {
    if (typeof data === "string") throw new Error("relay received plaintext");
    this.sent.push(new Uint8Array(data).slice());
  }

  public server(data: Uint8Array): void {
    this.onmessage?.({ data: data.slice().buffer } as MessageEvent<ArrayBuffer>);
  }

  public close(code = 1_000, reason = ""): void {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.closes.push({ code, reason });
    this.onclose?.({ code, reason } as CloseEvent);
  }
}

class FakeSession implements WasmClientSession {
  private finalization: Uint8Array | undefined = Uint8Array.of(0x33);

  public take_finalization(): Uint8Array {
    const value = this.finalization;
    if (value === undefined) throw new Error("already consumed");
    this.finalization = undefined;
    return value;
  }

  public host_pin(): Uint8Array {
    return hostPin.slice();
  }

  public seal(plaintext: Uint8Array): Uint8Array {
    return concatenate(Uint8Array.of(0xa0), plaintext);
  }

  public seal_close(): Uint8Array {
    return Uint8Array.of(0xaf);
  }

  public open(record: Uint8Array): WasmOpenedRecord {
    if (record[0] !== 0xa1) throw new Error("invalid record");
    return { plaintext: record.slice(1), isClose: false };
  }
}

class FakeLogin implements WasmClientLogin {
  public request(): Uint8Array {
    return Uint8Array.of(0x11);
  }

  public finish(): WasmClientSession {
    return new FakeSession();
  }
}

class ChangedPinLogin extends FakeLogin {
  public override finish(): WasmClientSession {
    throw new Error("host identity changed");
  }
}

class FakeResumeProof implements WasmClientResumeProof {
  public signature_input(): Uint8Array {
    return Uint8Array.of(0x71);
  }

  public complete(): WasmClientSession {
    return new FakeSession();
  }
}

class FakeResume implements WasmClientResume {
  public request(): Uint8Array {
    return new Uint8Array(101).fill(0x61);
  }

  public finish(): WasmClientResumeProof {
    return new FakeResumeProof();
  }
}

const wasm: RemoteCryptoWasmModule = {
  ClientLogin: FakeLogin,
  ClientResume: FakeResume,
  authorizationTranscript: () => Uint8Array.of(0x72),
};

const subtle = {
  sign: async () => new Uint8Array(64).fill(0x73).buffer,
} as unknown as SubtleCrypto;

describe("product remote application WebSocket", () => {
  it("completes greeting, password, shared choice, and application records", async () => {
    const { socket, relay } = passwordSocket("shared");
    let opened = 0;
    socket.onopen = () => {
      opened += 1;
    };
    await passwordHandshake(relay, false);
    expect(opened).toBe(1);
    expect(relay.url).toBe("wss://127.0.0.1:7443/client");
    expect(asText(relay.sent[0])).toBe("RSC1\u0005alice");

    socket.send('{"type":"connect"}');
    expect(relay.sent.at(-1)?.[0]).toBe(0xa0);
    let received: unknown;
    socket.onmessage = (event) => {
      received = event.data;
    };
    relay.server(encrypted(text.encode('{"type":"connected"}')));
    await tick();
    expect(received).toBe('{"type":"connected"}');
    expect(() => socket.send(new ArrayBuffer(1))).toThrow("attachments");
    expect(() =>
      socket.send(
        JSON.stringify({ type: "call", operation: { type: "create_media_url" } }),
      ),
    ).toThrow("media");
  });

  it("creates a challenge-bound private authorization only after acknowledgement", async () => {
    let authorization: RemoteAuthorization | undefined;
    const { socket, relay } = passwordSocket("private", {
      onAuthorization: (value) => {
        authorization = value;
      },
    });
    await passwordHandshake(relay, true);
    expect(socket.readyState).toBe(1);
    expect(authorization?.clientId).toEqual(clientId);
    expect(authorization?.hostResumePublicKey).toEqual(hostResumePublicKey);
    expect(authorization?.authorizationGeneration).toBe(7n);
  });

  it("does not open before browser persistence completes", async () => {
    let releasePersistence: (() => void) | undefined;
    const persisted = new Promise<void>((resolve) => {
      releasePersistence = resolve;
    });
    const { socket, relay } = passwordSocket("private", {
      onAuthorization: async () => persisted,
    });
    await passwordHandshake(relay, true);
    expect(socket.readyState).toBe(0);
    releasePersistence?.();
    await tick();
    expect(socket.readyState).toBe(1);
  });

  it("resumes before password with fresh host and client proofs", async () => {
    let relay: FakeSocket | undefined;
    const socket = new RemoteApplicationWebSocket({
      relayUrl: "wss://127.0.0.1:7443/ignored",
      username: "alice",
      authentication: {
        type: "resume",
        authorization: authorization(),
        key: {} as CryptoKey,
      },
      crypto: wasm,
      subtle,
      entropy: () => new Uint8Array(32).fill(8),
      socketFactory: (url) => (relay = new FakeSocket(url)),
    });
    relay?.open();
    relay?.server(text.encode("RSP1"));
    relay?.server(greeting());
    await tick();
    expect(asText(relay?.sent[1]?.slice(0, 4))).toBe("RSR1");
    expect(relay?.sent[1]?.slice(4, 20)).toEqual(clientId);
    relay?.server(framed("RSR2", Uint8Array.of(0x62)));
    await tick();
    expect(asText(relay?.sent[2]?.slice(0, 4))).toBe("RSR3");
    relay?.server(encryptedJson("RSA4", outcome(true)));
    await tick();
    expect(socket.readyState).toBe(1);
  });

  it("blocks a changed greeting identity without password fallback", async () => {
    let relay: FakeSocket | undefined;
    let failure: RemoteConnectionFailure | undefined;
    const changed = authorization();
    changed.hostId[0] = 99;
    const socket = new RemoteApplicationWebSocket({
      relayUrl: "wss://127.0.0.1:7443/",
      username: "alice",
      authentication: {
        type: "resume",
        authorization: changed,
        key: {} as CryptoKey,
      },
      crypto: wasm,
      subtle,
      socketFactory: (url) => (relay = new FakeSocket(url)),
      onFailure: (value) => {
        failure = value;
      },
    });
    relay?.open();
    relay?.server(text.encode("RSP1"));
    relay?.server(greeting());
    await tick();
    expect(failure).toBe("host_identity_changed");
    expect(socket.readyState).toBe(3);
    expect(relay?.sent).toHaveLength(1);
  });

  it("classifies an authenticated OPAQUE pin mismatch as an identity change", async () => {
    let relay: FakeSocket | undefined;
    let failure: RemoteConnectionFailure | undefined;
    new RemoteApplicationWebSocket({
      relayUrl: "wss://127.0.0.1:7443/",
      username: "alice",
      authentication: {
        type: "password",
        passphrase: text.encode("correct horse battery staple"),
        expectedIdentity: { relayId, hostId, hostPin },
        choice: { type: "shared" },
      },
      crypto: { ...wasm, ClientLogin: ChangedPinLogin },
      subtle,
      socketFactory: (url) => (relay = new FakeSocket(url)),
      onFailure: (value) => {
        failure = value;
      },
    });
    relay?.open();
    relay?.server(text.encode("RSP1"));
    relay?.server(greeting());
    await tick();
    relay?.server(framed("RSL2", Uint8Array.of(0x12)));
    await tick();
    expect(failure).toBe("host_identity_changed");
  });

  it("classifies a post-greeting resume rejection without weakening identity", async () => {
    let relay: FakeSocket | undefined;
    let failure: RemoteConnectionFailure | undefined;
    new RemoteApplicationWebSocket({
      relayUrl: "wss://127.0.0.1:7443/",
      username: "alice",
      authentication: {
        type: "resume",
        authorization: authorization(),
        key: {} as CryptoKey,
      },
      crypto: wasm,
      subtle,
      socketFactory: (url) => (relay = new FakeSocket(url)),
      onFailure: (value) => {
        failure = value;
      },
    });
    relay?.open();
    relay?.server(text.encode("RSP1"));
    relay?.server(greeting());
    await tick();
    relay?.close(4_008, "unavailable");
    expect(failure).toBe("resume_rejected");
  });
});

function passwordSocket(
  choice: "shared" | "private",
  callbacks: {
    readonly onAuthorization?: (
      authorization: RemoteAuthorization,
    ) => void | Promise<void>;
  } = {},
): { socket: RemoteApplicationWebSocket; relay: FakeSocket } {
  let relay: FakeSocket | undefined;
  const socket = new RemoteApplicationWebSocket({
    relayUrl: "wss://127.0.0.1:7443/ignored?discarded=yes",
    username: "alice",
    authentication: {
      type: "password",
      passphrase: text.encode("correct horse battery staple"),
      choice:
        choice === "shared"
          ? { type: "shared", clientBuild: "test" }
          : {
              type: "private",
              credential: {
                key: {} as CryptoKey,
                clientId,
                clientPublicKey,
                label: "My browser",
                clientBuild: "test",
              },
            },
    },
    crypto: wasm,
    subtle,
    entropy: () => new Uint8Array(32).fill(8),
    socketFactory: (url) => (relay = new FakeSocket(url)),
    ...callbacks,
  });
  if (relay === undefined) throw new Error("fake relay missing");
  return { socket, relay };
}

async function passwordHandshake(relay: FakeSocket, privateChoice: boolean): Promise<void> {
  relay.open();
  relay.server(text.encode("RSP1"));
  relay.server(greeting());
  await tick();
  expect(asText(relay.sent[1]?.slice(0, 4))).toBe("RSL1");
  relay.server(framed("RSL2", Uint8Array.of(0x12)));
  await tick();
  expect(asText(relay.sent[2]?.slice(0, 4))).toBe("RSL3");
  relay.server(encryptedJson("RSA2", ready()));
  await tick();
  expect(relay.sent[3]?.[0]).toBe(0xa0);
  const choice = JSON.parse(asText(relay.sent[3]?.slice(5))) as { choice: string };
  expect(choice.choice).toBe(privateChoice ? "private" : "shared");
  relay.server(encryptedJson("RSA4", outcome(privateChoice)));
  await tick();
}

function authorization(): RemoteAuthorization {
  return {
    relayId: relayId.slice(),
    hostId: hostId.slice(),
    hostPin: hostPin.slice(),
    username: "alice",
    hostResumePublicKey: hostResumePublicKey.slice(),
    clientId: clientId.slice(),
    clientPublicKey: clientPublicKey.slice(),
    authorizationGeneration: 7n,
    clientGeneration: 1n,
    protocolFloor: 1,
    label: "My browser",
  };
}

function greeting(): Uint8Array {
  return concatenate(text.encode("RHG1"), relayId, hostId, Uint8Array.of(0, 1));
}

function ready(): object {
  return {
    protocol_version: 1,
    host_build: "test-host",
    host_pin: encodeId(hostPin),
    host_resume_public_key: encodeId(hostResumePublicKey),
    authorization_generation: 7,
    authorization_challenge: encodeId(new Uint8Array(32).fill(9)),
    protocol_floor: 1,
  };
}

function outcome(privateChoice: boolean): object {
  return {
    protocol_version: 1,
    authorization: privateChoice
      ? { client_id: encodeId(clientId), fingerprint: "SHA256:test" }
      : null,
  };
}

function encryptedJson(magic: string, value: object): Uint8Array {
  return encrypted(concatenate(text.encode(magic), text.encode(JSON.stringify(value))));
}

function encrypted(plaintext: Uint8Array): Uint8Array {
  return concatenate(Uint8Array.of(0xa1), plaintext);
}

function framed(magic: string, payload: Uint8Array): Uint8Array {
  return concatenate(text.encode(magic), payload);
}

function concatenate(...values: readonly Uint8Array[]): Uint8Array {
  const output = new Uint8Array(values.reduce((total, value) => total + value.length, 0));
  let offset = 0;
  for (const value of values) {
    output.set(value, offset);
    offset += value.length;
  }
  return output;
}

function encodeId(value: Uint8Array): string {
  let binary = "";
  for (const byte of value) binary += String.fromCharCode(byte);
  return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}

function asText(value: Uint8Array | undefined): string {
  return value === undefined ? "" : new TextDecoder().decode(value);
}

async function tick(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}
