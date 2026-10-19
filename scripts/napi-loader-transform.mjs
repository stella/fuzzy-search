const GENERATOR_DIRECTIVES = [
  "// prettier-ignore",
  "/* eslint-disable */",
  "// @ts-nocheck",
];

const NATIVE_BINDING_ASSIGNMENT =
  "nativeBinding = requireNative()";
const LOAD_ERRORS_DECLARATION = "const loadErrors = []";

const CAUSE_ASSIGNMENTS = /** @type {const} */ ([
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

/** @param {string} loader */
const stripGeneratorDirectives = (loader) => {
  let transformed = loader;
  for (const directive of GENERATOR_DIRECTIVES) {
    transformed = transformed.replace(`${directive}\n`, "");
  }
  return transformed;
};

/** @param {string} loader */
export const transformNapiLoader = (loader) => {
  const strippedLoader = stripGeneratorDirectives(loader);
  if (!strippedLoader.includes(NATIVE_BINDING_ASSIGNMENT)) {
    throw new Error(
      "Generated NAPI loader no longer initializes nativeBinding",
    );
  }

  if (strippedLoader.includes("const setErrorCause =")) {
    for (const [assignment] of CAUSE_ASSIGNMENTS) {
      if (strippedLoader.includes(assignment)) {
        throw new Error(
          `Patched NAPI loader still contains: ${assignment}`,
        );
      }
    }
    return strippedLoader;
  }

  let transformed = strippedLoader.replace(
    LOAD_ERRORS_DECLARATION,
    `${LOAD_ERRORS_DECLARATION}

const setErrorCause = (error, cause) => {
  Object.defineProperty(error, 'cause', {
    configurable: true,
    value: cause,
    writable: true,
  })
}`,
  );

  if (transformed === strippedLoader) {
    throw new Error(
      "Generated NAPI loader no longer declares loadErrors",
    );
  }

  for (const [
    assignment,
    replacement,
  ] of CAUSE_ASSIGNMENTS) {
    if (!transformed.includes(assignment)) {
      throw new Error(
        `Generated NAPI loader no longer contains: ${assignment}`,
      );
    }
    transformed = transformed.replace(
      assignment,
      replacement,
    );
  }

  return transformed;
};
