/**
 * @file Builds and launches the current Rust `mant` command for development.
 *
 * Development never relies on a globally installed mant. The freshly
 * staged artifact is executed directly, so development never accidentally
 * selects a globally installed release.
 */

import { buildMant } from "./build-mant";

try {
  const mantPath = await buildMant();
  const child = Bun.spawn(
    [mantPath, ...process.argv.slice(2)],
    {
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
