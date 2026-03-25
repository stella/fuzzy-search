/* Browser/WASM entry point — loads the binding from
 * the wasm32-wasi sub-package and re-exports the
 * public API through the shared core. */

import native from "@stll/fuzzy-search-wasm32-wasi";
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
