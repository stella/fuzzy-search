/* Vite plugin that wires up @stll/fuzzy-search-wasm so its napi-rs-generated
 * wasm loader survives Vite's dep pre-bundler. Adds the package to
 * optimizeDeps.exclude so the loader module is served with its original
 * path, keeping `new URL("./foo.wasm", import.meta.url)` correct. */
import type { Plugin } from "vite";

const PACKAGE_NAME = "@stll/fuzzy-search-wasm";

export default function stllFuzzySearchWasmVite(): Plugin {
  return {
    name: "stll-fuzzy-search-wasm",
    config() {
      return {
        optimizeDeps: { exclude: [PACKAGE_NAME] },
      };
    },
  };
}
