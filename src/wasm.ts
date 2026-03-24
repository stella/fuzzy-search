import { createRequire } from "node:module";

import { createApi } from "./core";
import type { NativeBinding } from "./core";

const require = createRequire(import.meta.url);
// SAFETY: NAPI-RS WASI loader returns the same native binding
// shape as the native loader.
const native = require(
  "../fuzzy-search.wasi.cjs",
) as NativeBinding;

const { FuzzySearch, distance } = createApi(native);

export { FuzzySearch, distance };
export type {
  Metric,
  Options,
  PatternEntry,
  FuzzyMatch,
} from "./core";
