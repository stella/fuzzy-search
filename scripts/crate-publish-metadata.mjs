import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";

const CRATE_NAME = "stella-fuzzy-search-core";

const metadata = JSON.parse(
  execFileSync(
    "cargo",
    ["metadata", "--format-version", "1", "--no-deps"],
    {
      encoding: "utf8",
    },
  ),
);
const packages = metadata.packages.filter(
  ({ name }) => name === CRATE_NAME,
);
if (packages.length !== 1) {
  throw new Error(
    `Expected one ${CRATE_NAME} package, found ${packages.length}.`,
  );
}

const packageMetadata = packages.at(0);
const packageRoot = dirname(packageMetadata.manifest_path);
const packageFile = (path) =>
  path === null ? null : resolve(packageRoot, path);
const packagedFileName = (path) => {
  const file = packageFile(path);
  return file === null ? null : relative(packageRoot, file);
};
const readmeFile = packagedFileName(packageMetadata.readme);

const publishMetadata = {
  name: packageMetadata.name,
  vers: packageMetadata.version,
  deps: packageMetadata.dependencies.map((dependency) => ({
    name: dependency.name,
    version_req: dependency.req,
    features: dependency.features,
    optional: dependency.optional,
    default_features: dependency.uses_default_features,
    target: dependency.target,
    kind: dependency.kind ?? "normal",
    registry: dependency.registry,
    explicit_name_in_toml: dependency.rename,
  })),
  features: packageMetadata.features,
  authors: packageMetadata.authors,
  description: packageMetadata.description,
  documentation: packageMetadata.documentation,
  homepage: packageMetadata.homepage,
  readme:
    packageMetadata.readme === null
      ? null
      : readFileSync(
          packageFile(packageMetadata.readme),
          "utf8",
        ),
  readme_file: readmeFile,
  keywords: packageMetadata.keywords,
  categories: packageMetadata.categories,
  license: packageMetadata.license,
  license_file: packagedFileName(
    packageMetadata.license_file,
  ),
  repository: packageMetadata.repository,
  badges: {},
  links: packageMetadata.links,
  rust_version: packageMetadata.rust_version,
};

process.stdout.write(
  `${JSON.stringify(publishMetadata)}\n`,
);
