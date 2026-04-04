import { defineConfig } from "tsdown";

export default defineConfig([
  {
    entry: ["src/index.ts"],
    outDir: "dist",
    format: ["esm"],
    dts: true,
    clean: true,
    sourcemap: true,
    hash: false,
    deps: { neverBundle: [/index\.js/] },
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
    copy: [
      {
        from: "fuzzy-search.wasm32-wasi.wasm",
        to: "wasm/dist",
      },
    ],
    plugins: [
      wasmFetchGuardPlugin("@stll/fuzzy-search-wasm"),
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

/* Rolldown transform plugin: wraps the napi-rs-generated `await fetch(url)`
 * call in the wasi-browser loader with a WebAssembly magic-bytes check.
 * When a bundler (Vite dep pre-bundler, webpack dev server) rewrites
 * import.meta.url, the .wasm URL resolves to an HTML SPA fallback instead
 * of the wasm binary; this guard surfaces a helpful error message instead
 * of the cryptic `expected magic word 00 61 73 6d` from the WebAssembly
 * compiler. */
function wasmFetchGuardPlugin(packageName: string) {
  return {
    name: "stll-wasm-fetch-guard",
    transform(code: string, id: string) {
      if (!id.endsWith(".wasi-browser.js")) return null;
      const needle =
        "await fetch(__wasmUrl).then((res) => res.arrayBuffer())";
      if (!code.includes(needle)) return null;
      const replacement = `await fetch(__wasmUrl).then(async (res) => {
  const bytes = await res.arrayBuffer()
  const view = new Uint8Array(bytes)
  if (view.length < 4 || view[0] !== 0x00 || view[1] !== 0x61 || view[2] !== 0x73 || view[3] !== 0x6d) {
    throw new Error(
      ${JSON.stringify(
        `${packageName} failed to load its .wasm binary. The response did not contain WebAssembly bytes, which commonly happens when a bundler (Vite, webpack dev server, etc.) rewrites import.meta.url during pre-bundling.\n\nIf you are using Vite, import the bundled plugin:\n  import stllWasm from "${packageName}/vite"\n  // ...\n  plugins: [stllWasm()]`,
      )}
    )
  }
  return bytes
})`;
      return code.replace(needle, replacement);
    },
  };
}
