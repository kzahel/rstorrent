export const DEFAULT_MAX_CAPTURED_FRAMES = 100_000;
export const DEFAULT_MAX_CAPTURED_PAYLOAD_BYTES = 64 * 1024 * 1024;

export type ApplicationFrameDirection = "client_to_server" | "server_to_client";

export interface CapturedApplicationFrame {
  readonly direction: ApplicationFrameDirection;
  readonly payload: string | Uint8Array;
}

export interface FrameFamilyBandwidth {
  readonly messages: number;
  readonly payload_bytes: number;
}

export interface DirectionBandwidth {
  readonly messages: number;
  readonly text_messages: number;
  readonly binary_messages: number;
  readonly payload_bytes: number;
  readonly binary_payload_bytes: number;
  readonly websocket_bytes_estimate: number;
  readonly frame_families: Readonly<Record<string, FrameFamilyBandwidth>>;
}

export interface ViewUpdateBandwidth {
  readonly updates: number;
  readonly snapshots: number;
  readonly patches: number;
  readonly removals: number;
  readonly resets: number;
  readonly update_json_bytes: number;
}

export interface SemanticBandwidth {
  readonly batches: number;
  readonly initial_batches: number;
  readonly streamed_batches: number;
  readonly empty_batches: number;
  readonly reset_batches: number;
  readonly reset_frame_payload_bytes: number;
  readonly view_updates: Readonly<Record<string, ViewUpdateBandwidth>>;
}

export interface ApplicationBandwidthSummary {
  readonly client_to_server: DirectionBandwidth;
  readonly server_to_client: DirectionBandwidth;
  readonly semantic: SemanticBandwidth;
}

interface MutableFrameFamilyBandwidth {
  messages: number;
  payload_bytes: number;
}

interface MutableDirectionBandwidth {
  messages: number;
  text_messages: number;
  binary_messages: number;
  payload_bytes: number;
  binary_payload_bytes: number;
  websocket_bytes_estimate: number;
  frame_families: Record<string, MutableFrameFamilyBandwidth>;
}

interface MutableViewUpdateBandwidth {
  updates: number;
  snapshots: number;
  patches: number;
  removals: number;
  resets: number;
  update_json_bytes: number;
}

interface MutableSemanticBandwidth {
  batches: number;
  initial_batches: number;
  streamed_batches: number;
  empty_batches: number;
  reset_batches: number;
  reset_frame_payload_bytes: number;
  view_updates: Record<string, MutableViewUpdateBandwidth>;
}

export class BoundedApplicationFrameCapture {
  private readonly frames: CapturedApplicationFrame[] = [];
  private capturedPayloadBytes = 0;

  constructor(
    private readonly maximumFrames = DEFAULT_MAX_CAPTURED_FRAMES,
    private readonly maximumPayloadBytes = DEFAULT_MAX_CAPTURED_PAYLOAD_BYTES,
  ) {
    if (maximumFrames <= 0 || maximumPayloadBytes <= 0) {
      throw new Error("application frame capture bounds must be positive");
    }
  }

  add(frame: CapturedApplicationFrame): void {
    const payloadBytes = encodedBytes(frame.payload);
    if (this.frames.length >= this.maximumFrames) {
      throw new Error(
        `application frame capture exceeds ${this.maximumFrames} frames`,
      );
    }
    if (this.capturedPayloadBytes + payloadBytes > this.maximumPayloadBytes) {
      throw new Error(
        `application frame capture exceeds ${this.maximumPayloadBytes} payload bytes`,
      );
    }
    this.frames.push({
      direction: frame.direction,
      payload:
        typeof frame.payload === "string"
          ? frame.payload
          : frame.payload.slice(),
    });
    this.capturedPayloadBytes += payloadBytes;
  }

  mark(): number {
    return this.frames.length;
  }

  summarize(start = 0, end = this.frames.length): ApplicationBandwidthSummary {
    if (start < 0 || end < start || end > this.frames.length) {
      throw new Error(`invalid application frame range ${start}..${end}`);
    }
    return summarizeApplicationFrames(this.frames.slice(start, end));
  }
}

