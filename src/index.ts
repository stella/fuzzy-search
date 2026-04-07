/* Main entry point — loads the native NAPI-RS
 * binding and re-exports the public API. */

import * as native from "../index.js";
import { initBinding, type NativeBinding } from "./core";

initBinding(native as NativeBinding);

export { FuzzySearch, distance } from "./core";

export type {
  FuzzyMatch,
  Metric,
  NativeBinding,
  Options,
  PatternEntry,
} from "./core";
