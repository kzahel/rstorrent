export interface ApiCodec {
  readonly encoding: "json";
  encodeRequest(value: unknown): string;
  decodeResponse<T>(source: string, decoder: (source: string) => T): T;
}

export class JsonApiCodec implements ApiCodec {
  public readonly encoding = "json" as const;

  public encodeRequest(value: unknown): string {
    return JSON.stringify(value);
  }

  public decodeResponse<T>(source: string, decoder: (source: string) => T): T {
    return decoder(source);
  }
}
