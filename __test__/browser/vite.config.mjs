// Configured the way the README tells Vite users to: the package plugin, plus
// the cross-origin isolation headers the threaded WASM build needs.
import { defineConfig } from "vite";

import stllFuzzySearchWasm from "@stll/fuzzy-search-wasm/vite";

const isolation = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "require-corp",
};

export default defineConfig({
  plugins: [stllFuzzySearchWasm()],
  server: { headers: isolation },
  preview: { headers: isolation, port: 4789, strictPort: true },
});
