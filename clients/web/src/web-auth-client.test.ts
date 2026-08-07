import { describe, expect, it, vi } from "vitest";

import { WebAuthClient } from "./web-auth-client";

describe("WebAuthClient", () => {
  it("sends cookie credentials and validates bounded status", async () => {
    const fetchImplementation = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(
        JSON.stringify({
          available: true,
          state: "initial_window_open",
          remaining_seconds: 599,
        }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      ),
    );
    const client = new WebAuthClient(
      "http://127.0.0.1:3030/",
      fetchImplementation,
    );
    await expect(client.status()).resolves.toMatchObject({
      state: "initial_window_open",
      remaining_seconds: 599,
    });
    expect(fetchImplementation).toHaveBeenCalledWith(
      new URL("http://127.0.0.1:3030/api/v1/web-auth/status"),
      expect.objectContaining({ method: "GET", credentials: "include" }),
    );
  });

  it("rejects malformed sessions and exposes bounded API messages", async () => {
    const fetchImplementation = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ sessions: [{ id: "bad" }] }), {
          status: 200,
        }),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({ error: { message: "pairing code expired" } }),
          { status: 401 },
        ),
      );
    const client = new WebAuthClient(
      "http://127.0.0.1:3030/",
      fetchImplementation,
    );
    await expect(client.sessions()).rejects.toThrow("response was malformed");
    await expect(client.redeem("1234", "Browser")).rejects.toThrow(
      "pairing code expired",
    );
  });
});
