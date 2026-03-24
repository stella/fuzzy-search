/* Browser/WASM entry point — loads the NAPI-RS
 * browser WASM binding and re-exports the
 * public API through the shared core. */

// SAFETY: NAPI-RS auto-generated browser WASM loader
// exports the native module; cast to NativeBinding
// for the createApi factory.
import native from "../fuzzy-search.wasi-browser.js";

import {
  createApi,
  type NativeBinding,
} from "./core";

const { FuzzySearch, distance } =
  createApi(native as unknown as NativeBinding);

export { FuzzySearch, distance };

export type {
  FuzzyMatch,
  Metric,
  NativeBinding,
  Options,
  PatternEntry,
} from "./core";
