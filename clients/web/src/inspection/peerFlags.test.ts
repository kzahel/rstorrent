import { describe, expect, it } from "vitest";

import {
  describePeerFlags,
  formatPeerFlags,
  PEER_FLAG_DEFINITIONS,
  PEER_FLAG_ORDER,
} from "./peerFlags";

describe("peer flag presentation", () => {
  it("uses canonical case-sensitive glyph order and removes duplicates", () => {
    expect(
      formatPeerFlags([
        "utp",
        "extension_protocol",
        "incoming",
        "download_allowed",
        "incoming",
      ]),
    ).toBe("IDxT");
  });

  it("defines a label for every typed flag", () => {
    expect(new Set(PEER_FLAG_ORDER).size).toBe(
      Object.keys(PEER_FLAG_DEFINITIONS).length,
    );
    expect(describePeerFlags(PEER_FLAG_ORDER)).toContain(
      "Encrypted or obfuscated",
    );
    expect(describePeerFlags([])).toBe("No active peer flags");
  });
});
