/* Vite plugin that wires up @stll/fuzzy-search-wasm so its napi-rs-generated
 * wasm loader survives Vite's dep pre-bundler. Excludes the browser entry
 * and sibling wasm package from pre-bundling and SSR externalization so
 * relative asset URLs keep working. */
import type { Plugin, UserConfig } from "vite";

export const WASM_VITE_PACKAGES = [
  "@stll/fuzzy-search-wasm",
  "@stll/fuzzy-search-wasm32-wasi",
] as const;

function mergeStrings(
  existing: string[] | undefined,
  additions: readonly string[],
): string[] {
  return [...new Set([...(existing ?? []), ...additions])];
}

export function buildFuzzySearchWasmViteConfig(
  config: UserConfig = {},
): UserConfig {
  return {
    optimizeDeps: {
      ...config.optimizeDeps,
      exclude: mergeStrings(
        config.optimizeDeps?.exclude,
        WASM_VITE_PACKAGES,
      ),
    },
    ssr: {
      ...config.ssr,
      external:
        config.ssr?.external === true
          ? true
          : mergeStrings(
              config.ssr?.external,
              WASM_VITE_PACKAGES,
            ),
    },
  };
}

export default function stllFuzzySearchWasmVite(): Plugin {
  return {
    name: "stll-fuzzy-search-wasm",
    config(config) {
      return buildFuzzySearchWasmViteConfig(config);
    },
  };
}
