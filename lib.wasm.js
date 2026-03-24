// @ts-nocheck
/* WASM-only wrapper. Loads the WASI module instead
 * of the native .node binary. Same API as lib.mjs. */
import native from "./fuzzy-search.wasi-browser.js";

const NativeFuzzySearch = native.FuzzySearch;
const nativeDistance = native.distance;

function resolveDistance(dist, patternLength) {
  if (dist !== "auto") return dist;
  if (patternLength <= 2) return 0;
  if (patternLength <= 5) return 1;
  return 2;
}

function normalizeEntry(p, i) {
  if (typeof p === "string") {
    return { pattern: p };
  }
  if (
    typeof p === "object" &&
    p !== null &&
    typeof p.pattern === "string"
  ) {
    if (p.distance === "auto") {
      return {
        ...p,
        distance: resolveDistance("auto", p.pattern.length),
      };
    }
    return p;
  }
  throw new TypeError(
    `Pattern at index ${i} must be a string ` +
      `or { pattern, distance?, name? }`,
  );
}

function unpack(packed, haystack, names) {
  const len = packed.length;
  // eslint-disable-next-line unicorn/no-new-array
  const matches = new Array(len / 4);
  for (let i = 0, j = 0; i < len; i += 4, j++) {
    const idx = packed[i];
    const start = packed[i + 1];
    const end = packed[i + 2];
    const m = {
      pattern: idx,
      start,
      end,
      text: haystack.slice(start, end),
      distance: packed[i + 3],
    };
    if (names[idx] !== undefined) {
      m.name = names[idx];
    }
    matches[j] = m;
  }
  return matches;
}

class FuzzySearch {
  constructor(patterns, options) {
    const entries = patterns.map(normalizeEntry);
    this._names = entries.map((e) => e.name);
    this._inner = new NativeFuzzySearch(entries, options);
  }

  get patternCount() {
    return this._inner.patternCount;
  }

  isMatch(haystack) {
    return this._inner.isMatch(haystack);
  }

  findIter(haystack) {
    return unpack(
      this._inner._findIterPacked(haystack),
      haystack,
      this._names,
    );
  }

  replaceAll(haystack, replacements) {
    return this._inner.replaceAll(haystack, replacements);
  }
}

function distance(a, b, metric) {
  return nativeDistance(a, b, metric ?? null);
}

export { FuzzySearch, distance };
