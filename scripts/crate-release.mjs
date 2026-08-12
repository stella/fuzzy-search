import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  appendFileSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const CRATE_NAME = "stella-fuzzy-search-core";
const ROOT_PACKAGE_NAME = "stella-fuzzy-search";
const USER_AGENT =
  "stella-fuzzy-search-release (https://github.com/stella/fuzzy-search)";

const fail = (message) => {
  throw new Error(message);
};

const metadata = () =>
  JSON.parse(
    execFileSync(
      "cargo",
      ["metadata", "--format-version", "1", "--no-deps"],
      { encoding: "utf8" },
    ),
  );

const packageCrate = () => {
  const cargoMetadata = metadata();
  const rootPackage = cargoMetadata.packages.find(
    ({ name }) => name === ROOT_PACKAGE_NAME,
  );
  const corePackage = cargoMetadata.packages.find(
    ({ name }) => name === CRATE_NAME,
  );
  if (!rootPackage || !corePackage) {
    fail("Expected the NAPI root and core Cargo packages");
  }
  if (rootPackage.publish?.length !== 0) {
    fail(`${ROOT_PACKAGE_NAME} must remain private`);
  }
  if (
    corePackage.publish?.length !== 1 ||
    corePackage.publish.at(0) !== "crates-io"
  ) {
    fail(`${CRATE_NAME} must publish only to crates.io`);
  }

  const releaseVersion = readFileSync("VERSION", "utf8").trim();
  if (
    rootPackage.version !== releaseVersion ||
    corePackage.version !== releaseVersion
  ) {
    fail(
      `VERSION ${releaseVersion} must match both Cargo packages`,
    );
  }

  execFileSync(
    "cargo",
    ["package", "--locked", "--package", CRATE_NAME],
    { stdio: "inherit" },
  );

  const packageRoot = resolve(
    cargoMetadata.target_directory,
    "package",
  );
  return {
    crateFile: join(
      packageRoot,
      `${CRATE_NAME}-${releaseVersion}.crate`,
    ),
    expandedDirectory: join(
      packageRoot,
      `${CRATE_NAME}-${releaseVersion}`,
    ),
    releaseVersion,
  };
};

const apiUrl = (version) =>
  `https://crates.io/api/v1/crates/${CRATE_NAME}/${version}`;

const request = (url) =>
  fetch(url, { headers: { "user-agent": USER_AGENT } });

const publishedVersion = async (version) => {
  const response = await request(apiUrl(version));
  if (response.status === 404) {
    return null;
  }
  if (!response.ok) {
    fail(
      `crates.io version lookup failed: ${response.status} ${response.statusText}`,
    );
  }
  return (await response.json()).version;
};

const sha256 = (contents) =>
  createHash("sha256").update(contents).digest("hex");

const verifyPublishedContents = async ({
  crateFile,
  expandedDirectory,
  releaseVersion,
}) => {
  const registryVersion = await publishedVersion(releaseVersion);
  if (!registryVersion) {
    return false;
  }

  const downloadUrl = `${apiUrl(releaseVersion)}/download`;
  const response = await request(downloadUrl);
  if (!response.ok) {
    fail(
      `crates.io download failed: ${response.status} ${response.statusText}`,
    );
  }
  const archive = Buffer.from(await response.arrayBuffer());
  const archiveChecksum = sha256(archive);
  if (archiveChecksum !== registryVersion.checksum) {
    fail(
      `Downloaded crate checksum ${archiveChecksum} does not match crates.io ${registryVersion.checksum}`,
    );
  }

  const temporaryDirectory = mkdtempSync(
    join(tmpdir(), "stella-fuzzy-search-crate-"),
  );
  try {
    const archivePath = join(temporaryDirectory, "published.crate");
    writeFileSync(archivePath, archive);
    execFileSync(
      "tar",
      ["-xzf", archivePath, "-C", temporaryDirectory],
      { stdio: "inherit" },
    );
    const publishedDirectory = join(
      temporaryDirectory,
      `${CRATE_NAME}-${releaseVersion}`,
    );
    execFileSync(
      "diff",
      [
        "-ru",
        "--exclude=.cargo_vcs_info.json",
        expandedDirectory,
        publishedDirectory,
      ],
      { stdio: "inherit" },
    );
  } finally {
    rmSync(temporaryDirectory, { force: true, recursive: true });
  }

  if (!readFileSync(crateFile).length) {
    fail("Local Cargo package archive is empty");
  }
  return true;
};

const writeOutputs = ({ crateFile, alreadyReleased }) => {
  const outputPath = process.env.GITHUB_OUTPUT;
  if (!outputPath) {
    return;
  }
  appendFileSync(
    outputPath,
    `already-released=${alreadyReleased}\ncrate-file=${crateFile}\n`,
  );
};

const wait = (milliseconds) =>
  new Promise((resolvePromise) =>
    setTimeout(resolvePromise, milliseconds),
  );

const main = async () => {
  const command = process.argv.at(2);
  if (command !== "preflight" && command !== "verify") {
    fail("Usage: node scripts/crate-release.mjs <preflight|verify>");
  }

  const packageDetails = packageCrate();
  if (command === "preflight") {
    const alreadyReleased = await verifyPublishedContents(
      packageDetails,
    );
    writeOutputs({
      crateFile: packageDetails.crateFile,
      alreadyReleased,
    });
    console.log(
      `${CRATE_NAME}@${packageDetails.releaseVersion} already released: ${alreadyReleased}`,
    );
    return;
  }

  for (let attempt = 1; attempt <= 20; attempt += 1) {
    if (await verifyPublishedContents(packageDetails)) {
      console.log(
        `Verified ${CRATE_NAME}@${packageDetails.releaseVersion} on crates.io`,
      );
      return;
    }
    if (attempt < 20) {
      await wait(3_000);
    }
  }
  fail(
    `${CRATE_NAME}@${packageDetails.releaseVersion} did not become available on crates.io`,
  );
};

await main();
