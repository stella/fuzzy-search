import { constants } from "node:fs";
import {
  access,
  readFile,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";

const source = new URL("../index.js", import.meta.url);
const target = new URL("../index.cjs", import.meta.url);

let hasGeneratedLoader = true;
try {
  await access(source, constants.F_OK);
} catch {
  hasGeneratedLoader = false;
}

if (hasGeneratedLoader) {
  await rm(target, { force: true });
  await rename(source, target);
} else {
  await access(target, constants.F_OK);
}

const loader = await readFile(target, "utf8");
const nativeBindingAssignment =
  "nativeBinding = requireNative()";
if (!loader.includes(nativeBindingAssignment)) {
  throw new Error(
    "Generated NAPI loader no longer initializes nativeBinding",
  );
}

const causeAssignments = [
  [
    "wasiBindingError.cause = err",
    "setErrorCause(wasiBindingError, err)",
  ],
  [
    "error.cause = wasiBindingError",
    "setErrorCause(error, wasiBindingError)",
  ],
  [
    "error.cause = loadErrors.reduce((err, cur) => {",
    "setErrorCause(error, loadErrors.reduce((err, cur) => {",
  ],
  ["cur.cause = err", "setErrorCause(cur, err)"],
  ["    })\n    throw error", "    }))\n    throw error"],
];

if (loader.includes("const setErrorCause =")) {
  for (const [assignment] of causeAssignments) {
    if (loader.includes(assignment)) {
      throw new Error(
        `Patched NAPI loader still contains: ${assignment}`,
      );
    }
  }
  process.exit(0);
}

const withCauseHelper = loader.replace(
  nativeBindingAssignment,
  `${nativeBindingAssignment}

const setErrorCause = (error, cause) => {
  Object.defineProperty(error, 'cause', {
    configurable: true,
    value: cause,
    writable: true,
  })
}`,
);

let patchedLoader = withCauseHelper;
for (const [assignment, replacement] of causeAssignments) {
  if (!patchedLoader.includes(assignment)) {
    throw new Error(
      `Generated NAPI loader no longer contains: ${assignment}`,
    );
  }
  patchedLoader = patchedLoader.replace(
    assignment,
    replacement,
  );
}

await writeFile(target, patchedLoader);
