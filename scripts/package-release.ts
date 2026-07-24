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
const mantuiManifest = join(root, "apps", "mantui", "package.json");
const cargoManifest = join(root, "engine", "Cargo.toml");

const RELEASE_TAG_PATTERN = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/;

/** Markdown manuals shipped beside the executables for immediate self-hosting. */
export const RELEASE_MANUALS = [
  { source: "docs/manuals/mant.md", destination: "mant.md" },
  { source: "docs/manuals/mantui.md", destination: "mantui.md" },
] as const;

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
  if (!version) throw new Error("engine/Cargo.toml has no workspace package version");
  return version;
}

/**
 * Validate every independently versioned workspace boundary.
 *
 * The root manifest names the distribution, the mantui manifest names the Bun
 * workspace, and Cargo supplies the version inherited by native crates. A
 * release is coherent only when all three advance together.
 */
export function releaseVersionFromManifests(
  rootManifest: string,
  tuiManifest: string,
  nativeManifest: string,
): string {
  const rootVersion = packageVersion(rootManifest, "package.json");
  const tuiVersion = packageVersion(
    tuiManifest,
    "apps/mantui/package.json",
  );
  const nativeVersion = workspaceVersion(nativeManifest);

  if (rootVersion !== tuiVersion || rootVersion !== nativeVersion) {
    throw new Error(
      "version mismatch: "
      + `package.json=${rootVersion}, `
      + `apps/mantui/package.json=${tuiVersion}, `
      + `engine/Cargo.toml=${nativeVersion}`,
    );
  }
  return rootVersion;
}

function packageVersion(manifest: string, label: string): string {
  const parsed = JSON.parse(manifest) as { version?: unknown };
  if (typeof parsed.version !== "string" || parsed.version.length === 0) {
    throw new Error(`${label} has no release version`);
  }
  return parsed.version;
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

/**
 * Archive the staging tree reproducibly.
 *
 * A published archive should rebuild to the same bytes from the same commit so
 * its checksum can be independently verified. Plain `tar -czf` embeds per-file
 * mtime/uid/gid and gzip embeds a timestamp, so two builds diverge. With GNU
 * tar we pin ownership, sort entries, and zero mtimes, then compress with a
 * separate `gzip -n` (no name/timestamp). Other tar implementations (for
 * example BSD tar on macOS) fall back to the previous behavior.
 */
export async function runTar(stagingDirectory: string, archiveRoot: string, archive: string) {
  if (await isGnuTar()) {
    await runDeterministicGnuTar(stagingDirectory, archiveRoot, archive);
    return;
  }
  await runCommand(
    ["tar", "-czf", archive, "-C", stagingDirectory, archiveRoot],
    // Prevent macOS tar from adding AppleDouble metadata files.
    { COPYFILE_DISABLE: "1" },
  );
}

async function isGnuTar(): Promise<boolean> {
  const child = Bun.spawn(["tar", "--version"], {
    stdin: "ignore",
    stdout: "pipe",
    stderr: "ignore",
  });
  const version = await new Response(child.stdout).text();
  await child.exited;
  return version.includes("GNU tar");
}

async function runDeterministicGnuTar(
  stagingDirectory: string,
  archiveRoot: string,
  archive: string,
) {
  // Produce the tar and gzip streams separately so gzip can drop its own
  // name/timestamp header; tar itself pins every source of nondeterminism.
  await runCommand(
    [
      "tar",
      "--sort=name",
      "--mtime=@0",
      "--owner=0",
      "--group=0",
      "--numeric-owner",
      "-cf",
      archive,
      "-C",
      stagingDirectory,
      archiveRoot,
    ],
    { COPYFILE_DISABLE: "1" },
  );
  await runCommand(["gzip", "-n", "-f", archive]);
  // `gzip -f archive` writes `archive.gz`; restore the requested name.
  const { rename } = await import("node:fs/promises");
  await rename(`${archive}.gz`, archive);
}

async function runCommand(command: string[], extraEnv: Record<string, string> = {}) {
  const child = Bun.spawn(command, {
    cwd: root,
    env: { ...process.env, ...extraEnv },
    stdin: "ignore",
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await child.exited;
  if (exitCode !== 0) {
    throw new Error(`${command[0]} failed with exit code ${exitCode}`);
  }
}

/** Package the binaries produced by the current host's canonical build. */
export async function packageRelease(
  releaseTag: string | undefined = process.env.MANT_RELEASE_TAG,
  expectedTarget: string | undefined = process.env.MANT_RELEASE_TARGET,
): Promise<string> {
  const [rootPackage, tuiPackage, cargoWorkspace] = await Promise.all([
    readFile(packageManifest, "utf8"),
    readFile(mantuiManifest, "utf8"),
    readFile(cargoManifest, "utf8"),
  ]);
  const releaseVersion = releaseVersionFromManifests(
    rootPackage,
    tuiPackage,
    cargoWorkspace,
  );
  if (releaseTag && versionFromReleaseTag(releaseTag) !== releaseVersion) {
    throw new Error(
      `release tag ${releaseTag} does not match package version ${releaseVersion}`,
    );
  }

  const { archiveTarget } = resolveReleasePlatform();
  if (expectedTarget && expectedTarget !== archiveTarget) {
    throw new Error(
      `release runner target mismatch: expected ${expectedTarget}, built ${archiveTarget}`,
    );
  }
  const archiveRoot = `mant-${releaseVersion}-${archiveTarget}`;
  const stagingDirectory = join(distDirectory, ".release-staging");
  const packageDirectory = join(stagingDirectory, archiveRoot);
  const licenseDirectory = join(packageDirectory, "LICENSES");
  const archive = join(distDirectory, `${archiveRoot}.tar.gz`);

  await rm(stagingDirectory, { recursive: true, force: true });
  await mkdir(licenseDirectory, { recursive: true });
  try {
    await copyExecutable(join(distDirectory, "mantui"), join(packageDirectory, "mantui"));
    await copyExecutable(join(distDirectory, "mant"), join(packageDirectory, "mant"));
    for (const manual of RELEASE_MANUALS) {
      await copyFile(join(root, manual.source), join(packageDirectory, manual.destination));
    }
    await copyFile(join(root, "README.md"), join(packageDirectory, "README.md"));
    await copyFile(join(root, "LICENSE"), join(packageDirectory, "LICENSE"));
    await copyFile(
      join(root, "engine", "crates", "libmandoc-rs", "vendor", "mandoc-1.14.6", "LICENSE"),
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
