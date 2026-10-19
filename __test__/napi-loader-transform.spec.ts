import { describe, expect, test } from "bun:test";
import {
  stripDirectiveComments,
  transformNapiLoader,
} from "../scripts/napi-loader-transform.mjs";

const generatedLoader = `// prettier-ignore
/* eslint-disable */
// @ts-nocheck
const loadErrors = []
function createLoadErrorChain(errors) {
  return errors.reduce((previous, current) => {
    const error = new Error(current.message)
    error.cause = previous
    return error
  }, null)
}
nativeBinding = requireNative()
error.cause = createLoadErrorChain(wasiBindingErrors)
error.cause = createLoadErrorChain(loadErrors)
`;

describe("napi loader transform", () => {
  test("preserves each generated error cause without direct assignment", () => {
    const transformed = transformNapiLoader(generatedLoader);

    expect(transformed).toContain(
      "setErrorCause(error, previous)",
    );
    expect(transformed).toContain(
      "setErrorCause(error, createLoadErrorChain(wasiBindingErrors))",
    );
    expect(transformed).toContain(
      "setErrorCause(error, createLoadErrorChain(loadErrors))",
    );
  });

  test("strips generated lint, format, and TypeScript directives", () => {
    expect(stripDirectiveComments(generatedLoader)).not.toMatch(
      /(?:eslint|oxlint)-disable|@ts-(?:ignore|expect-error|nocheck)|prettier-ignore/u,
    );
  });
});
