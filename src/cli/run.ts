/**
 * @file Executes parsed CLI commands and owns Mant's user-facing error boundary.
 *
 * Heavy query and TUI modules are loaded only for commands that need them, so
 * `mant --help` remains usable even when an optional runtime component fails.
 */

import type { RoffAstResult } from "../core";
import type { QueryResult } from "../query";
import type { TldrCacheUpdate } from "../tldr";
import { CLI_HELP, CliUsageError, parseCliArguments } from "./arguments";

// ── Injectable host boundary ────────────────────────────────

type OutputWriter = (message: string) => void;

export interface CliDependencies {
  query?: (options: { topic: string }) => Promise<QueryResult>;
  fetchRoffAst?: (topic: string) => Promise<RoffAstResult>;
  updateTldrCache?: () => Promise<TldrCacheUpdate>;
  runTui?: (result: QueryResult) => Promise<void>;
  isInteractive?: () => boolean;
  stdout?: OutputWriter;
  stderr?: OutputWriter;
  env?: Record<string, string | undefined>;
}

const writeStdout: OutputWriter = (message) => console.log(message);
const writeStderr: OutputWriter = (message) => console.error(message);

// ── Public execution boundary ───────────────────────────────

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

    if (command.kind === "update-tldr") {
      const update = dependencies.updateTldrCache
        ?? (await import("../tldr")).updateTldrCache;
      const result = await update();
      const revision = result.revision ? ` (${result.revision})` : "";
      stdout(`tldr cache ${result.action}: ${result.cacheDir}${revision}`);
      return 0;
    }

    if (command.output === "roff-ast") {
      const fetchAst = dependencies.fetchRoffAst
        ?? (await import("../core")).fetchRoffAst;
      stdout(JSON.stringify(await fetchAst(command.topic), null, 2));
      return 0;
    }

    if (command.output === "tui") {
      const isInteractive = dependencies.isInteractive
        ?? (() => Boolean(process.stdin.isTTY && process.stdout.isTTY));
      if (!isInteractive()) {
        throw new Error(
          "interactive view requires a terminal; use --json for redirected or scripted output",
        );
      }
    }

    const executeQuery = dependencies.query ?? (await import("../query")).query;
    const result = await executeQuery({ topic: command.topic });

    if (command.output === "json") {
      stdout(JSON.stringify(result, null, 2));
      return 0;
    }

    const startTui = dependencies.runTui ?? (await import("../ui/app")).runTui;
    await startTui(result);
    return 0;
  } catch (error) {
    const debug = Boolean((dependencies.env ?? process.env).MANT_DEBUG);
    stderr(formatCliError(error, debug));
    return error instanceof CliUsageError ? 2 : 1;
  }
}

// ── Error presentation ──────────────────────────────────────

/** Formats expected CLI failures without leaking runtime stack traces. */
export function formatCliError(error: unknown, debug = false): string {
  if (debug && error instanceof Error && error.stack) return error.stack;

  const message = getErrorMessage(error);
  if (error instanceof CliUsageError) {
    return `mant: ${message}\nTry 'mant --help' for more information.`;
  }
  return `mant: ${message}`;
}

/** Formats failures that somehow escape runCli's defensive boundary. */
export function formatUnexpectedError(error: unknown, debug = false): string {
  if (debug && error instanceof Error && error.stack) return error.stack;
  return `mant: unexpected failure: ${getErrorMessage(error)}`;
}

function getErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message.trim();
  if (typeof error === "string" && error.trim()) return error.trim();
  return "an unknown error occurred";
}
