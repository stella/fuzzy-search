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
const errorChainDeclaration =
  "function createLoadErrorChain(errors) {";
if (!loader.includes(errorChainDeclaration)) {
  throw new Error(
    "Generated NAPI loader no longer declares createLoadErrorChain",
  );
}

// Any remaining `.cause =` would leave an enumerable cause on a loader error.
const causeAssignmentPattern = /\.cause\s*=(?!=)/;

const causeAssignments = [
  [
    "    error.cause = previous",
    "    setErrorCause(error, previous)",
  ],
  [
    "    error.cause = createLoadErrorChain(wasiBindingErrors)",
    "    setErrorCause(error, createLoadErrorChain(wasiBindingErrors))",
  ],
  [
    "    error.cause = createLoadErrorChain(loadErrors)",
    "    setErrorCause(error, createLoadErrorChain(loadErrors))",
  ],
];

if (loader.includes("const setErrorCause =")) {
  if (causeAssignmentPattern.test(loader)) {
    throw new Error(
      "Patched NAPI loader still assigns an error cause",
    );
  }
  process.exit(0);
}

const withCauseHelper = loader.replace(
  errorChainDeclaration,
  `const setErrorCause = (error, cause) => {
  Object.defineProperty(error, 'cause', {
    configurable: true,
    value: cause,
    writable: true,
  })
}

${errorChainDeclaration}`,
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

if (causeAssignmentPattern.test(patchedLoader)) {
  throw new Error(
    "Generated NAPI loader assigns an error cause this script does not patch",
  );
}

await writeFile(target, patchedLoader);
