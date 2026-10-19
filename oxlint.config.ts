import { library } from "@stll/oxlint-config";
import {
  portableSafetyPluginSpecifiers,
  portableSafetyRules,
} from "@stll/oxlint-plugin";

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
  jsPlugins: [...portableSafetyPluginSpecifiers],
  rules: { ...portableSafetyRules },
});
