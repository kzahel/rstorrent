import { describe, expect, it } from "vitest";

import { isAllowedLiveGateway } from "./bootstrap";

describe("live gateway policy", () => {
  it("accepts explicit loopback HTTP and exact same-origin HTTPS", () => {
    expect(
      isAllowedLiveGateway(
        new URL("http://127.0.0.1:3030"),
        "https://preview.example",
      ),
    ).toBe(true);
    expect(
      isAllowedLiveGateway(
        new URL("https://preview.example"),
        "https://preview.example",
      ),
    ).toBe(true);
  });

  it("rejects remote HTTP, cross-origin HTTPS, credentials, and subpaths", () => {
    expect(
      isAllowedLiveGateway(
        new URL("http://preview.example"),
        "http://preview.example",
      ),
    ).toBe(false);
    expect(
      isAllowedLiveGateway(
        new URL("https://other.example"),
        "https://preview.example",
      ),
    ).toBe(false);
    expect(
      isAllowedLiveGateway(
        new URL("https://user:secret@preview.example"),
        "https://preview.example",
      ),
    ).toBe(false);
    expect(
      isAllowedLiveGateway(
        new URL("https://preview.example/nested"),
        "https://preview.example",
      ),
    ).toBe(false);
  });
});
