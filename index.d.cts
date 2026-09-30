import type { NativeBinding } from "./src/core";

// The generated NAPI-RS CommonJS loader exports the binding itself.
declare const native: NativeBinding;
export = native;
