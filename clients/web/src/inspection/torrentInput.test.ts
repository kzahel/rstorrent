import { describe, expect, it } from "vitest";

import {
  MAX_TORRENT_INPUT_BYTES,
  validateTorrentInput,
} from "./torrentInput";

describe("validateTorrentInput", () => {
  it("trims and accepts a bounded magnet link", () => {
    expect(
      validateTorrentInput(
        "  magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213  ",
      ),
    ).toEqual({
      accepted: true,
      magnet: "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213",
    });
  });

  it("distinguishes empty, remote, file, and malformed input", () => {
    expect(validateTorrentInput("   ")).toMatchObject({
      accepted: false,
      message: expect.stringContaining("file selection"),
    });
    expect(validateTorrentInput("https://example.test/file.torrent")).toMatchObject({
      accepted: false,
      message: expect.stringContaining("Remote .torrent URLs"),
    });
    expect(validateTorrentInput("file:///tmp/file.torrent")).toMatchObject({
      accepted: false,
      message: expect.stringContaining("file selection"),
    });
    expect(validateTorrentInput("not a magnet")).toEqual({
      accepted: false,
      message: "Enter a magnet link beginning with magnet:?",
    });
  });

  it("bounds the UTF-8 input before application dispatch", () => {
    const oversized = `magnet:?xt=urn:btih:${"é".repeat(MAX_TORRENT_INPUT_BYTES)}`;
    expect(validateTorrentInput(oversized)).toMatchObject({
      accepted: false,
      message: expect.stringContaining("16,384 bytes"),
    });
  });
});
