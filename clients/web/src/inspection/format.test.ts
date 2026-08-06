import { describe, expect, it } from "vitest";

import { formatBytes, formatExactBytes, formatRate } from "./format";

describe("byte formatting", () => {
  it.each([
    [0, "0 B", "0 B"],
    [999, "999 B", "999 B"],
    [1_000, "1.0 kB", "1.0 kB"],
    [999_999, "1000 kB", "999 kB"],
    [1_000_000, "1.0 MB", "1.0 MB"],
    [1_000_000_000, "1.0 GB", "1.0 GB"],
    [1_000_000_000_000, "1.0 TB", "1.0 TB"],
    [1_000_000_000_000_000, "1.0 PB", "1.0 PB"],
  ])("formats %s bytes in decimal units", (value, numberText, exactText) => {
    expect(formatBytes(value, "decimal")).toBe(numberText);
    expect(formatExactBytes(String(value), "decimal")).toBe(exactText);
  });

  it.each([
    [0, "0 B", "0 B"],
    [1_023, "1023 B", "1023 B"],
    [1_024, "1.0 KiB", "1.0 KiB"],
    [1_048_575, "1024 KiB", "1023 KiB"],
    [1_048_576, "1.0 MiB", "1.0 MiB"],
    [1_073_741_824, "1.0 GiB", "1.0 GiB"],
    [1_099_511_627_776, "1.0 TiB", "1.0 TiB"],
    [1_125_899_906_842_624, "1.0 PiB", "1.0 PiB"],
  ])("formats %s bytes in binary units", (value, numberText, exactText) => {
    expect(formatBytes(value, "binary")).toBe(numberText);
    expect(formatExactBytes(String(value), "binary")).toBe(exactText);
  });

  it("retains unavailable and rate policy", () => {
    expect(formatBytes(null, "decimal")).toBe("—");
    expect(formatExactBytes("not-an-integer", "binary")).toBe("—");
    expect(formatRate(0, "decimal")).toBe("—");
    expect(formatRate(null, "binary")).toBe("—");
    expect(formatRate(1_000, "decimal")).toBe("1.0 kB/s");
    expect(formatRate(1_024, "binary")).toBe("1.0 KiB/s");
  });

  it("keeps exact integer inputs precise above Number.MAX_SAFE_INTEGER", () => {
    expect(formatExactBytes("9007199254740993", "decimal")).toBe("9.0 PB");
    expect(formatExactBytes("9007199254740993", "binary")).toBe("8.0 PiB");
    expect(formatExactBytes("999999999999999999999999", "decimal")).toBe(
      "999999999 PB",
    );
    expect(formatExactBytes("999999999999999999999999", "binary")).toBe(
      "888178419 PiB",
    );
  });
});
