import type {
  ApiHello,
  OpenViewSetRequest,
  OpenViewSetResponse,
  RequestEnvelope,
  ResponseEnvelope,
  UpdateBatch,
  UpdateViewSetRequest,
} from "./generated/v1";
import { JsonApiCodec, type ApiCodec } from "./codec";
import {
  decodeApiErrorEnvelope,
  decodeApiHello,
  decodeOpenViewSetResponse,
  decodeResponseEnvelope,
  decodeUpdateBatch,
} from "../validation";

const MAX_RESPONSE_BYTES = 512 * 1024;

export interface ApplicationViewClient {
  hello(signal?: AbortSignal): Promise<ApiHello>;
  dispatch(
    request: RequestEnvelope,
    signal?: AbortSignal,
  ): Promise<ResponseEnvelope>;
  openViewSet(
    request: OpenViewSetRequest,
    signal?: AbortSignal,
  ): Promise<OpenViewSetResponse>;
  updateViewSet(
    viewSetId: string,
    request: UpdateViewSetRequest,
    signal?: AbortSignal,
  ): Promise<void>;
  nextUpdates(
    viewSetId: string,
    after: string,
    waitMillis: number,
    signal?: AbortSignal,
  ): Promise<UpdateBatch>;
  closeViewSet(viewSetId: string, signal?: AbortSignal): Promise<void>;
  close(): Promise<void>;
}

export type FetchImplementation = (
  input: RequestInfo | URL,
  init?: RequestInit,
) => Promise<Response>;

export class HttpApiError extends Error {
  public constructor(
    public readonly status: number,
    public readonly code: string,
    message: string,
  ) {
    super(`${code}: ${message}`);
  }
}

export class HttpApplicationClient implements ApplicationViewClient {
  private closed = false;

  public constructor(
    private readonly baseUrl: string,
    private readonly token: string,
    private readonly origin: string,
    private readonly fetchImplementation: FetchImplementation = globalThis.fetch,
    private readonly codec: ApiCodec = new JsonApiCodec(),
  ) {
    if (token.length === 0 || token.length > 128) {
      throw new Error("gateway token must be 1..=128 characters");
    }
    if (origin.length === 0 || origin.length > 512) {
      throw new Error("gateway origin must be 1..=512 characters");
    }
  }

  public async hello(signal?: AbortSignal): Promise<ApiHello> {
    return this.request("GET", "/api/v1/hello", undefined, decodeApiHello, signal);
  }

  public async dispatch(
    request: RequestEnvelope,
    signal?: AbortSignal,
  ): Promise<ResponseEnvelope> {
    return this.request(
      "POST",
      "/api/v1/commands",
      request,
      decodeResponseEnvelope,
      signal,
    );
  }

  public async openViewSet(
    request: OpenViewSetRequest,
    signal?: AbortSignal,
  ): Promise<OpenViewSetResponse> {
    return this.request(
      "POST",
      "/api/v1/view-sets",
      request,
      decodeOpenViewSetResponse,
      signal,
    );
  }

  public async updateViewSet(
    viewSetId: string,
    request: UpdateViewSetRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    await this.requestWithoutBody(
      "PUT",
      `/api/v1/view-sets/${encodeURIComponent(viewSetId)}/views`,
      request,
      signal,
    );
  }

  public async nextUpdates(
    viewSetId: string,
    after: string,
    waitMillis: number,
    signal?: AbortSignal,
  ): Promise<UpdateBatch> {
    const query = new URLSearchParams({
      after,
      wait_ms: String(waitMillis),
    });
    return this.request(
      "GET",
      `/api/v1/view-sets/${encodeURIComponent(viewSetId)}/updates?${query}`,
      undefined,
      decodeUpdateBatch,
      signal,
    );
  }

  public async closeViewSet(
    viewSetId: string,
    signal?: AbortSignal,
  ): Promise<void> {
    await this.requestWithoutBody(
      "DELETE",
      `/api/v1/view-sets/${encodeURIComponent(viewSetId)}`,
      undefined,
      signal,
    );
  }

  public async close(): Promise<void> {
    this.closed = true;
  }

  private async request<T>(
    method: string,
    path: string,
    body: unknown,
    decoder: (source: string) => T,
    signal: AbortSignal | undefined,
  ): Promise<T> {
    const response = await this.send(method, path, body, signal);
    const source = await boundedResponseText(response);
    if (!response.ok) throw decodeHttpError(response.status, source);
    return this.codec.decodeResponse(source, decoder);
  }

  private async requestWithoutBody(
    method: string,
    path: string,
    body: unknown,
    signal: AbortSignal | undefined,
  ): Promise<void> {
    const response = await this.send(method, path, body, signal);
    const source = await boundedResponseText(response);
    if (!response.ok) throw decodeHttpError(response.status, source);
    if (source.length !== 0) {
      throw new Error("no-content API response unexpectedly carried a body");
    }
  }

  private send(
    method: string,
    path: string,
    body: unknown,
    signal: AbortSignal | undefined,
  ): Promise<Response> {
    if (this.closed) return Promise.reject(new Error("gateway client is closed"));
    const encoded = body === undefined ? undefined : this.codec.encodeRequest(body);
    return this.fetchImplementation(new URL(path, this.baseUrl), {
      method,
      headers: {
        Accept: "application/json",
        Authorization: `Bearer ${this.token}`,
        Origin: this.origin,
        ...(encoded === undefined ? {} : { "Content-Type": "application/json" }),
      },
      ...(encoded === undefined ? {} : { body: encoded }),
      ...(signal === undefined ? {} : { signal }),
    });
  }
}

async function boundedResponseText(response: Response): Promise<string> {
  const declared = response.headers.get("content-length");
  if (declared !== null && Number(declared) > MAX_RESPONSE_BYTES) {
    throw new Error("gateway response exceeds the client bound");
  }
  if (response.body === null) return "";
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let total = 0;
  let output = "";
  while (true) {
    const chunk = await reader.read();
    if (chunk.done) break;
    total += chunk.value.byteLength;
    if (total > MAX_RESPONSE_BYTES) {
      await reader.cancel();
      throw new Error("gateway response exceeds the client bound");
    }
    output += decoder.decode(chunk.value, { stream: true });
  }
  output += decoder.decode();
  return output;
}

function decodeHttpError(status: number, source: string): Error {
  try {
    const envelope = decodeApiErrorEnvelope(source);
    return new HttpApiError(status, envelope.error.code, envelope.error.message);
  } catch (error) {
    return new HttpApiError(
      status,
      "invalid_error_response",
      error instanceof Error ? error.message : String(error),
    );
  }
}
