#!/usr/bin/env bun
/**
 * @file Starts the interactive `mantui` TUI executable.
 *
 * The separate Rust `mant` executable owns querying and agent output.
 * This file only translates the testable TUI host result into a process exit
 * code and retains a last-resort boundary for unexpected failures.
 */

import { formatUnexpectedError, runCli } from "./cli/run";

runCli(process.argv.slice(2)).then(
  (exitCode) => {
    process.exitCode = exitCode;
  },
  (error: unknown) => {
    console.error(formatUnexpectedError(error, Boolean(process.env.MANT_DEBUG)));
    process.exitCode = 1;
  },
);
