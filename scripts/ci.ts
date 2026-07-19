/**
 * @file Runs Mant's local cross-platform build and test verification sequence.
 */

import {
  access,
  chmod,
  constants,
  copyFile,
  mkdir,
  rm,
  stat,
} from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { assertSupportedBuildPlatform, resolveCCompiler } from "./c-compiler";

const root = new URL("..", import.meta.url).pathname;
const distDirectory = join(root, "dist");
const sidecarName = "mant-mandoc-json";
const sidecarSource = join(root, "native", "bin", sidecarName);
const nativeCliName = "mant-cli";
const nativeCliSource = join(root, "native", "bin", nativeCliName);
const nativeCliPath = join(distDirectory, nativeCliName);
const sidecarBuildInputs = [
  join(root, "native", "mandoc-json", "mant-mandoc-json.c"),
  join(root, "scripts", "build-mandoc-json.sh"),
];
const executableName = "mant";
const executablePath = join(distDirectory, executableName);
const compiledEntrypoint = join(distDirectory, ".mant-compile-entry.ts");

async function isExecutable(path: string): Promise<boolean> {
  try {
    await access(path, constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

/** Returns false when a tracked native input is newer than the local binary. */
async function isCurrentSidecar(path: string): Promise<boolean> {
  if (!(await isExecutable(path))) return false;

  const outputModifiedAt = (await stat(path)).mtimeMs;
  const inputStats = await Promise.all(sidecarBuildInputs.map((input) => stat(input)));
  return inputStats.every((input) => input.mtimeMs <= outputModifiedAt);
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
  if (helpExitCode !== 0 || !helpOutput.includes("mant-cli <topic>")) {
    throw new Error(`packaged TUI help smoke test failed: ${helpStderr.trim()}`);
  }

  const queryProcess = Bun.spawn([nativeCliPath, "ls", "--json", "--compact"], {
    cwd: distDirectory,
    stdout: "pipe",
    stderr: "pipe",
  });
  const [queryOutput, queryStderr, queryExitCode] = await Promise.all([
    new Response(queryProcess.stdout).text(),
    new Response(queryProcess.stderr).text(),
    queryProcess.exited,
  ]);
  if (queryExitCode !== 0) {
    throw new Error(`packaged mant-cli smoke test failed: ${queryStderr.trim()}`);
  }
  const query = JSON.parse(queryOutput) as {
    schema?: string;
    manual?: { schema?: string; sections?: unknown[] };
  };
  if (
    query.schema !== "mant.query/v1"
    || query.manual?.schema !== "mant.document/v1"
    || !query.manual.sections?.length
  ) {
    throw new Error("packaged mant-cli did not return a readable native document");
  }
}

async function writeCompiledEntrypoint(): Promise<void> {
  // Keep the asset-only import outside src/ so `bun run dev` can work before a
  // native sidecar has been built. Bun embeds this bare file import in --compile
  // mode, where src/core/sidecar-cache.ts discovers it through embeddedFiles.
  await Bun.write(
    compiledEntrypoint,
    [
      'import "../src/cli.ts";',
      `import "../native/bin/${sidecarName}" with { type: "file" };`,
      "",
    ].join("\n"),
  );
}

async function main(): Promise<void> {
  assertSupportedBuildPlatform();

  await run("install locked dependencies", [process.execPath, "install", "--frozen-lockfile"]);
  await run("type check", [process.execPath, "run", "lint"]);
  await run("check Rust formatting", [
    "cargo",
    "fmt",
    "--manifest-path",
    join(root, "native", "Cargo.toml"),
    "--all",
    "--check",
  ]);
  await run("test Rust workspace", [
    "cargo",
    "test",
    "--manifest-path",
    join(root, "native", "Cargo.toml"),
    "--workspace",
  ]);
  await run("lint Rust workspace", [
    "cargo",
    "clippy",
    "--manifest-path",
    join(root, "native", "Cargo.toml"),
    "--workspace",
    "--all-targets",
    "--",
    "-D",
    "warnings",
  ]);
  await run("build native mant-cli", [process.execPath, "run", "build:mant-cli"]);
  if (!(await isExecutable(nativeCliSource))) {
    throw new Error("build:mant-cli did not stage an executable native/bin/mant-cli");
  }

  // Skip the mandoc download/compile cycle when a usable sidecar already
  // exists.  Set MANT_REBUILD_SIDECAR=1 to force a rebuild after changing
  // native/mandoc-json/mant-mandoc-json.c or the pinned mandoc version.
  const rebuildSidecar = process.env.MANT_REBUILD_SIDECAR === "1";
  const sidecarExists = await isExecutable(sidecarSource);
  const sidecarReady = !rebuildSidecar
    && sidecarExists
    && await isCurrentSidecar(sidecarSource);
  if (sidecarReady) {
    console.log("\n==> libmandoc sidecar already present; skipping build:mandoc-json");
    console.log("    (set MANT_REBUILD_SIDECAR=1 to force a rebuild)");
  } else {
    // Resolve the compiler only when native code actually needs rebuilding.
    // A packaged/prebuilt sidecar therefore remains usable on hosts without a
    // development toolchain.
    const compiler = resolveCCompiler();
    const compilerOrigin = compiler.source === "environment"
      ? "CC environment variable"
      : `${process.platform} default`;
    console.log(`\n    C compiler: ${compiler.path} (${compilerOrigin})`);
    const compilerName = basename(compiler.path);
    const label = sidecarExists
      ? `rebuild libmandoc sidecar with ${compilerName}`
      : `build libmandoc sidecar with ${compilerName}`;
    await run(label, [process.execPath, "run", "build:mandoc-json"], {
      CC: compiler.path,
    });
  }

  await run("test", [process.execPath, "test"]);
  await mkdir(distDirectory, { recursive: true });
  await copyFile(nativeCliSource, nativeCliPath);
  await chmod(nativeCliPath, 0o755);
  await rm(join(distDirectory, "mant-mandoc-json"), { force: true });
  await writeCompiledEntrypoint();
  try {
    await run("compile current-platform executable", [
      process.execPath,
      "build",
      "--compile",
      "--define",
      "MANT_COMPILED=true",
      "--outfile",
      executablePath,
      compiledEntrypoint,
    ]);
  } finally {
    await rm(compiledEntrypoint, { force: true });
  }
  await verifyPackagedExecutable();

  console.log(`\nlocal CI succeeded: ${dirname(executablePath)}`);
  console.log(`  TUI:        ${executablePath}`);
  console.log(`  agent CLI:  ${nativeCliPath}`);
}

await main();
