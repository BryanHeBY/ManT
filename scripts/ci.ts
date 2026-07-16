import { chmod, mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const distDirectory = join(root, "dist");
const sidecarSource = join(root, "native", "bin", "mant-mandoc-json");
const executableName = process.platform === "win32" ? "mant.exe" : "mant";
const sidecarName = process.platform === "win32"
  ? "mant-mandoc-json.exe"
  : "mant-mandoc-json";
const executablePath = join(distDirectory, executableName);
const sidecarOutput = join(distDirectory, sidecarName);

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
  const child = Bun.spawn([executablePath, "ls", "--roff-ast"], {
    cwd: distDirectory,
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
    throw new Error("packaged executable did not use the bundled mandoc sidecar");
  }
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
  await run("build libmandoc sidecar with GCC", [process.execPath, "run", "build:mandoc-json"], {
    CC: gcc,
  });
  await run("test", [process.execPath, "test"]);
  await run("compile current-platform executable", [
    process.execPath,
    "build",
    "--compile",
    "--define",
    "MANT_COMPILED=true",
    "--outfile",
    executablePath,
    "src/cli.ts",
  ]);

  await mkdir(distDirectory, { recursive: true });
  await Bun.write(sidecarOutput, Bun.file(sidecarSource));
  if (process.platform !== "win32") await chmod(sidecarOutput, 0o755);
  await verifyPackagedExecutable();

  console.log(`\nlocal CI succeeded: ${dirname(executablePath)}`);
}

await main();