export function summarizeApplicationFrames(
  frames: readonly CapturedApplicationFrame[],
): ApplicationBandwidthSummary {
  const directions: Record<ApplicationFrameDirection, MutableDirectionBandwidth> = {
    client_to_server: emptyDirection(),
    server_to_client: emptyDirection(),
  };
  const semantic: MutableSemanticBandwidth = {
    batches: 0,
    initial_batches: 0,
    streamed_batches: 0,
    empty_batches: 0,
    reset_batches: 0,
    reset_frame_payload_bytes: 0,
    view_updates: {},
  };

  for (const frame of frames) {
    const direction = directions[frame.direction];
    const bytes = encodedBytes(frame.payload);
    direction.messages += 1;
    direction.payload_bytes += bytes;
    direction.websocket_bytes_estimate += websocketFrameBytes(
      bytes,
      frame.direction === "client_to_server",
    );
    if (typeof frame.payload !== "string") {
      direction.binary_messages += 1;
      direction.binary_payload_bytes += bytes;
      incrementFamily(direction.frame_families, "binary", bytes);
      continue;
    }

    direction.text_messages += 1;
    const value = parseFrame(frame.payload);
    incrementFamily(direction.frame_families, value.type, bytes);
    if (frame.direction !== "server_to_client") continue;

    if (value.type === "view_batch") {
      recordBatch(value.batch, bytes, "streamed", semantic);
    } else if (
      value.type === "result" &&
      isRecord(value.result) &&
      value.result.type === "view_set_opened" &&
      isRecord(value.result.response)
    ) {
      recordBatch(value.result.response.initial, bytes, "initial", semantic);
    }
  }

  return {
    client_to_server: freezeDirection(directions.client_to_server),
    server_to_client: freezeDirection(directions.server_to_client),
    semantic: {
      ...semantic,
      view_updates: orderedRecord(semantic.view_updates),
    },
  };
}

export function encodedBytes(payload: string | Uint8Array): number {
  return typeof payload === "string"
    ? new TextEncoder().encode(payload).byteLength
    : payload.byteLength;
}

export function websocketFrameBytes(payloadBytes: number, masked: boolean): number {
  if (!Number.isSafeInteger(payloadBytes) || payloadBytes < 0) {
    throw new Error("WebSocket payload bytes must be a non-negative integer");
  }
  const headerBytes = payloadBytes <= 125 ? 2 : payloadBytes <= 65_535 ? 4 : 10;
  return payloadBytes + headerBytes + (masked ? 4 : 0);
}

function emptyDirection(): MutableDirectionBandwidth {
  return {
    messages: 0,
    text_messages: 0,
    binary_messages: 0,
    payload_bytes: 0,
    binary_payload_bytes: 0,
    websocket_bytes_estimate: 0,
    frame_families: {},
  };
}

function freezeDirection(value: MutableDirectionBandwidth): DirectionBandwidth {
  return {
    ...value,
    frame_families: orderedRecord(value.frame_families),
  };
}

function incrementFamily(
  families: Record<string, MutableFrameFamilyBandwidth>,
  family: string,
  bytes: number,
): void {
  const current = families[family] ?? { messages: 0, payload_bytes: 0 };
  current.messages += 1;
  current.payload_bytes += bytes;
  families[family] = current;
}

function parseFrame(payload: string): Record<string, unknown> & { type: string } {
  let value: unknown;
  try {
    value = JSON.parse(payload);
  } catch (error) {
    throw new Error(`application text frame is not JSON: ${String(error)}`);
  }
  if (!isRecord(value) || typeof value.type !== "string") {
    throw new Error("application text frame has no string type");
  }
  return value as Record<string, unknown> & { type: string };
}

function recordBatch(
  value: unknown,
  framePayloadBytes: number,
  source: "initial" | "streamed",
  semantic: MutableSemanticBandwidth,
): void {
  if (!isRecord(value) || !Array.isArray(value.updates)) {
    throw new Error("application server frame contains an invalid update batch");
  }
  semantic.batches += 1;
  if (source === "initial") semantic.initial_batches += 1;
  else semantic.streamed_batches += 1;
  if (value.updates.length === 0) semantic.empty_batches += 1;

  let reset = false;
  for (const update of value.updates) {
    if (!isRecord(update) || typeof update.type !== "string") {
      throw new Error("application update has no string type");
    }
    const viewId =
      typeof update.view_id === "string" ? update.view_id : "<view-set>";
    const current = semantic.view_updates[viewId] ?? emptyViewUpdate();
    current.updates += 1;
    current.update_json_bytes += encodedBytes(JSON.stringify(update));
    if (update.type === "snapshot") current.snapshots += 1;
    else if (update.type === "patch") current.patches += 1;
    else if (update.type === "view_removed") current.removals += 1;
    else if (update.type === "reset_required") {
      current.resets += 1;
      reset = true;
    }
    semantic.view_updates[viewId] = current;
  }
  if (reset) {
    semantic.reset_batches += 1;
    semantic.reset_frame_payload_bytes += framePayloadBytes;
  }
}

function emptyViewUpdate(): MutableViewUpdateBandwidth {
  return {
    updates: 0,
    snapshots: 0,
    patches: 0,
    removals: 0,
    resets: 0,
    update_json_bytes: 0,
  };
}

function orderedRecord<T>(value: Record<string, T>): Readonly<Record<string, T>> {
  return Object.fromEntries(
    Object.entries(value).sort(([left], [right]) => left.localeCompare(right)),
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
