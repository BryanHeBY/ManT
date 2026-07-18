#!/usr/bin/env bun
/**
 * @file Provides Mant's minimal executable entry point.
 *
 * Argument parsing and command execution live in testable CLI modules.  This
 * file only translates their result into a process exit code and retains one
 * last-resort boundary so an unexpected failure never exposes a Bun stack by
 * default.
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
