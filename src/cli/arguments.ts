/**
 * @file Parses command-line tokens into Mant's small command model.
 *
 * This module has no process or UI dependencies, keeping usage validation
 * deterministic and allowing help/error behavior to be tested in isolation.
 */

// ── Public command model ────────────────────────────────────

export type CliOutputMode = "tui" | "json" | "roff-ast";

export type CliCommand =
  | { kind: "help" }
  | { kind: "update-tldr" }
  | { kind: "query"; topic: string; output: CliOutputMode };

/** An invalid invocation; callers should report these with exit code 2. */
export class CliUsageError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CliUsageError";
  }
}

// ── Help text ───────────────────────────────────────────────

export const CLI_HELP = `Mant — browse local man pages in a structured terminal UI

Usage:
  mant <topic> [--json | --roff-ast]
  mant --update-tldr
  mant --help

Options:
  -h, --help       Show this help and exit
  -j, --json       Print the parsed manual as JSON
      --roff-ast   Print the source-level libmandoc AST as JSON
      --update-tldr
                   Clone or update the local tldr-pages cache
  --               Treat all remaining arguments as the topic

Examples:
  mant git
  mant printf --json
  mant --update-tldr`;

// ── Parser ──────────────────────────────────────────────────

/** Converts raw argv tokens into one validated command. */
export function parseCliArguments(args: readonly string[]): CliCommand {
  let output: CliOutputMode = "tui";
  let updateTldr = false;
  let showHelp = false;
  let parseOptions = true;
  const topicParts: string[] = [];

  for (const arg of args) {
    if (parseOptions && arg === "--") {
      parseOptions = false;
    } else if (parseOptions && (arg === "--help" || arg === "-h")) {
      showHelp = true;
    } else if (parseOptions && (arg === "--json" || arg === "-j")) {
      output = mergeOutputMode(output, "json");
    } else if (parseOptions && arg === "--roff-ast") {
      output = mergeOutputMode(output, "roff-ast");
    } else if (parseOptions && arg === "--update-tldr") {
      updateTldr = true;
    } else if (parseOptions && arg.startsWith("-")) {
      throw new CliUsageError(`unknown option '${arg}'`);
    } else {
      topicParts.push(arg);
    }
  }

  // Help is intentionally side-effect free even when a topic or action was
  // also supplied, matching the forgiving behavior of established CLI tools.
  if (showHelp) return { kind: "help" };

  if (updateTldr) {
    if (topicParts.length > 0 || output !== "tui") {
      throw new CliUsageError(
        "--update-tldr cannot be combined with a topic or output option",
      );
    }
    return { kind: "update-tldr" };
  }

  const topic = topicParts.join(" ").trim();
  if (!topic) throw new CliUsageError("a manual topic is required");
  return { kind: "query", topic, output };
}

function mergeOutputMode(
  current: CliOutputMode,
  requested: Exclude<CliOutputMode, "tui">,
): CliOutputMode {
  if (current !== "tui" && current !== requested) {
    throw new CliUsageError("--json and --roff-ast cannot be used together");
  }
  return requested;
}
