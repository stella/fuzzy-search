/** Read a benchmark invariant; a missing value must fail instead of biasing results. */
export const requiredAt = <T>(
  values: readonly T[],
  index: number,
): T => {
  const value = values[index];
  if (value === undefined) {
    throw new RangeError(
      `Missing benchmark value at index ${index}`,
    );
  }
  return value;
};

