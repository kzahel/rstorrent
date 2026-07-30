import type {
  GatewayClientMessage,
  GatewayServerMessage,
  RequestEnvelope,
  ResponseEnvelope,
  SubscriptionSpec,
  ViewUpdate,
} from "./generated/contract";
import type {
  ApplicationClient,
  ApplicationSubscription,
} from "./application-client";
import {
  ContractError,
  decodeGatewayServerMessage,
} from "./validation";

const MAX_BUFFERED_SOCKET_BYTES = 4 * 1024 * 1024;
const CONNECT_TIMEOUT_MILLIS = 5_000;

interface Pending<T> {
  resolve(value: T): void;
  reject(error: Error): void;
}

export class WebSocketApplicationClient implements ApplicationClient {
  private readonly pendingDispatch = new Map<string, Pending<ResponseEnvelope>>();
  private readonly pendingSubscribe = new Map<
    string,
    Pending<WebSocketSubscription> & { spec: SubscriptionSpec }
  >();
  private readonly subscriptions = new Map<string, WebSocketSubscription>();
  private authentication: Pending<void> | undefined;
  private closed = false;
  private nextRequest = 1;

  private constructor(
    private readonly socket: WebSocket,
    private readonly token: string,
  ) {
    socket.addEventListener("message", (event) => {
      this.receive(event.data);
    });
    socket.addEventListener("close", () => {
      this.failAll(new Error("gateway connection closed"));
    });
    socket.addEventListener("error", () => {
      this.failAll(new Error("gateway connection failed"));
    });
  }

  public static async connect(
    url: string,
    token: string,
  ): Promise<WebSocketApplicationClient> {
    if (token.length === 0 || token.length > 128) {
      throw new Error("gateway token must be 1..=128 characters");
    }
    const socket = new WebSocket(url);
    const client = new WebSocketApplicationClient(socket, token);
    await client.authenticate();
    return client;
  }

  public dispatch(request: RequestEnvelope): Promise<ResponseEnvelope> {
    if (this.pendingDispatch.has(request.request_id)) {
      return Promise.reject(new Error("request ID is already pending"));
    }
    const result = new Promise<ResponseEnvelope>((resolve, reject) => {
      this.pendingDispatch.set(request.request_id, { resolve, reject });
    });
    try {
      this.send({ type: "dispatch", request });
    } catch (error) {
      this.pendingDispatch.delete(request.request_id);
      throw error;
    }
    return result;
  }

  public subscribe(
    spec: SubscriptionSpec,
  ): Promise<ApplicationSubscription> {
    const requestId = this.requestId("subscribe");
    const result = new Promise<WebSocketSubscription>((resolve, reject) => {
      this.pendingSubscribe.set(requestId, { resolve, reject, spec });
    });
    try {
      this.send({ type: "subscribe", request_id: requestId, spec });
    } catch (error) {
      this.pendingSubscribe.delete(requestId);
      throw error;
    }
    return result;
  }

  public async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    for (const subscription of this.subscriptions.values()) {
      subscription.closeLocal();
    }
    this.subscriptions.clear();
    this.socket.close(1000, "client closed");
    this.failAll(new Error("gateway client closed"));
  }

  private async authenticate(): Promise<void> {
    await new Promise<void>((resolve, reject) => {
      const timer = window.setTimeout(() => {
        this.authentication = undefined;
        reject(new Error("gateway authentication timed out"));
        this.socket.close();
      }, CONNECT_TIMEOUT_MILLIS);
      this.authentication = {
        resolve: () => {
          window.clearTimeout(timer);
          resolve();
        },
        reject: (error) => {
          window.clearTimeout(timer);
          reject(error);
        },
      };
      const sendAuthentication = (): void => {
        try {
          this.send({
            type: "authenticate",
            contract_version: 1,
            token: this.token,
          });
        } catch (error) {
          this.authentication = undefined;
          window.clearTimeout(timer);
          reject(asError(error));
        }
      };
      if (this.socket.readyState === WebSocket.OPEN) {
        sendAuthentication();
      } else {
        this.socket.addEventListener("open", sendAuthentication, { once: true });
      }
    });
  }

  private receive(data: unknown): void {
    if (typeof data !== "string") {
      this.failAll(new ContractError("gateway sent a non-text frame"));
      this.socket.close(1002, "invalid frame");
      return;
    }
    let message: GatewayServerMessage;
    try {
      message = decodeGatewayServerMessage(data);
    } catch (error) {
      this.failAll(asError(error));
      this.socket.close(1002, "invalid contract");
      return;
    }
    switch (message.type) {
      case "authenticated":
        if (message.contract_version !== 1) {
          this.failAll(new ContractError("unsupported gateway version"));
          this.socket.close(1002, "unsupported version");
          return;
        }
        this.authentication?.resolve();
        this.authentication = undefined;
        break;
      case "response": {
        const pending = this.pendingDispatch.get(message.response.request_id);
        if (pending !== undefined) {
          this.pendingDispatch.delete(message.response.request_id);
          pending.resolve(message.response);
        }
        break;
      }
      case "subscribed": {
        const pending = this.pendingSubscribe.get(message.request_id);
        if (pending === undefined) break;
        this.pendingSubscribe.delete(message.request_id);
        const subscription = new WebSocketSubscription(
          message.stream_id,
          pending.spec.delivery.max_queue_bytes,
          (outgoing) => this.send(outgoing),
          () => this.subscriptions.delete(message.stream_id),
          () => this.requestId("subscription"),
        );
        this.subscriptions.set(message.stream_id, subscription);
        pending.resolve(subscription);
        break;
      }
      case "update":
        this.subscriptions.get(message.update.stream_id)?.push(message.update);
        break;
      case "unsubscribed":
        this.subscriptions.get(message.stream_id)?.closeLocal();
        this.subscriptions.delete(message.stream_id);
        break;
      case "error": {
        const error = new Error(`${message.code}: ${message.message}`);
        if (message.request_id !== undefined && message.request_id !== null) {
          const dispatch = this.pendingDispatch.get(message.request_id);
          if (dispatch !== undefined) {
            this.pendingDispatch.delete(message.request_id);
            dispatch.reject(error);
          }
          const subscribe = this.pendingSubscribe.get(message.request_id);
          if (subscribe !== undefined) {
            this.pendingSubscribe.delete(message.request_id);
            subscribe.reject(error);
          }
        } else {
          this.authentication?.reject(error);
          this.authentication = undefined;
        }
        break;
      }
    }
  }

  private send(message: GatewayClientMessage): void {
    if (this.closed) throw new Error("gateway client is closed");
    if (this.socket.readyState !== WebSocket.OPEN) {
      throw new Error("gateway socket is not open");
    }
    if (this.socket.bufferedAmount > MAX_BUFFERED_SOCKET_BYTES) {
      throw new Error("gateway socket write queue exceeded its bound");
    }
    this.socket.send(JSON.stringify(message));
  }

  private requestId(prefix: string): string {
    const result = `${prefix}-${this.nextRequest}`;
    this.nextRequest += 1;
    return result;
  }

  private failAll(error: Error): void {
    this.authentication?.reject(error);
    this.authentication = undefined;
    for (const pending of this.pendingDispatch.values()) pending.reject(error);
    for (const pending of this.pendingSubscribe.values()) pending.reject(error);
    this.pendingDispatch.clear();
    this.pendingSubscribe.clear();
    for (const subscription of this.subscriptions.values()) {
      subscription.fail(error);
    }
    this.subscriptions.clear();
  }
}

