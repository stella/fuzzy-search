/* Browser/WASM entry point -- loads the WASM binding
 * from the generated browser glue and re-exports the
 * public API through the shared core. */

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
