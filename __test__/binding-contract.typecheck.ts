import type { NativeBinding } from "../src/core";

// A generated declaration change must satisfy the shared wrapper contract
// directly, as well as through each loader declaration.
type RequireNativeBinding<Binding extends NativeBinding> = Binding;

export type BackendContracts = {
  generated: RequireNativeBinding<typeof import("../index.js")>;
  nativeLoader: RequireNativeBinding<typeof import("../index.cjs")>;
  wasiNamedExports: RequireNativeBinding<
    typeof import("../fuzzy-search.wasi-browser.js")
  >;
  wasiLoader: RequireNativeBinding<
    (typeof import("../fuzzy-search.wasi-browser.js"))["default"]
  >;
};
