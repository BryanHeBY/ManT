/**
 * @file Connects the incremental Cargo build to Mant's Bun development entry.
 *
 * Development never relies on a globally installed native CLI. The freshly
 * staged artifact is selected explicitly, while release installations use
 * the ordinary `MANT_CLI_PATH` then PATH lookup policy.
 */

import { buildMantCli } from "./build-mant-cli";

const root = new URL("..", import.meta.url).pathname;

try {
  const nativeCli = await buildMantCli();
  const child = Bun.spawn(
    [process.execPath, "src/cli.ts", ...process.argv.slice(2)],
    {
      cwd: root,
      env: { ...process.env, MANT_CLI_PATH: nativeCli },
      stdin: "inherit",
      stdout: "inherit",
      stderr: "inherit",
    },
  );
  process.exitCode = await child.exited;
} catch (error) {
  const detail = error instanceof Error ? error.message : String(error);
  console.error(`mant development startup failed: ${detail}`);
  process.exitCode = 1;
}
