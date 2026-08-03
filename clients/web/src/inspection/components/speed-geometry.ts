export function monotoneTangents(values: readonly number[]): number[] {
  if (values.length <= 1) return values.map(() => 0);
  const slopes = values.slice(1).map((value, index) => value - values[index]!);
  const tangents = new Array<number>(values.length).fill(0);
  tangents[0] = slopes[0] ?? 0;
  tangents[tangents.length - 1] = slopes.at(-1) ?? 0;
  for (let index = 1; index < values.length - 1; index += 1) {
    const before = slopes[index - 1] ?? 0;
    const after = slopes[index] ?? 0;
    tangents[index] =
      before === 0 || after === 0 || Math.sign(before) !== Math.sign(after)
        ? 0
        : (2 * before * after) / (before + after);
  }
  for (let index = 0; index < slopes.length; index += 1) {
    const slope = slopes[index] ?? 0;
    if (slope === 0) {
      tangents[index] = 0;
      tangents[index + 1] = 0;
      continue;
    }
    const left = (tangents[index] ?? 0) / slope;
    const right = (tangents[index + 1] ?? 0) / slope;
    const magnitude = left * left + right * right;
    if (magnitude > 9) {
      const scale = 3 / Math.sqrt(magnitude);
      tangents[index] = scale * left * slope;
      tangents[index + 1] = scale * right * slope;
    }
  }
  return tangents;
}

export function monotoneCubicValue(
  start: number,
  end: number,
  startTangent: number,
  endTangent: number,
  progress: number,
): number {
  const t = Math.max(0, Math.min(1, progress));
  const t2 = t * t;
  const t3 = t2 * t;
  return (
    (2 * t3 - 3 * t2 + 1) * start +
    (t3 - 2 * t2 + t) * startTangent +
    (-2 * t3 + 3 * t2) * end +
    (t3 - t2) * endTangent
  );
}

export function contiguousRuns(
  values: readonly (number | null)[],
): readonly { readonly start: number; readonly values: readonly number[] }[] {
  const runs: { start: number; values: number[] }[] = [];
  let current: { start: number; values: number[] } | null = null;
  values.forEach((value, index) => {
    if (value === null) {
      current = null;
    } else if (current === null) {
      current = { start: index, values: [value] };
      runs.push(current);
    } else {
      current.values.push(value);
    }
  });
  return runs;
}

export function decayedScaleMaximum(
  current: number,
  target: number,
  elapsedMillis: number,
  animate: boolean,
): number {
  if (!animate || current <= 0 || target >= current) return target;
  const remaining = 2 ** (-Math.max(0, elapsedMillis) / 2_000);
  return target + (current - target) * remaining;
}