class WebSocketSubscription implements ApplicationSubscription {
  private readonly queue: QueuedUpdates;
  private closed = false;

  public constructor(
    public readonly streamId: string,
    maxQueueBytes: number,
    private readonly send: (message: GatewayClientMessage) => void,
    private readonly onClose: () => void,
    private readonly requestId: () => string,
  ) {
    this.queue = new QueuedUpdates(maxQueueBytes);
  }

  public [Symbol.asyncIterator](): AsyncIterator<ViewUpdate> {
    return this.queue;
  }

  public push(update: ViewUpdate): void {
    this.queue.push(update);
  }

  public async resync(): Promise<void> {
    if (this.closed) throw new Error("subscription is closed");
    this.send({
      type: "resync",
      request_id: this.requestId(),
      stream_id: this.streamId,
    });
  }

  public async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.send({
      type: "unsubscribe",
      request_id: this.requestId(),
      stream_id: this.streamId,
    });
    this.closeLocal();
    this.onClose();
  }

  public closeLocal(): void {
    this.closed = true;
    this.queue.close();
  }

  public fail(error: Error): void {
    this.closed = true;
    this.queue.fail(error);
  }
}

class QueuedUpdates implements AsyncIterator<ViewUpdate> {
  private readonly updates: Array<{ update: ViewUpdate; bytes: number }> = [];
  private readonly waiting: Array<Pending<IteratorResult<ViewUpdate>>> = [];
  private queuedBytes = 0;
  private terminalError?: Error;
  private closed = false;

  public constructor(private readonly maxBytes: number) {}

  public next(): Promise<IteratorResult<ViewUpdate>> {
    const queued = this.updates.shift();
    if (queued !== undefined) {
      this.queuedBytes -= queued.bytes;
      return Promise.resolve({ done: false, value: queued.update });
    }
    if (this.terminalError !== undefined) {
      return Promise.reject(this.terminalError);
    }
    if (this.closed) return Promise.resolve({ done: true, value: undefined });
    return new Promise((resolve, reject) => {
      this.waiting.push({ resolve, reject });
    });
  }

  public push(update: ViewUpdate): void {
    if (this.closed) return;
    const waiting = this.waiting.shift();
    if (waiting !== undefined) {
      waiting.resolve({ done: false, value: update });
      return;
    }
    const bytes = new TextEncoder().encode(JSON.stringify(update)).byteLength;
    if (bytes > this.maxBytes || this.queuedBytes + bytes > this.maxBytes) {
      this.updates.length = 0;
      this.queuedBytes = 0;
      const reset: ViewUpdate = {
        contract_version: update.contract_version,
        stream_id: update.stream_id,
        epoch: update.epoch,
        sequence: update.sequence,
        base_revision: update.base_revision,
        revision: update.revision,
        type: "reset_required",
        reason: "queue_overflow",
      };
      const resetBytes = new TextEncoder().encode(
        JSON.stringify(reset),
      ).byteLength;
      this.updates.push({ update: reset, bytes: resetBytes });
      this.queuedBytes = resetBytes;
      return;
    }
    this.updates.push({ update, bytes });
    this.queuedBytes += bytes;
  }

  public close(): void {
    this.closed = true;
    for (const waiting of this.waiting.splice(0)) {
      waiting.resolve({ done: true, value: undefined });
    }
  }

  public fail(error: Error): void {
    this.terminalError = error;
    this.closed = true;
    for (const waiting of this.waiting.splice(0)) waiting.reject(error);
  }
}

function asError(value: unknown): Error {
  return value instanceof Error ? value : new Error(String(value));
}
