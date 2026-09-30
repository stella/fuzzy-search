/* Main entry point — loads the native NAPI-RS
 * binding and re-exports the public API. */

import native from "../index.cjs";
import { initBinding } from "./core";

initBinding(native);

export { FuzzySearch, distance } from "./core";

export type {
  FuzzyMatch,
  Metric,
  NativeBinding,
  Options,
  PatternEntry,
} from "./core";
