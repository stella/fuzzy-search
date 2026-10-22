/** @param {unknown} value @returns {value is Record<string, unknown>} */
const isRecord = (value) =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** @param {unknown} value */
const stringRecord = (value) => {
  if (!isRecord(value))
    throw new Error("Expected string-valued dependency map");
  /** @type {Record<string, string>} */
  const result = {};
  for (const [key, entry] of Object.entries(value)) {
    if (typeof entry !== "string")
      throw new Error("Expected string-valued dependency map");
    result[key] = entry;
  }
  return result;
};

/** @param {string} source */
export const parsePackageManifest = (source) => {
  /** @type {unknown} */
  const value = JSON.parse(source);
  if (
    !isRecord(value) ||
    typeof value["name"] !== "string" ||
    typeof value["version"] !== "string"
  )
    throw new Error("Expected package name and version strings");
  const main = value["main"];
  if (main !== undefined && typeof main !== "string")
    throw new Error("Expected package main string");
  const optional = value["optionalDependencies"];
  return {
    ...value,
    name: value["name"],
    version: value["version"],
    main,
    optionalDependencies:
      optional === undefined ? undefined : stringRecord(optional),
  };
};

/** @param {unknown} value */
export const parseCargoMetadata = (value) => {
  if (
    !isRecord(value) ||
    typeof value["target_directory"] !== "string" ||
    !Array.isArray(value["packages"])
  )
    throw new Error("Expected Cargo metadata packages and target directory");
  const packages = value["packages"].map((entry) => {
    if (
      !isRecord(entry) ||
      typeof entry["name"] !== "string" ||
      typeof entry["version"] !== "string"
    )
      throw new Error("Expected Cargo package name and version strings");
    const publish = entry["publish"];
    if (
      publish !== null &&
      (!Array.isArray(publish) ||
        !publish.every((registry) => typeof registry === "string"))
    )
      throw new Error("Expected Cargo publish registry list or null");
    return {
      name: entry["name"],
      version: entry["version"],
      publish,
    };
  });
  return {
    packages,
    target_directory: value["target_directory"],
  };
};

/** @param {unknown} value */
export const parseRegistryVersion = (value) => {
  if (
    !isRecord(value) ||
    !isRecord(value["version"]) ||
    typeof value["version"]["checksum"] !== "string"
  )
    throw new Error("Expected crates.io version checksum");
  return { checksum: value["version"]["checksum"] };
};

/** @param {unknown} version */
export const formatSbomVersion = (version) => {
  if (version === undefined || version === null) return "<missing>";
  if (typeof version === "string") return version;
  return JSON.stringify(version);
};
