import type {
  OpenViewSetRequest,
  RequestEnvelope,
  ResponseEnvelope,
  UpdateViewSetRequest,
  ViewSpec,
} from "./api";
import { HttpApiError, type ApplicationViewClient } from "./api/client";
import {
  reduceOpenViewSet,
  reduceUpdateBatch,
  ViewSetContinuityError,
  type ViewSetState,
} from "./view-set-reducer";
import { ContractError } from "./validation";

export interface ViewControllerOptions {
  waitMillis?: number;
  retryBaseMillis?: number;
  retryMaximumMillis?: number;
}

export type ViewStateListener = (state: ViewSetState) => void;
export type ViewErrorListener = (error: Error) => void;

export class ViewController {
  private readonly waitMillis: number;
  private readonly retryBaseMillis: number;
  private readonly retryMaximumMillis: number;
  private state: ViewSetState;
  private inFlight: AbortController | undefined;
  private polling: Promise<void> | undefined;
  private closed = false;
  private views: ViewSpec[];

  private constructor(
    private readonly client: ApplicationViewClient,
    initial: ViewSetState,
    private readonly onState: ViewStateListener,
    private readonly onError: ViewErrorListener,
    options: ViewControllerOptions,
    views: ViewSpec[],
  ) {
    this.state = initial;
    this.waitMillis = options.waitMillis ?? 20_000;
    this.retryBaseMillis = options.retryBaseMillis ?? 250;
    this.retryMaximumMillis = options.retryMaximumMillis ?? 2_000;
    this.views = [...views];
  }

  public static async open(
    client: ApplicationViewClient,
    views: ViewSpec[],
    onState: ViewStateListener,
    onError: ViewErrorListener = () => {},
    options: ViewControllerOptions = {},
  ): Promise<ViewController> {
    const request: OpenViewSetRequest = { views, options: {} };
    const response = await client.openViewSet(request);
    const initial = reduceOpenViewSet(response);
    const controller = new ViewController(
      client,
      initial,
      onState,
      onError,
      options,
      views,
    );
    try {
      onState(initial);
    } catch (error) {
      await client.closeViewSet(response.view_set_id);
      throw error;
    }
    controller.polling = controller.poll();
    return controller;
  }

  public current(): ViewSetState {
    return this.state;
  }

  public async dispatch(request: RequestEnvelope): Promise<ResponseEnvelope> {
    this.ensureOpen();
    const response = await this.client.dispatch(request);
    this.wake();
    return response;
  }

  public async setViews(views: ViewSpec[]): Promise<void> {
    this.ensureOpen();
    const request: UpdateViewSetRequest = { views };
    await this.client.updateViewSet(this.state.viewSetId, request);
    this.views = [...views];
    this.wake();
  }

  public async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.inFlight?.abort("view controller closed");
    await this.polling;
    await this.client.closeViewSet(this.state.viewSetId);
  }

  private async poll(): Promise<void> {
    let retryMillis = this.retryBaseMillis;
    while (!this.closed) {
      const request = new AbortController();
      this.inFlight = request;
      try {
        const batch = await this.client.nextUpdates(
          this.state.viewSetId,
          this.state.cursor,
          this.waitMillis,
          request.signal,
        );
        if (request.signal.aborted) continue;
        const next = reduceUpdateBatch(this.state, batch);
        if (next !== this.state) {
          this.onState(next);
          this.state = next;
        }
        retryMillis = this.retryBaseMillis;
      } catch (error) {
        if (request.signal.aborted) continue;
        const failure = asError(error);
        this.onError(failure);
        if (
          failure instanceof HttpApiError &&
          (failure.code === "unknown_view_set" ||
            failure.code === "view_set_closed")
        ) {
          try {
            const reopened = await this.client.openViewSet(
              { views: this.views, options: {} },
              request.signal,
            );
            const next = reduceOpenViewSet(reopened);
            this.onState(next);
            this.state = next;
            retryMillis = this.retryBaseMillis;
            continue;
          } catch (reopenError) {
            if (request.signal.aborted) continue;
            this.onError(asError(reopenError));
          }
        }
        if (
          failure instanceof ContractError ||
          failure instanceof ViewSetContinuityError
        ) {
          break;
        }
        await abortableDelay(retryMillis, request.signal);
        retryMillis = Math.min(retryMillis * 2, this.retryMaximumMillis);
      } finally {
        if (this.inFlight === request) this.inFlight = undefined;
      }
    }
  }

  private wake(): void {
    this.inFlight?.abort("immediate poll requested");
  }

  private ensureOpen(): void {
    if (this.closed) throw new Error("view controller is closed");
  }
}

function abortableDelay(millis: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    const timer = globalThis.setTimeout(resolve, millis);
    signal.addEventListener(
      "abort",
      () => {
        globalThis.clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
  });
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
