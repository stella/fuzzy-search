const FETCH_NEEDLE =
  "await fetch(__wasmUrl).then((res) => res.arrayBuffer())";

function buildMissingNeedleWarning(id: string): string {
  return (
    "[stll-wasm-fetch-guard] Expected fetch pattern " +
    `not found in ${id}. The magic-bytes guard was not applied; ` +
    "the napi-rs loader format may have changed."
  );
}

function buildReplacement(packageName: string): string {
  return `await fetch(__wasmUrl).then(async (res) => {
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
}

export function injectWasmFetchGuard(
  code: string,
  id: string,
  packageName: string,
  warn: (message: string) => void = console.warn,
): string | null {
  if (!id.endsWith(".wasi-browser.js")) return null;
  if (!code.includes(FETCH_NEEDLE)) {
    warn(buildMissingNeedleWarning(id));
    return null;
  }

  return code.replace(
    FETCH_NEEDLE,
    buildReplacement(packageName),
  );
}

export function wasmFetchGuardPlugin(packageName: string) {
  return {
    name: "stll-wasm-fetch-guard",
    transform(code: string, id: string) {
      return injectWasmFetchGuard(code, id, packageName);
    },
  };
}
