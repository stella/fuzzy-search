import { library } from "@stll/oxlint-config";

export default library({
  ignorePatterns: [
    "*.node",
    "npm/",
    "index.cjs",
    "index.d.ts",
    "*.wasi.cjs",
    "*.wasi-browser.js",
    "browser.js",
    "wasi-worker-browser.mjs",
    "wasi-worker.mjs",
  ],
  overrides: [
    {
      files: ["scripts/**"],
      rules: {
        "typescript/no-unnecessary-condition": "off",
        "typescript/strict-boolean-expressions": "off",
      },
    },
    {
      files: ["__bench__/**", "__test__/**"],
      rules: {
        "no-console": "off",
        "no-non-null-assertion": "off",
        "require-await": "off",
      },
    },
  ],
});
