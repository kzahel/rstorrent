// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  DirectFileRequest,
  DirectFileResponse,
  RemoteApplicationWebSocket,
} from "./remote-application-websocket";
import { prepareDirectFileSink, saveDirectFile } from "./direct-file-save";

describe("remote direct file save", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    delete (window as Window & { showSaveFilePicker?: unknown }).showSaveFilePicker;
  });

  it("streams an accepted range into the user-selected file", async () => {
    const written: Uint8Array[] = [];
    const close = vi.fn(async () => undefined);
    (window as Window & { showSaveFilePicker?: unknown }).showSaveFilePicker = vi.fn(() =>
      Promise.resolve({
        createWritable: async () => ({
          write: async (bytes: Uint8Array) => written.push(bytes.slice()),
          close,
          abort: async () => undefined,
        }),
      }),
    );
    vi.stubGlobal("RTCPeerConnection", MockPeerConnection);
    const requests: DirectFileRequest[] = [];
    const transport = {
      directFileSupported: () => true,
      directFileConnectionGeneration: () => 7,
      directFile: async (request: DirectFileRequest): Promise<DirectFileResponse> => {
        requests.push(request);
        if (request.type === "open") {
          return {
            type: "opened",
            request_id: request.request_id,
            circuit_generation: request.circuit_generation,
            browser_peer_generation: request.browser_peer_generation,
            host_peer_generation: 1,
            file_length: "5",
            max_chunk_bytes: 16 * 1024,
            answer: { type: "answer", sdp: "v=0\r\n" },
            candidates: [],
          };
        }
        if (request.type === "end_of_candidates") {
          return { ...request, type: "end_of_candidates", host_peer_generation: 1 };
        }
        if (request.type === "close") {
          return { ...request, type: "closed", host_peer_generation: 1 };
        }
        throw new Error("unexpected candidate");
      },
    } as unknown as RemoteApplicationWebSocket;

    const sink = prepareDirectFileSink("hello.txt", 5n);
    await saveDirectFile(
      transport,
      {
        torrentId: "torrent",
        fileIndex: 0,
        fileName: "hello.txt",
        expectedLength: 5n,
      },
      sink,
    );

    expect(new TextDecoder().decode(written[0])).toBe("hello");
    expect(close).toHaveBeenCalledOnce();
    expect(requests.map((request) => request.type)).toEqual([
      "open",
      "end_of_candidates",
      "close",
    ]);
    expect((requests.at(-1) as Extract<DirectFileRequest, { type: "close" }>).outcome).toBe(
      "complete",
    );
  });

  it("rejects a large fallback before opening a peer", async () => {
    await expect(prepareDirectFileSink("large.bin", 32n * 1024n * 1024n + 1n)).rejects.toThrow(
      "32 MiB",
    );
  });
});

class MockDataChannel extends EventTarget {
  public binaryType = "arraybuffer";
  public readyState: RTCDataChannelState = "connecting";
  public onmessage: ((event: MessageEvent) => void) | null = null;
  public onerror: (() => void) | null = null;
  public onclose: (() => void) | null = null;
  private requestId = 0;

  public send(buffer: ArrayBuffer): void {
    const view = new DataView(buffer);
    if (view.getUint8(1) === 0x01) {
      this.requestId = view.getUint32(2);
      const offset = view.getBigUint64(6);
      const length = view.getUint32(14);
      queueMicrotask(() => this.deliver(rangeAccepted(this.requestId, 5n, offset, length)));
      queueMicrotask(() => this.deliver(rangeChunk(this.requestId, offset, new TextEncoder().encode("hello"))));
    } else if (view.getUint8(1) === 0x03) {
      queueMicrotask(() => this.deliver(rangeComplete(this.requestId)));
    }
  }

  public open(): void {
    this.readyState = "open";
    this.dispatchEvent(new Event("open"));
  }

  public close(): void {
    this.readyState = "closed";
  }

  private deliver(data: ArrayBuffer): void {
    this.onmessage?.(new MessageEvent("message", { data }));
  }
}

class MockPeerConnection extends EventTarget {
  public iceGatheringState: RTCIceGatheringState = "complete";
  public iceConnectionState: RTCIceConnectionState = "connected";
  public localDescription: RTCSessionDescription | null = null;
  private readonly channel = new MockDataChannel();

  public createDataChannel(): RTCDataChannel {
    return this.channel as unknown as RTCDataChannel;
  }

  public async createOffer(): Promise<RTCSessionDescriptionInit> {
    return { type: "offer", sdp: "v=0\r\n" };
  }

  public async setLocalDescription(description: RTCLocalSessionDescriptionInit): Promise<void> {
    this.localDescription = description as RTCSessionDescription;
  }

  public async setRemoteDescription(): Promise<void> {
    queueMicrotask(() => this.channel.open());
  }

  public async addIceCandidate(): Promise<void> {}

  public close(): void {}
}

function rangeAccepted(requestId: number, fileLength: bigint, offset: bigint, length: number): ArrayBuffer {
  const buffer = new ArrayBuffer(26);
  const view = new DataView(buffer);
  view.setUint8(0, 1);
  view.setUint8(1, 0x81);
  view.setUint32(2, requestId);
  view.setBigUint64(6, fileLength);
  view.setBigUint64(14, offset);
  view.setUint32(22, length);
  return buffer;
}

function rangeChunk(requestId: number, offset: bigint, payload: Uint8Array): ArrayBuffer {
  const bytes = new Uint8Array(14 + payload.byteLength);
  const view = new DataView(bytes.buffer);
  view.setUint8(0, 1);
  view.setUint8(1, 0x82);
  view.setUint32(2, requestId);
  view.setBigUint64(6, offset);
  bytes.set(payload, 14);
  return bytes.buffer;
}

function rangeComplete(requestId: number): ArrayBuffer {
  const buffer = new ArrayBuffer(6);
  const view = new DataView(buffer);
  view.setUint8(0, 1);
  view.setUint8(1, 0x83);
  view.setUint32(2, requestId);
  return buffer;
}
