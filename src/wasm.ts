/* Browser/WASM entry point — loads the NAPI-RS
 * browser WASM binding and re-exports the
 * public API through the shared core. */

// SAFETY: NAPI-RS auto-generated browser WASM loader
// exports the native module; cast to NativeBinding
// for initBinding.
import native from "../fuzzy-search.wasi-browser.js";
import { initBinding, type NativeBinding } from "./core";

initBinding(native as unknown as NativeBinding);

export { FuzzySearch, distance } from "./core";

export type {
  FuzzyMatch,
  Metric,
  NativeBinding,
  Options,
  PatternEntry,
} from "./core";
