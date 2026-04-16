import { defineConfig } from "tsdown";

import { wasmFetchGuardPlugin } from "./src/wasm-fetch-guard.ts";

export default defineConfig([
  {
    entry: ["src/index.ts"],
    outDir: "dist",
    format: ["esm"],
    dts: true,
    clean: true,
    sourcemap: true,
    hash: false,
    deps: { neverBundle: [/index\.cjs/] },
  },
  {
    entry: ["src/wasm.ts"],
    outDir: "wasm/dist",
    format: ["esm"],
    dts: true,
    clean: true,
    sourcemap: true,
    hash: false,
    deps: { neverBundle: [/^@napi-rs\/wasm-runtime$/] },
    plugins: [
      wasmFetchGuardPlugin("@stll/fuzzy-search-wasm"),
    ],
    copy: [
      {
        from: "fuzzy-search.wasm32-wasi.wasm",
        to: "wasm/dist",
      },
    ],
  },
  {
    entry: ["wasi-worker-browser.mjs"],
    outDir: "wasm/dist",
    format: ["esm"],
    dts: false,
    clean: false,
    sourcemap: true,
    hash: false,
    deps: { neverBundle: [/^@napi-rs\/wasm-runtime$/] },
  },
  {
    entry: ["src/vite.ts"],
    outDir: "wasm/dist",
    format: ["esm"],
    dts: true,
    clean: false,
    sourcemap: true,
    hash: false,
    deps: { neverBundle: [/^vite$/] },
  },
]);
