/**
 * @file Executes the interactive `mantui` command and owns its error boundary.
 *
 * Query semantics live in Rust's `mant`; this module only checks terminal
 * suitability, invokes the versioned process client, and starts OpenTUI.
 */

import type { MantQueryBundle, NativeQueryRequest } from "../native";
import { CLI_HELP, CliUsageError, parseCliArguments } from "./arguments";

type OutputWriter = (message: string) => void;

export interface CliDependencies {
  query?: (request: NativeQueryRequest) => Promise<MantQueryBundle>;
  runTui?: (result: MantQueryBundle) => Promise<void>;
  isInteractive?: () => boolean;
  stdout?: OutputWriter;
  stderr?: OutputWriter;
  env?: Record<string, string | undefined>;
}

const writeStdout: OutputWriter = (message) => console.log(message);
const writeStderr: OutputWriter = (message) => console.error(message);

// ── Public execution boundary ──────────────────────────────────────────────

/** Runs one invocation and returns a conventional process exit code. */
export async function runCli(
  args: readonly string[],
  dependencies: CliDependencies = {},
): Promise<number> {
  const stdout = dependencies.stdout ?? writeStdout;
  const stderr = dependencies.stderr ?? writeStderr;

  try {
    const command = parseCliArguments(args);
    if (command.kind === "help") {
      stdout(CLI_HELP);
      return 0;
    }

    const isInteractive = dependencies.isInteractive
      ?? (() => Boolean(process.stdin.isTTY && process.stdout.isTTY));
    if (!isInteractive()) {
      throw new Error(
        "interactive view requires a terminal; use mant for Markdown or JSON output",
      );
    }

    const executeQuery = dependencies.query
      ?? (await import("../native")).mantClient.query;
    const result = await executeQuery({
      topic: command.topic,
      ...(command.section === undefined ? {} : { section: command.section }),
      ...(command.forceLibmandoc ? { forceLibmandoc: true } : {}),
      ...(command.forceGroff ? { forceGroff: true } : {}),
    });
    const startTui = dependencies.runTui ?? (await import("../ui/app")).runTui;
    await startTui(result);
    return 0;
  } catch (error) {
    const debug = Boolean((dependencies.env ?? process.env).MANT_DEBUG);
    stderr(formatCliError(error, debug));
    return error instanceof CliUsageError ? 2 : 1;
  }
}

// ── Error presentation ────────────────────────────────────────────────────

/** Formats expected failures without leaking runtime stack traces. */
export function formatCliError(error: unknown, debug = false): string {
  if (debug && error instanceof Error && error.stack) return error.stack;

  const message = getErrorMessage(error);
  if (error instanceof CliUsageError) {
    return `mantui: ${message}\nTry 'mantui --help' for more information.`;
  }
  return `mantui: ${message}`;
}

/** Formats failures that somehow escape runCli's defensive boundary. */
export function formatUnexpectedError(error: unknown, debug = false): string {
  if (debug && error instanceof Error && error.stack) return error.stack;
  return `mantui: unexpected failure: ${getErrorMessage(error)}`;
}

function getErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message.trim();
  if (typeof error === "string" && error.trim()) return error.trim();
  return "an unknown error occurred";
}
