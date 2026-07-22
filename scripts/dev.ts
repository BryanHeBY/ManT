/**
 * @file Connects the incremental Cargo build to ManT's Bun development entry.
 *
 * Development never relies on a globally installed mant. The freshly
 * staged artifact is selected explicitly, while release installations use
 * the ordinary `MANT_PATH` then PATH lookup policy.
 */

import { buildMant } from "./build-mant";

const root = new URL("..", import.meta.url).pathname;

try {
  const mantPath = await buildMant();
  const child = Bun.spawn(
    [process.execPath, "apps/mantui/src/mantui.ts", ...process.argv.slice(2)],
    {
      cwd: root,
      env: { ...process.env, MANT_PATH: mantPath },
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
