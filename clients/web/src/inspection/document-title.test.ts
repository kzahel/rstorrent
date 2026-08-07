import { describe, expect, it } from "vitest";

import type { SessionSummary } from "./model";
import { documentTitleForSession } from "./document-title";

const CONNECTED: SessionSummary = {
  connection: "connected",
  downloadRate: 0,
  uploadRate: 0,
  dhtNodes: null,
  knownPeers: null,
};

describe("documentTitleForSession", () => {
  it("keeps the application title while idle or disconnected", () => {
    expect(documentTitleForSession(CONNECTED, "decimal")).toBe("RSTorrent");
    expect(
      documentTitleForSession(
        { ...CONNECTED, connection: "reconnecting", downloadRate: 1_500_000 },
        "decimal",
      ),
    ).toBe("RSTorrent");
  });

  it("shows both directions during an active download", () => {
    expect(
      documentTitleForSession(
        { ...CONNECTED, downloadRate: 1_500_000, uploadRate: 24_000 },
        "decimal",
      ),
    ).toBe("RSTorrent - ↓1.5 MB/s ↑24.0 kB/s");
  });

  it("shows upload-only activity and follows binary units", () => {
    expect(
      documentTitleForSession(
        { ...CONNECTED, downloadRate: 0, uploadRate: 1_536 },
        "binary",
      ),
    ).toBe("RSTorrent - ↓0 B/s ↑1.5 KiB/s");
  });

  it("does not present an unavailable direction as zero", () => {
    expect(
      documentTitleForSession(
        { ...CONNECTED, downloadRate: 1_000, uploadRate: null },
        "decimal",
      ),
    ).toBe("RSTorrent - ↓1.0 kB/s ↑—");
  });
});
