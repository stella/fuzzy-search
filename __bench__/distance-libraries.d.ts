declare module "damerau-levenshtein" {
  type DistanceResult = {
    similarity: number;
    steps: number;
  };

  const distance: (left: string, right: string) => DistanceResult;
  export default distance;
}

declare module "js-levenshtein" {
  const distance: (left: string, right: string) => number;
  export default distance;
}
