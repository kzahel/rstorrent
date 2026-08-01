import type {
  RequestEnvelope,
  ResponseEnvelope,
  SubscriptionSpec,
  ViewUpdate,
} from "./api";

export interface ApplicationSubscription extends AsyncIterable<ViewUpdate> {
  readonly streamId: string;
  resync(): Promise<void>;
  close(): Promise<void>;
}

export interface ApplicationClient {
  dispatch(request: RequestEnvelope): Promise<ResponseEnvelope>;
  subscribe(spec: SubscriptionSpec): Promise<ApplicationSubscription>;
  close(): Promise<void>;
}

export class InMemoryApplicationClient implements ApplicationClient {
  public constructor(
    private readonly dispatchHandler: (
      request: RequestEnvelope,
    ) => Promise<ResponseEnvelope>,
    private readonly subscriptionHandler: (
      spec: SubscriptionSpec,
    ) => Promise<ApplicationSubscription>,
  ) {}

  public dispatch(request: RequestEnvelope): Promise<ResponseEnvelope> {
    return this.dispatchHandler(request);
  }

  public subscribe(
    spec: SubscriptionSpec,
  ): Promise<ApplicationSubscription> {
    return this.subscriptionHandler(spec);
  }

  public async close(): Promise<void> {}
}
