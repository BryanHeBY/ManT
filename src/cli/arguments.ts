/**
 * @file Parses the interactive `mant` command line without loading the TUI.
 *
 * Non-interactive outline, text, JSON, Markdown, and cache operations belong
 * to the separate Rust `mant-cli`, leaving this command focused on TUI use.
 */

// ── Public command model ───────────────────────────────────────────────────

export type CliCommand =
  | { kind: "help" }
  | { kind: "query"; topic: string; section?: string };

/** An invalid invocation; callers report these with exit code 2. */
export class CliUsageError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CliUsageError";
  }
}

// ── Help text ──────────────────────────────────────────────────────────────

export const CLI_HELP = `Mant — browse local man pages in a structured terminal UI

Usage:
  mant <topic> [--section <section>]
  mant --help

Options:
  -h, --help              Show this help and exit
  -s, --section <value>   Select a manual section, such as 1 or 3p
  --                      Treat all remaining arguments as the topic

Agent and pipeline output:
  mant-cli <topic>              Print Markdown
  mant-cli <topic> --outline    Print a selectable section tree
  mant-cli <topic> --node 4.2   Print one section subtree
  mant-cli <topic> --text       Print unstyled semantic text
  mant-cli <topic> --json       Print the versioned document as JSON
  mant-cli update tldr          Update the tldr cache

Examples:
  mant git
  mant printf --section 3`;

// ── Parser ─────────────────────────────────────────────────────────────────

/** Converts raw argv tokens into one validated interactive command. */
export function parseCliArguments(args: readonly string[]): CliCommand {
  let showHelp = false;
  let parseOptions = true;
  let section: string | undefined;
  const topicParts: string[] = [];

  for (let index = 0; index < args.length; index++) {
    const arg = args[index]!;
    if (parseOptions && arg === "--") {
      parseOptions = false;
    } else if (parseOptions && (arg === "--help" || arg === "-h")) {
      showHelp = true;
    } else if (parseOptions && (arg === "--section" || arg === "-s")) {
      const value = args[++index];
      if (value === undefined) throw new CliUsageError("--section requires a value");
      if (section !== undefined) {
        throw new CliUsageError("--section may only be supplied once");
      }
      section = value;
    } else if (parseOptions && arg.startsWith("-")) {
      throw new CliUsageError(
        `unknown option '${arg}'; non-interactive output is provided by mant-cli`,
      );
    } else {
      topicParts.push(arg);
    }
  }

  // Help stays side-effect free even when another token was supplied.
  if (showHelp) return { kind: "help" };

  const topic = topicParts.join(" ").trim();
  if (!topic) throw new CliUsageError("a manual topic is required");
  if (section !== undefined && !section.trim()) {
    throw new CliUsageError("manual section must not be empty");
  }
  return {
    kind: "query",
    topic,
    ...(section === undefined ? {} : { section: section.trim() }),
  };
}
