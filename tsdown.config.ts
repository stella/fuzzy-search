import { defineConfig } from "tsdown";

export default defineConfig([
  {
    entry: ["src/index.ts"],
    outDir: "dist",
    format: ["esm"],
    dts: { resolve: true, autoAddExts: true },
    clean: true,
    sourcemap: true,
    hash: false,
    external: [/index\.js/],
  },
  {
    entry: ["src/wasm.ts"],
    outDir: "wasm/dist",
    format: ["esm"],
    dts: { resolve: true, autoAddExts: true },
    clean: true,
    sourcemap: true,
    hash: false,
    external: [/fuzzy-search\.wasi/],
  },
]);
