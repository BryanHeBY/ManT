/**
 * @file Runs Mant's local cross-platform build and test verification sequence.
 */

import { access, constants, mkdir, readdir, rm, stat } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { assertSupportedBuildPlatform, resolveCCompiler } from "./c-compiler";

const root = new URL("..", import.meta.url).pathname;
const distDirectory = join(root, "dist");
const sidecarName = "mant-mandoc-json";
const sidecarSource = join(root, "native", "bin", sidecarName);
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
  console.log("\n==> packaged sidecar smoke tests");
  const sidecarCache = join(distDirectory, ".sidecar-cache");
  await rm(sidecarCache, { recursive: true, force: true });
  try {
    const child = Bun.spawn([executablePath, "ls", "--roff-ast"], {
      cwd: distDirectory,
      env: { ...process.env, MANT_SIDECAR_DIR: sidecarCache },
      stdout: "pipe",
      stderr: "pipe",
    });
    const [output, stderr, exitCode] = await Promise.all([
      new Response(child.stdout).text(),
      new Response(child.stderr).text(),
      child.exited,
    ]);
    if (exitCode !== 0) {
      throw new Error(`packaged AST smoke test failed: ${stderr.trim()}`);
    }

    const result = JSON.parse(output) as {
      document?: { schema?: string; engine?: { name?: string } };
    };
    if (
      result.document?.schema !== "mant.roff-ast/v1"
      || result.document.engine?.name !== "libmandoc"
    ) {
      throw new Error("packaged executable did not use the embedded mandoc sidecar");
    }

    const cachedFiles = await readdir(sidecarCache, { recursive: true });
    if (!cachedFiles.some((path) => path.endsWith(sidecarName))) {
      throw new Error("packaged executable did not materialize its embedded mandoc sidecar");
    }

    // Exercise the ordinary query pipeline too. The AST check above proves
    // extraction works; this check proves the embedded sidecar's HTML mode is
    // wired into the parser used by end users.
    const queryProcess = Bun.spawn([executablePath, "ls", "--json"], {
      cwd: distDirectory,
      env: { ...process.env, MANT_SIDECAR_DIR: sidecarCache },
      stdout: "pipe",
      stderr: "pipe",
    });
    const [queryOutput, queryStderr, queryExitCode] = await Promise.all([
      new Response(queryProcess.stdout).text(),
      new Response(queryProcess.stderr).text(),
      queryProcess.exited,
    ]);
    if (queryExitCode !== 0) {
      throw new Error(`packaged HTML smoke test failed: ${queryStderr.trim()}`);
    }
    const queryResult = JSON.parse(queryOutput) as { sections?: unknown[] };
    if (!queryResult.sections?.length) {
      throw new Error("packaged HTML smoke test returned no manual sections");
    }
  } finally {
    await rm(sidecarCache, { recursive: true, force: true });
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
}

await main();
