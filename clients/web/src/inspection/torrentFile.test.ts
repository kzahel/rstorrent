import { describe, expect, it, vi } from "vitest";

import {
  MAX_TORRENT_FILE_BYTES,
  readTorrentFile,
  torrentFileSizeError,
} from "./torrentFile";

describe("torrent file intake", () => {
  it("checks numeric boundaries without allocating maximum-size files", () => {
    expect(torrentFileSizeError(0)).toContain("at least one byte");
    expect(torrentFileSizeError(1)).toBeNull();
    expect(torrentFileSizeError(MAX_TORRENT_FILE_BYTES)).toBeNull();
    expect(torrentFileSizeError(MAX_TORRENT_FILE_BYTES + 1)).toContain("64 MiB");
    expect(torrentFileSizeError(Number.NaN)).toContain("at least one byte");
  });

  it("reads once and rejects read failures or a changed size", async () => {
    const source = new Uint8Array([1, 2, 3]).buffer;
    const arrayBuffer = vi.fn().mockResolvedValue(source);
    await expect(readTorrentFile({ size: 3, arrayBuffer })).resolves.toBe(source);
    expect(arrayBuffer).toHaveBeenCalledOnce();

    await expect(
      readTorrentFile({
        size: 3,
        arrayBuffer: vi.fn().mockRejectedValue(new Error("not readable")),
      }),
    ).rejects.toThrow("Could not read the torrent file: not readable");
    await expect(
      readTorrentFile({ size: 4, arrayBuffer: vi.fn().mockResolvedValue(source) }),
    ).rejects.toThrow("changed while it was being read");
  });
});
