const directiveComment =
  /^\s*(?:\/\/\s*(?:@ts-(?:ignore|expect-error|nocheck)|(?:eslint|oxlint)-disable(?:-next-line)?\b|prettier-ignore\b).*|\/\*\s*(?:(?:eslint|oxlint)-disable\b|prettier-ignore\b).*?\*\/)\s*\r?\n/gmu;

const nativeBindingAssignment = "nativeBinding = requireNative()";
const loadErrorsDeclaration = "const loadErrors = []";
const causeHelper = `const setErrorCause = (error, cause) => {
  Object.defineProperty(error, 'cause', {
    configurable: true,
    value: cause,
    writable: true,
  })
}`;

const causeAssignments = new Map([
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
]);

/** @param {string} source */
export const stripDirectiveComments = (source) =>
  source.replaceAll(directiveComment, "");

/** @param {string} source */
export const transformNapiLoader = (source) => {
  const loader = stripDirectiveComments(source);
  if (!loader.includes(nativeBindingAssignment)) {
    throw new Error(
      "Generated NAPI loader no longer initializes nativeBinding",
    );
  }
  if (!loader.includes(loadErrorsDeclaration)) {
    throw new Error("Generated NAPI loader no longer tracks load errors");
  }

  if (loader.includes("const setErrorCause =")) {
    for (const assignment of causeAssignments.keys()) {
      if (loader.includes(assignment)) {
        throw new Error(
          `Patched NAPI loader still contains: ${assignment}`,
        );
      }
    }
    return loader;
  }

  let patchedLoader = loader.replace(
    loadErrorsDeclaration,
    `${loadErrorsDeclaration}\n\n${causeHelper}`,
  );
  for (const [assignment, replacement] of causeAssignments) {
    if (!patchedLoader.includes(assignment)) {
      throw new Error(
        `Generated NAPI loader no longer contains: ${assignment}`,
      );
    }
    patchedLoader = patchedLoader.replace(assignment, replacement);
  }
  return patchedLoader;
};
