import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["src/index.ts", "src/wasm.ts"],
  format: ["esm"],
  dts: true,
  clean: true,
  sourcemap: true,
  hash: false,
  external: [
    /\.\.\/index\.js/,
    /\.\.\/fuzzy-search\.wasi\.cjs/,
  ],
});
