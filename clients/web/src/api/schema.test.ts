import { describe, expect, it } from "vitest";

import type { Command } from "./generated/v1";
import { assertApiSchema, SchemaError } from "./schema";

function validateCommand(value: unknown): void {
  assertApiSchema<Command>("Command", value);
}

describe("generated settings patch schema", () => {
  it("accepts independent and combined typed subsets", () => {
    expect(() =>
      validateCommand({
        type: "update_torrent_settings",
        torrent_id: "t1-000102030405060708090a0b0c0d0e0f",
        patch: { download_rate_limit: { type: "unlimited" } },
      }),
    ).not.toThrow();
    expect(() =>
      validateCommand({
        type: "update_torrent_settings",
        torrent_id: "t1-000102030405060708090a0b0c0d0e0f",
        patch: {
          upload_rate_limit: {
            type: "limited",
            bytes_per_second: 65_536,
          },
          download_rate_limit: { type: "unlimited" },
        },
      }),
    ).not.toThrow();
    expect(() =>
      validateCommand({
        type: "update_client_settings",
        patch: { ipv6_enabled: false, peer_connection_limit: 321 },
      }),
    ).not.toThrow();
  });

  it.each([
    { type: "update_client_settings", patch: {} },
    {
      type: "update_client_settings",
      patch: { peer_connection_limit: null },
    },
    {
      type: "update_client_settings",
      patch: { unknown_setting: true },
    },
    {
      type: "update_torrent_settings",
      torrent_id: "t1-000102030405060708090a0b0c0d0e0f",
      patch: { upload_rate_limit: { type: "limited", bytes_per_second: 1 } },
    },
  ])("rejects an empty, null, unknown, or invalid patch: %#", (command) => {
    expect(() => validateCommand(command)).toThrow(SchemaError);
  });
});
