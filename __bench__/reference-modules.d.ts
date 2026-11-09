// Reference implementations used as benchmark and correctness oracles; the
// packages ship no type declarations.

declare module "damerau-levenshtein" {
  export default function damerauLevenshtein(
    left: string,
    right: string,
    limit?: number,
  ): { steps: number; relative: number; similarity: number };
}

declare module "js-levenshtein" {
  export default function levenshtein(left: string, right: string): number;
}
