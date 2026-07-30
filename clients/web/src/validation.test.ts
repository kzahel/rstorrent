import { describe, expect, it } from "vitest";

import { ContractError, decodeGatewayServerMessage } from "./validation";

describe("gateway validation", () => {
  it("rejects unknown variants and non-canonical ranges", () => {
    expect(() =>
      decodeGatewayServerMessage(JSON.stringify({ type: "invented" })),
    ).toThrow(ContractError);
    expect(() =>
      decodeGatewayServerMessage(
        JSON.stringify({
          type: "update",
          update: {
            contract_version: 1,
            stream_id: "1",
            epoch: "1",
            sequence: "1",
            base_revision: "0",
            revision: "0",
            type: "snapshot",
            snapshot: {
              type: "piece_activity",
              torrent_id: "0".repeat(40),
              piece_count: 2,
              verified: [{ start: 1, end_exclusive: 3 }],
              active: null,
            },
          },
        }),
      ),
    ).toThrow(ContractError);
  });
});
