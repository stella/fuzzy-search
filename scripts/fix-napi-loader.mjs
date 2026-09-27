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
if (!loader.includes("nativeBinding = requireNative()")) {
  throw new Error(
    "Generated NAPI loader no longer initializes nativeBinding",
  );
}

// Any `.cause = …` left in the loader creates an enumerable cause, unlike
// `new Error(message, { cause })`. Every assignment must be rewritten.
const causeAssignmentPattern = /\.cause\s*=(?!=)/;
const causeHelperAnchor =
  "function createLoadErrorChain(errors) {";
const causeHelper = `function setErrorCause(error, cause) {
  Object.defineProperty(error, 'cause', {
    configurable: true,
    value: cause,
    writable: true,
  })
}

`;
const causeAssignments = [
  [
    "error.cause = previous",
    "setErrorCause(error, previous)",
  ],
  [
    "error.cause = createLoadErrorChain(wasiBindingErrors)",
    "setErrorCause(error, createLoadErrorChain(wasiBindingErrors))",
  ],
  [
    "error.cause = createLoadErrorChain(loadErrors)",
    "setErrorCause(error, createLoadErrorChain(loadErrors))",
  ],
];

const assertNoCauseAssignment = (code) => {
  const match = causeAssignmentPattern.exec(code);
  if (match) {
    const line = code
      .slice(0, match.index)
      .split("\n").length;
    throw new Error(
      `NAPI loader still assigns an error cause directly (index.cjs:${line})`,
    );
  }
};

const countOccurrences = (code, needle) =>
  code.split(needle).length - 1;

if (loader.includes(causeHelper)) {
  assertNoCauseAssignment(loader);
  process.exit(0);
}

if (countOccurrences(loader, causeHelperAnchor) !== 1) {
  throw new Error(
    `Generated NAPI loader no longer contains exactly one: ${causeHelperAnchor}`,
  );
}
let patchedLoader = loader.replace(
  causeHelperAnchor,
  `${causeHelper}${causeHelperAnchor}`,
);

for (const [assignment, replacement] of causeAssignments) {
  if (countOccurrences(patchedLoader, assignment) !== 1) {
    throw new Error(
      `Generated NAPI loader no longer contains exactly one: ${assignment}`,
    );
  }
  patchedLoader = patchedLoader.replace(
    assignment,
    replacement,
  );
}

assertNoCauseAssignment(patchedLoader);
await writeFile(target, patchedLoader);
