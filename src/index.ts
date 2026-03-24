import { createRequire } from "node:module";

import { createApi } from "./core";
import type { NativeBinding } from "./core";

const require = createRequire(import.meta.url);
// SAFETY: NAPI-RS auto-generated loader returns the native binding
// object; its shape is validated by usage below.
const native = require("../index.js") as NativeBinding;

const { FuzzySearch, distance } = createApi(native);

export { FuzzySearch, distance };
export type {
  Metric,
  Options,
  PatternEntry,
  FuzzyMatch,
} from "./core";
