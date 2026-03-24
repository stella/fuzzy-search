import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["src/lib.ts"],
  format: ["esm", "cjs"],
  dts: true,
  clean: true,
  sourcemap: true,
  hash: false,
  external: [/\.\/index\.js/, /\.\.\/index\.js/],
});
