import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  RequestEnvelope,
  ResponseEnvelope,
  SubscriptionSpec,
  ViewUpdate,
} from "./api";
import type {
  ApplicationClient,
  ApplicationSubscription,
} from "./application-client";

export class TauriApplicationClient implements ApplicationClient {
  private readonly subscriptions = new Set<TauriSubscription>();

  public dispatch(request: RequestEnvelope): Promise<ResponseEnvelope> {
    return invoke<ResponseEnvelope>("application_dispatch", { request });
  }

  public async subscribe(
    spec: SubscriptionSpec,
  ): Promise<ApplicationSubscription> {
    const channel = new Channel<ViewUpdate>();
    const subscription = new TauriSubscription(
      channel,
      spec.delivery.max_queue_bytes,
      () => {
        this.subscriptions.delete(subscription);
      },
    );
    channel.onmessage = (update) => subscription.push(update);
    const streamId = await invoke<string>("application_subscribe", {
      spec,
      updates: channel,
    });
    subscription.attach(streamId);
    this.subscriptions.add(subscription);
    return subscription;
  }

  public async close(): Promise<void> {
    await Promise.all(
      [...this.subscriptions].map(async (subscription) =>
        subscription.close(),
      ),
    );
    this.subscriptions.clear();
  }
}

export class TauriSubscription implements ApplicationSubscription {
  private readonly queue: Array<{ update: ViewUpdate; bytes: number }> = [];
  private readonly waiters: Array<
    (result: IteratorResult<ViewUpdate>) => void
  > = [];
  private attachedStreamId: string | undefined;
  private queuedBytes = 0;
  private closed = false;

  public constructor(
    private readonly channel: Channel<ViewUpdate>,
    private readonly maxQueueBytes: number,
    private readonly onClose: () => void,
  ) {}

  public get streamId(): string {
    if (this.attachedStreamId === undefined) {
      throw new Error("Tauri subscription is not attached");
    }
    return this.attachedStreamId;
  }

  public attach(streamId: string): void {
    this.attachedStreamId = streamId;
  }

  public push(update: ViewUpdate): void {
    if (this.closed) return;
    const waiter = this.waiters.shift();
    if (waiter !== undefined) {
      waiter({ done: false, value: update });
    } else {
      const bytes = new TextEncoder().encode(JSON.stringify(update)).byteLength;
      if (
        bytes > this.maxQueueBytes ||
        this.queuedBytes + bytes > this.maxQueueBytes
      ) {
        this.queue.length = 0;
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
        this.queue.push({ update: reset, bytes: resetBytes });
        this.queuedBytes = resetBytes;
      } else {
        this.queue.push({ update, bytes });
        this.queuedBytes += bytes;
      }
    }
  }

  public [Symbol.asyncIterator](): AsyncIterator<ViewUpdate> {
    return {
      next: () => {
        const queued = this.queue.shift();
        if (queued !== undefined) {
          this.queuedBytes -= queued.bytes;
          return Promise.resolve({ done: false, value: queued.update });
        }
        if (this.closed) {
          return Promise.resolve({ done: true, value: undefined });
        }
        return new Promise((resolve) => this.waiters.push(resolve));
      },
    };
  }

  public async resync(): Promise<void> {
    await invoke("application_resync", { streamId: this.streamId });
  }

  public async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.channel.onmessage = () => {};
    await invoke("application_unsubscribe", {
      streamId: this.streamId,
    });
    for (const waiter of this.waiters.splice(0)) {
      waiter({ done: true, value: undefined });
    }
    this.queue.length = 0;
    this.queuedBytes = 0;
    this.onClose();
  }
}
