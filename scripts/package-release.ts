/**
 * @file Packages the current host's tested binaries into a release archive.
 *
 * This script deliberately does not build anything. `bun run build` remains
 * the verification boundary; packaging only validates version/platform
 * identity, copies distributable files, and records an archive checksum.
 */

import { createHash } from "node:crypto";
import {
  access,
  chmod,
  copyFile,
  mkdir,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import { basename, join } from "node:path";
import { resolveReleasePlatform } from "./release-platform";

const root = new URL("..", import.meta.url).pathname;
const distDirectory = join(root, "dist");
const packageManifest = join(root, "package.json");
const cargoManifest = join(root, "native", "Cargo.toml");

const RELEASE_TAG_PATTERN = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/;

/** Validate a release tag and return its package version. */
export function versionFromReleaseTag(tag: string): string {
  const match = RELEASE_TAG_PATTERN.exec(tag);
  if (!match) {
    throw new Error(`release tag '${tag}' must use the form vMAJOR.MINOR.PATCH`);
  }
  return tag.slice(1);
}

/** Read the version inherited by every crate in the native workspace. */
export function workspaceVersion(manifest: string): string {
  const workspacePackage = manifest.match(
    /\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/,
  )?.[1];
  const version = workspacePackage?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  if (!version) throw new Error("native/Cargo.toml has no workspace package version");
  return version;
}

async function sha256(path: string): Promise<string> {
  const hash = createHash("sha256");
  for await (const chunk of Bun.file(path).stream()) hash.update(chunk);
  return hash.digest("hex");
}

async function copyExecutable(source: string, destination: string): Promise<void> {
  await access(source);
  await copyFile(source, destination);
  await chmod(destination, 0o755);
}

async function runTar(stagingDirectory: string, archiveRoot: string, archive: string) {
  const child = Bun.spawn(
    ["tar", "-czf", archive, "-C", stagingDirectory, archiveRoot],
    {
      cwd: root,
      // Prevent macOS tar from adding AppleDouble metadata files.
      env: { ...process.env, COPYFILE_DISABLE: "1" },
      stdin: "ignore",
      stdout: "inherit",
      stderr: "inherit",
    },
  );
  const exitCode = await child.exited;
  if (exitCode !== 0) throw new Error(`tar failed with exit code ${exitCode}`);
}

/** Package the binaries produced by the current host's canonical build. */
export async function packageRelease(
  releaseTag: string | undefined = process.env.MANT_RELEASE_TAG,
  expectedTarget: string | undefined = process.env.MANT_RELEASE_TARGET,
): Promise<string> {
  const packageJson = JSON.parse(await readFile(packageManifest, "utf8")) as {
    version?: unknown;
  };
  if (typeof packageJson.version !== "string" || packageJson.version.length === 0) {
    throw new Error("package.json has no release version");
  }

  const cargoVersion = workspaceVersion(await readFile(cargoManifest, "utf8"));
  if (cargoVersion !== packageJson.version) {
    throw new Error(
      `version mismatch: package.json=${packageJson.version}, native/Cargo.toml=${cargoVersion}`,
    );
  }
  if (releaseTag && versionFromReleaseTag(releaseTag) !== packageJson.version) {
    throw new Error(
      `release tag ${releaseTag} does not match package version ${packageJson.version}`,
    );
  }

  const { archiveTarget } = resolveReleasePlatform();
  if (expectedTarget && expectedTarget !== archiveTarget) {
    throw new Error(
      `release runner target mismatch: expected ${expectedTarget}, built ${archiveTarget}`,
    );
  }
  const archiveRoot = `mant-${packageJson.version}-${archiveTarget}`;
  const stagingDirectory = join(distDirectory, ".release-staging");
  const packageDirectory = join(stagingDirectory, archiveRoot);
  const licenseDirectory = join(packageDirectory, "LICENSES");
  const archive = join(distDirectory, `${archiveRoot}.tar.gz`);

  await rm(stagingDirectory, { recursive: true, force: true });
  await mkdir(licenseDirectory, { recursive: true });
  try {
    await copyExecutable(join(distDirectory, "mant"), join(packageDirectory, "mant"));
    await copyExecutable(join(distDirectory, "mant-cli"), join(packageDirectory, "mant-cli"));
    await copyFile(join(root, "README.md"), join(packageDirectory, "README.md"));
    await copyFile(join(root, "LICENSE"), join(packageDirectory, "LICENSE"));
    await copyFile(
      join(root, "native", "crates", "libmandoc-rs", "vendor", "mandoc-1.14.6", "LICENSE"),
      join(licenseDirectory, "mandoc.txt"),
    );
    await runTar(stagingDirectory, archiveRoot, archive);
  } finally {
    await rm(stagingDirectory, { recursive: true, force: true });
  }

  const checksum = `${await sha256(archive)}  ${basename(archive)}\n`;
  await writeFile(`${archive}.sha256`, checksum);
  console.log(`packaged ${archive}`);
  console.log(checksum.trim());
  return archive;
}

if (import.meta.main) {
  try {
    await packageRelease();
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    console.error(`release packaging failed: ${detail}`);
    process.exitCode = 1;
  }
}
