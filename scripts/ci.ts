/**
 * @file Runs ManT's local cross-platform build and test verification sequence.
 */

import {
  access,
  chmod,
  constants,
  copyFile,
  mkdir,
} from "node:fs/promises";
import { dirname, join } from "node:path";
import { assertSupportedBuildPlatform } from "./c-compiler";
import { resolveReleasePlatform } from "./release-platform";

const root = new URL("..", import.meta.url).pathname;
const distDirectory = join(root, "dist");
const mantName = "mant";
const mantSource = join(root, "engine", "bin", mantName);
const mantPath = join(distDirectory, mantName);
const executableName = "mantui";
const executablePath = join(distDirectory, executableName);
const executableEntrypoint = join(root, "apps", "mantui", "src", "mantui.ts");

async function isExecutable(path: string): Promise<boolean> {
  try {
    await access(path, constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

async function run(
  label: string,
  command: string[],
  environment: Record<string, string | undefined> = {},
): Promise<void> {
  console.log(`\n==> ${label}`);
  console.log(`$ ${command.join(" ")}`);

  const child = Bun.spawn(command, {
    cwd: root,
    env: { ...process.env, ...environment },
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await child.exited;
  if (exitCode !== 0) throw new Error(`${label} failed with exit code ${exitCode}`);
}

async function verifyPackagedExecutable(): Promise<void> {
  console.log("\n==> packaged executable smoke tests");
  const helpProcess = Bun.spawn([executablePath, "--help"], {
    cwd: distDirectory,
    stdout: "pipe",
    stderr: "pipe",
  });
  const [helpOutput, helpStderr, helpExitCode] = await Promise.all([
    new Response(helpProcess.stdout).text(),
    new Response(helpProcess.stderr).text(),
    helpProcess.exited,
  ]);
  if (helpExitCode !== 0 || !helpOutput.includes("Usage:\n  mantui <topic>")) {
    throw new Error(`packaged TUI help smoke test failed: ${helpStderr.trim()}`);
  }

  const queryProcess = Bun.spawn(
    [mantPath, "ls", "--format", "json", "--compact"],
    {
    cwd: distDirectory,
    stdout: "pipe",
    stderr: "pipe",
    },
  );
  const [queryOutput, queryStderr, queryExitCode] = await Promise.all([
    new Response(queryProcess.stdout).text(),
    new Response(queryProcess.stderr).text(),
    queryProcess.exited,
  ]);
  if (queryExitCode !== 0) {
    throw new Error(`packaged mant smoke test failed: ${queryStderr.trim()}`);
  }
  const query = JSON.parse(queryOutput) as {
    schema?: string;
    manual?: { schema?: string; sections?: unknown[] };
  };
  if (
    query.schema !== "mant.query/v2"
    || query.manual?.schema !== "mant.document/v2"
    || !query.manual.sections?.length
  ) {
    throw new Error("packaged mant did not return a readable native document");
  }
}

async function main(): Promise<void> {
  assertSupportedBuildPlatform();
  const releasePlatform = resolveReleasePlatform();

  await run("install locked dependencies", [process.execPath, "install", "--frozen-lockfile"]);
  await run("type check", [process.execPath, "run", "lint"]);
  await run("check Rust formatting", [
    "cargo",
    "fmt",
    "--manifest-path",
    join(root, "engine", "Cargo.toml"),
    "--all",
    "--check",
  ]);
  await run("test Rust workspace", [
    "cargo",
    "test",
    "--manifest-path",
    join(root, "engine", "Cargo.toml"),
    "--workspace",
  ]);
  await run("lint Rust workspace", [
    "cargo",
    "clippy",
    "--manifest-path",
    join(root, "engine", "Cargo.toml"),
    "--workspace",
    "--all-targets",
    "--",
    "-D",
    "warnings",
  ]);
  await run("build native mant", [process.execPath, "run", "build:mant"]);
  if (!(await isExecutable(mantSource))) {
    throw new Error("build:mant did not stage an executable engine/bin/mant");
  }

  await run("test", [process.execPath, "test"]);
  await mkdir(distDirectory, { recursive: true });
  await copyFile(mantSource, mantPath);
  await chmod(mantPath, 0o755);
  await run("compile current-platform executable", [
    process.execPath,
    "build",
    "--compile",
    "--target",
    releasePlatform.bunCompileTarget,
    "--define",
    "MANT_COMPILED=true",
    "--outfile",
    executablePath,
    executableEntrypoint,
  ]);
  await verifyPackagedExecutable();

  console.log(`\nlocal CI succeeded: ${dirname(executablePath)}`);
  console.log(`  TUI:        ${executablePath}`);
  console.log(`  agent CLI:  ${mantPath}`);
}

await main();
