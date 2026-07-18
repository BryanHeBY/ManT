/**
 * @file Runs Mant's local cross-platform build and test verification sequence.
 */

import { access, constants, mkdir, readdir, rm } from "node:fs/promises";
import { dirname, join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const distDirectory = join(root, "dist");
const sidecarSource = join(root, "native", "bin", "mant-mandoc-json");
const executableName = process.platform === "win32" ? "mant.exe" : "mant";
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
  console.log("\n==> packaged AST smoke test");
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
    if (!cachedFiles.some((path) => path.endsWith("mant-mandoc-json"))) {
      throw new Error("packaged executable did not materialize its embedded mandoc sidecar");
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
      'import "../native/bin/mant-mandoc-json" with { type: "file" };',
      "",
    ].join("\n"),
  );
}

async function main(): Promise<void> {
  if ((process.platform as string) === "win32") {
    throw new Error("local GCC build is supported on Linux and macOS; use WSL on Windows");
  }

  const gcc = Bun.which("gcc");
  if (!gcc) {
    throw new Error("gcc is required: install GCC before running bun run build");
  }

  await run("install locked dependencies", [process.execPath, "install", "--frozen-lockfile"]);
  await run("type check", [process.execPath, "run", "lint"]);

  // Skip the mandoc download/compile cycle when a usable sidecar already
  // exists.  Set MANT_REBUILD_SIDECAR=1 to force a rebuild after changing
  // native/mandoc-json/mant-mandoc-json.c or the pinned mandoc version.
  const rebuildSidecar = process.env.MANT_REBUILD_SIDECAR === "1";
  const sidecarReady = !rebuildSidecar && await isExecutable(sidecarSource);
  if (sidecarReady) {
    console.log("\n==> libmandoc sidecar already present; skipping build:mandoc-json");
    console.log("    (set MANT_REBUILD_SIDECAR=1 to force a rebuild)");
  } else {
    const label = rebuildSidecar
      ? "rebuild libmandoc sidecar with GCC"
      : "build libmandoc sidecar with GCC";
    await run(label, [process.execPath, "run", "build:mandoc-json"], {
      CC: gcc,
    });
  }

  await run("test", [process.execPath, "test"]);
  await mkdir(distDirectory, { recursive: true });
  await rm(join(distDirectory, "mant-mandoc-json"), { force: true });
  await rm(join(distDirectory, "mant-mandoc-json.exe"), { force: true });
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
