import { constants } from "node:fs";
import { access, readFile, rename, rm, writeFile } from "node:fs/promises";

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
const errorListDeclaration = "const loadErrors = []";
if (!loader.includes(errorListDeclaration)) {
  throw new Error("Generated NAPI loader no longer declares loadErrors");
}

const causeAssignments = [
  ["error.cause = previous", "setErrorCause(error, previous)"],
  [
    "error.cause = createLoadErrorChain(wasiBindingErrors)",
    "setErrorCause(error, createLoadErrorChain(wasiBindingErrors))",
  ],
  [
    "error.cause = createLoadErrorChain(loadErrors)",
    "setErrorCause(error, createLoadErrorChain(loadErrors))",
  ],
];

if (loader.includes("const setErrorCause =")) {
  for (const [assignment] of causeAssignments) {
    if (loader.includes(assignment)) {
      throw new Error(`Patched NAPI loader still contains: ${assignment}`);
    }
  }
  process.exit(0);
}

const withCauseHelper = loader.replace(
  errorListDeclaration,
  `${errorListDeclaration}

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
  if (patchedLoader.split(assignment).length !== 2) {
    throw new Error(
      `Generated NAPI loader must contain exactly one: ${assignment}`,
    );
  }
  patchedLoader = patchedLoader.replace(assignment, replacement);
}

await writeFile(target, patchedLoader);
