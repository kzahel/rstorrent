import { describe, expect, it } from "vitest";

import {
  RemoteApplicationWebSocket,
  provisionRemotePassword,
  type RemoteConnectionFailure,
  type RemoteCryptoWasmModule,
  type WasmClientLogin,
  type WasmClientRegistration,
  type WasmClientSession,
  type WasmOpenedRecord,
} from "./remote-application-websocket";
import type { ApplicationWebSocket } from "./websocket-view-client";

const text = new TextEncoder();

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
    if (value === undefined) throw new Error("already been consumed");
    this.finalization = undefined;
    return value;
  }

  public host_pin(): Uint8Array {
    return new Uint8Array(64).fill(0x44);
  }

  public seal(plaintext: Uint8Array): Uint8Array {
    return framed(Uint8Array.of(0xa0), plaintext);
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

  public finish(
    _passphrase: Uint8Array,
    _relayId: Uint8Array,
    _username: string,
    _hostId: Uint8Array,
    expectedPin: Uint8Array,
  ): WasmClientSession {
    if (expectedPin[0] === 0xff) throw new Error("host identity changed");
    return new FakeSession();
  }
}

class FakeRegistration implements WasmClientRegistration {
  public request(): Uint8Array {
    return Uint8Array.of(0x21);
  }

  public finish(): Uint8Array {
    return Uint8Array.of(0x22);
  }
}

const crypto: RemoteCryptoWasmModule = {
  ClientLogin: FakeLogin,
  ClientRegistration: FakeRegistration,
};

describe("remote application WebSocket", () => {
  it("opens only after OPAQUE and authenticated readiness, then carries text records", () => {
    let relay: FakeSocket | undefined;
    let opened = 0;
    let pin: Uint8Array | undefined;
    const socket = new RemoteApplicationWebSocket({
      relayUrl: "ws://127.0.0.1:43000/ignored?secret=never",
      relayId: new Uint8Array(32).fill(1),
      username: "alice",
      passphrase: text.encode("correct horse battery staple"),
      hostId: new Uint8Array(32).fill(2),
      crypto,
      entropy: () => new Uint8Array(32).fill(3),
      socketFactory: (url) => (relay = new FakeSocket(url)),
      onHostPin: (value) => {
        pin = value;
      },
    });
    socket.onopen = () => {
      opened += 1;
    };
    relay?.open();
    expect(relay?.url).toBe("ws://127.0.0.1:43000/client");
    expect(asText(relay?.sent[0])).toBe("RSC1\u0005alice");

    relay?.server(text.encode("RSP1"));
    expect(asText(relay?.sent[1]?.slice(0, 4))).toBe("RSL1");
    relay?.server(framed(text.encode("RSL2"), Uint8Array.of(0x12)));
    expect(asText(relay?.sent[2]?.slice(0, 4))).toBe("RSL3");
    expect(opened).toBe(0);

    relay?.server(framed(Uint8Array.of(0xa1), text.encode("RSA1")));
    expect(opened).toBe(1);
    expect(socket.readyState).toBe(1);
    expect(pin).toEqual(new Uint8Array(64).fill(0x44));

    socket.send('{"type":"connect"}');
    expect(relay?.sent.at(-1)?.[0]).toBe(0xa0);
    let received: unknown;
    socket.onmessage = (event) => {
      received = event.data;
    };
    relay?.server(framed(Uint8Array.of(0xa1), text.encode('{"type":"connected"}')));
    expect(received).toBe('{"type":"connected"}');

    socket.close(1_000, "done");
    expect(relay?.sent.at(-1)).toEqual(Uint8Array.of(0xaf));
  });

  it("rejects remote attachment and media breadth before encryption", () => {
    const { socket } = openSocket();
    expect(() => socket.send(new ArrayBuffer(1))).toThrow("attachments");
    expect(() =>
      socket.send(
        JSON.stringify({
          type: "call",
          operation: { type: "create_media_url" },
        }),
      ),
    ).toThrow("media");
    expect(() =>
      socket.send(JSON.stringify({ type: "begin_torrent_upload" })),
    ).toThrow("attachments");
    socket.close();
  });

  it("reports a pinned host mismatch distinctly and otherwise fails generically", () => {
    let relay: FakeSocket | undefined;
    let failure: RemoteConnectionFailure | undefined;
    const socket = new RemoteApplicationWebSocket({
      ...options(),
      expectedHostPin: new Uint8Array(64).fill(0xff),
      socketFactory: (url) => (relay = new FakeSocket(url)),
      onFailure: (value) => {
        failure = value;
      },
    });
    relay?.open();
    relay?.server(text.encode("RSP1"));
    relay?.server(framed(text.encode("RSL2"), Uint8Array.of(0x12)));
    expect(failure).toBe("host_identity_changed");
    expect(socket.readyState).toBe(3);
    expect(relay?.closes[0]?.code).toBe(4_008);
  });

  it("reports a relay close during authentication as a generic failure", () => {
    let relay: FakeSocket | undefined;
    let failure: RemoteConnectionFailure | undefined;
    const socket = new RemoteApplicationWebSocket({
      ...options(),
      socketFactory: (url) => (relay = new FakeSocket(url)),
      onFailure: (value) => {
        failure = value;
      },
    });
    relay?.open();
    relay?.close(4_004, "unavailable");
    expect(failure).toBe("connection_failed");
    expect(socket.readyState).toBe(3);
  });
});

describe("remote password provisioning", () => {
  it("installs only after the explicit host acknowledgement", async () => {
    let relay: FakeSocket | undefined;
    const provisioning = provisionRemotePassword({
      ...options(),
      socketFactory: (url) => (relay = new FakeSocket(url)),
    });
    relay?.open();
    relay?.server(text.encode("RSP1"));
    expect(asText(relay?.sent[1]?.slice(0, 4))).toBe("RSG1");
    relay?.server(framed(text.encode("RSG2"), Uint8Array.of(0x24)));
    expect(asText(relay?.sent[2]?.slice(0, 4))).toBe("RSG3");
    relay?.server(text.encode("RSG4"));
    await expect(provisioning).resolves.toBeUndefined();
    expect(relay?.closes[0]?.code).toBe(1_000);
  });
});

function openSocket(): { socket: RemoteApplicationWebSocket; relay: FakeSocket } {
  let relay: FakeSocket | undefined;
  const socket = new RemoteApplicationWebSocket({
    ...options(),
    socketFactory: (url) => (relay = new FakeSocket(url)),
  });
  relay?.open();
  relay?.server(text.encode("RSP1"));
  relay?.server(framed(text.encode("RSL2"), Uint8Array.of(0x12)));
  relay?.server(framed(Uint8Array.of(0xa1), text.encode("RSA1")));
  if (relay === undefined) throw new Error("fake relay was not created");
  return { socket, relay };
}

function options() {
  return {
    relayUrl: "ws://127.0.0.1:43000",
    relayId: new Uint8Array(32).fill(1),
    username: "alice",
    passphrase: text.encode("correct horse battery staple"),
    hostId: new Uint8Array(32).fill(2),
    crypto,
    entropy: () => new Uint8Array(32).fill(3),
  };
}

function framed(prefix: Uint8Array, body: Uint8Array): Uint8Array {
  const value = new Uint8Array(prefix.byteLength + body.byteLength);
  value.set(prefix);
  value.set(body, prefix.byteLength);
  return value;
}

function asText(value: Uint8Array | undefined): string | undefined {
  return value === undefined ? undefined : new TextDecoder().decode(value);
}
