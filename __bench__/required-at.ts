/** Read a benchmark invariant; a missing value must fail instead of biasing results. */
export const requiredAt = <T>(values: readonly T[], index: number): T => {
  const value = values[index];
  if (value === undefined) {
    throw new RangeError(`Missing benchmark value at index ${index}`);
  }
  return value;
};

/** Keep skipped timings in their original comparator slots. */
export const speedupRows = (
  names: readonly string[],
  times: readonly number[],
) => {
  if (names.length !== times.length) {
    throw new RangeError(
      "Benchmark names and timings must have the same length",
    );
  }
  const baseline = requiredAt(times, 0);
  if (!Number.isFinite(baseline) || baseline <= 0) {
    throw new RangeError("Benchmark baseline must be positive and finite");
  }
  const rows: { name: string; multiple: number }[] = [];
  for (const [index, name] of names.entries()) {
    if (index === 0) continue;
    const time = requiredAt(times, index);
    if (Number.isNaN(time)) continue;
    rows.push({ name, multiple: time / baseline });
  }
  return rows;
};
