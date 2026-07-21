/**
 * @file Builds and stages the current-platform Rust `mant` executable.
 *
 * Cargo owns compilation and dependency tracking. This wrapper applies ManT's
 * Linux/macOS C-compiler policy and atomically publishes the release artifact
 * under native/bin so development can select it through MANT_PATH.
 */

import { chmod, copyFile, mkdir, rename, rm } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { assertSupportedBuildPlatform, resolveCCompiler } from "./c-compiler";

const root = new URL("..", import.meta.url).pathname;
const manifest = join(root, "native", "Cargo.toml");
const cargoArtifact = join(root, "native", "target", "release", "mant");
const stagedArtifact = join(root, "native", "bin", "mant");

/** Build the native CLI and return the path consumed by the Bun process client. */
export async function buildMant(): Promise<string> {
  assertSupportedBuildPlatform();
  const compiler = resolveCCompiler();
  const origin = compiler.source === "environment"
    ? "CC environment variable"
    : `${process.platform} default`;
  console.log(`C compiler: ${compiler.path} (${origin})`);

  const command = [
    "cargo",
    "build",
    "--locked",
    "--release",
    "--manifest-path",
    manifest,
    "--package",
    "mant",
  ];
  console.log(`$ ${command.join(" ")}`);
  const child = Bun.spawn(command, {
    cwd: root,
    env: { ...process.env, CC: compiler.path },
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await child.exited;
  if (exitCode !== 0) {
    throw new Error(`cargo failed to build mant (exit ${exitCode})`);
  }

  await stageExecutable(cargoArtifact, stagedArtifact);
  console.log(`staged ${basename(stagedArtifact)} at ${stagedArtifact}`);
  return stagedArtifact;
}

async function stageExecutable(source: string, target: string): Promise<void> {
  await mkdir(dirname(target), { recursive: true });
  const staging = `${target}.${process.pid}.tmp`;
  try {
    await copyFile(source, staging);
    await chmod(staging, 0o755);
    await rename(staging, target);
  } finally {
    await rm(staging, { force: true });
  }
}

if (import.meta.main) {
  try {
    await buildMant();
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    console.error(`mant build failed: ${detail}`);
    process.exitCode = 1;
  }
}
