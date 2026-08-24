import { invoke, type InvokeArgs } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const EXTERNAL_INTAKE_EVENT = "rstorrent://external-torrent-intake";
const MAX_PENDING_ACTIVATIONS = 8;
const ACTIVATION_ID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const GENERATION = /^(0|[1-9][0-9]{0,19})$/;

export type DesktopExternalActivationKind = "magnet" | "torrent_file";

export interface DesktopExternalActivation {
  readonly id: string;
  readonly kind: DesktopExternalActivationKind;
}

export interface DesktopExternalIntakeSnapshot {
  readonly generation: string;
  readonly pending: readonly DesktopExternalActivation[];
  readonly rejectedCount: number;
  readonly overflowCount: number;
}

export interface DesktopExternalIntake {
  getSnapshot(): DesktopExternalIntakeSnapshot;
  subscribe(listener: () => void): () => void;
  synchronize(): Promise<void>;
  cancel(activationId: string): Promise<void>;
  consumeNotices(): void;
  close(): void;
}

export interface DesktopExternalIntakeBridge {
  invoke<T>(command: string, arguments_?: InvokeArgs): Promise<T>;
  listen(event: string, handler: () => void): Promise<() => void>;
}

const EMPTY_SNAPSHOT: DesktopExternalIntakeSnapshot = {
  generation: "0",
  pending: [],
  rejectedCount: 0,
  overflowCount: 0,
};

const defaultBridge: DesktopExternalIntakeBridge = {
  invoke: <T>(command: string, arguments_?: InvokeArgs) =>
    invoke<T>(command, arguments_),
  listen: async (event, handler) =>
    listen<unknown>(event, () => handler()),
};

export class TauriDesktopExternalIntake implements DesktopExternalIntake {
  private snapshot = EMPTY_SNAPSHOT;
  private readonly listeners = new Set<() => void>();
  private unlisten: (() => void) | null = null;
  private refreshPromise: Promise<void> | null = null;
  private refreshRequested = false;
  private closed = false;

  private constructor(private readonly bridge: DesktopExternalIntakeBridge) {}

  static async open(
    bridge: DesktopExternalIntakeBridge = defaultBridge,
  ): Promise<TauriDesktopExternalIntake> {
    const intake = new TauriDesktopExternalIntake(bridge);
    intake.unlisten = await bridge.listen(EXTERNAL_INTAKE_EVENT, () => {
      void intake.refresh().catch((error: unknown) => {
        if (!intake.closed) {
          console.error("Desktop external torrent intake refresh failed:", error);
        }
      });
    });
    try {
      await intake.refresh();
      return intake;
    } catch (error) {
      intake.close();
      throw error;
    }
  }

  getSnapshot = (): DesktopExternalIntakeSnapshot => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    if (this.closed) return () => undefined;
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  async synchronize(): Promise<void> {
    await this.refresh();
  }

  async cancel(activationId: string): Promise<void> {
    this.ensureOpen();
    validateActivationId(activationId);
    await this.bridge.invoke("desktop_external_intake_cancel", {
      activationId,
    });
    await this.refresh();
  }

  consumeNotices(): void {
    if (this.closed) return;
    if (this.snapshot.rejectedCount === 0 && this.snapshot.overflowCount === 0) {
      return;
    }
    this.snapshot = {
      ...this.snapshot,
      rejectedCount: 0,
      overflowCount: 0,
    };
    this.emit();
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.unlisten?.();
    this.unlisten = null;
    this.listeners.clear();
  }

  private refresh(): Promise<void> {
    this.ensureOpen();
    this.refreshRequested = true;
    if (this.refreshPromise !== null) return this.refreshPromise;
    this.refreshPromise = this.runRefresh().finally(() => {
      this.refreshPromise = null;
    });
    return this.refreshPromise;
  }

  private async runRefresh(): Promise<void> {
    while (this.refreshRequested && !this.closed) {
      this.refreshRequested = false;
      const next = decodeExternalIntakeSnapshot(
        await this.bridge.invoke<unknown>("desktop_external_intake_pull"),
      );
      if (this.closed) return;
      this.snapshot = next;
      this.emit();
    }
  }

  private emit(): void {
    for (const listener of this.listeners) listener();
  }

  private ensureOpen(): void {
    if (this.closed) throw new Error("desktop external torrent intake is closed");
  }
}

export function decodeExternalIntakeSnapshot(
  value: unknown,
): DesktopExternalIntakeSnapshot {
  const record = exactRecord(value, [
    "generation",
    "pending",
    "rejectedCount",
    "overflowCount",
  ]);
  const generation = record.generation;
  if (typeof generation !== "string" || !GENERATION.test(generation)) {
    throw new Error("desktop external intake generation is invalid");
  }
  if (!Array.isArray(record.pending) || record.pending.length > MAX_PENDING_ACTIVATIONS) {
    throw new Error("desktop external intake pending queue is invalid");
  }
  const pending = record.pending.map((item) => {
    const descriptor = exactRecord(item, ["id", "kind"]);
    const id = descriptor.id;
    validateActivationId(id);
    const kind = descriptor.kind;
    if (kind !== "magnet" && kind !== "torrent_file") {
      throw new Error("desktop external intake kind is invalid");
    }
    return { id, kind: kind as DesktopExternalActivationKind };
  });
  const ids = new Set(pending.map(({ id }) => id));
  if (ids.size !== pending.length) {
    throw new Error("desktop external intake activation IDs are duplicated");
  }
  return {
    generation,
    pending,
    rejectedCount: boundedCount(record.rejectedCount, "rejected"),
    overflowCount: boundedCount(record.overflowCount, "overflow"),
  };
}

function validateActivationId(value: unknown): asserts value is string {
  if (typeof value !== "string" || !ACTIVATION_ID.test(value)) {
    throw new Error("desktop external intake activation ID is invalid");
  }
}

function boundedCount(value: unknown, label: string): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < 0 ||
    value > 0xffff_ffff
  ) {
    throw new Error(`desktop external intake ${label} count is invalid`);
  }
  return value;
}

function exactRecord(
  value: unknown,
  expectedKeys: readonly string[],
): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("desktop external intake payload is invalid");
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort();
  const expected = [...expectedKeys].sort();
  if (
    keys.length !== expected.length ||
    keys.some((key, index) => key !== expected[index])
  ) {
    throw new Error("desktop external intake payload fields are invalid");
  }
  return record;
}
