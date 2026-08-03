import { describe, expect, it } from "vitest";

import {
  contiguousRuns,
  decayedScaleMaximum,
  monotoneCubicValue,
  monotoneTangents,
} from "./speed-geometry";

describe("speed chart geometry", () => {
  it("does not overshoot monotone samples", () => {
    const values = [0, 4, 5, 12, 12, 20];
    const tangents = monotoneTangents(values);
    for (let index = 0; index < values.length - 1; index += 1) {
      const low = Math.min(values[index]!, values[index + 1]!);
      const high = Math.max(values[index]!, values[index + 1]!);
      for (let step = 0; step <= 100; step += 1) {
        const value = monotoneCubicValue(
          values[index]!,
          values[index + 1]!,
          tangents[index]!,
          tangents[index + 1]!,
          step / 100,
        );
        expect(value).toBeGreaterThanOrEqual(low - 1e-9);
        expect(value).toBeLessThanOrEqual(high + 1e-9);
      }
    }
  });

  it("sets turning-point tangents to zero", () => {
    expect(monotoneTangents([2, 9, 3])).toEqual([7, 0, -6]);
  });

  it("keeps unavailable intervals as separate runs", () => {
    expect(contiguousRuns([null, 1, 2, null, null, 3])).toEqual([
      { start: 1, values: [1, 2] },
      { start: 5, values: [3] },
    ]);
  });

  it("grows immediately and decays with a two-second half-life", () => {
    expect(decayedScaleMaximum(10, 20, 100, true)).toBe(20);
    expect(decayedScaleMaximum(20, 10, 2_000, true)).toBe(15);
    expect(decayedScaleMaximum(20, 10, 2_000, false)).toBe(10);
  });
});
