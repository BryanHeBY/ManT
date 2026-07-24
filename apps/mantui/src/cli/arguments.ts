/**
 * @file Parses the interactive `mantui` command line without loading the TUI.
 *
 * Non-interactive outline, text, JSON, Markdown, and cache operations belong
 * to the separate Rust `mant`, leaving this command focused on TUI use.
 */

// ── Public command model ───────────────────────────────────────────────────

export type CliCommand =
  | { kind: "help" }
  | {
    kind: "query";
    input:
      | { kind: "manual"; topic: string; section?: string }
      | { kind: "markdown-file"; path: string };
    forceLibmandoc?: boolean;
    forceGroff?: boolean;
  };

/** An invalid invocation; callers report these with exit code 2. */
export class CliUsageError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CliUsageError";
  }
}

// ── Help text ──────────────────────────────────────────────────────────────

export const CLI_HELP = `ManT — explore local manuals and Markdown in a structured terminal UI

Usage:
  mantui <topic|markdown> [--section <section>] [--force-libmandoc] [--force-groff]
  mantui -h, --help

Options:
  -h, --help              Show this help and exit
  -s, --section <value>   Select a manual section, such as 1 or 3p
  --force-libmandoc       Require direct libmandoc output and print diagnostics
  --force-groff           Use man -Thtml + groff HTML parser instead of libmandoc
                           (not comprehensively tested)
  --                      Treat all remaining arguments as the topic

What ManT provides:
  Structured documents with a hierarchy-aware sidebar, in-page references,
  document search, and optional or embedded tldr quick references.

For agents and scripts:
  mant <topic> --outline               Discover sections and options
  mant <topic> --explain=--option      Explain one semantic entry
  mant <topic> --search=<pattern>      Find text with stable locations
  mant -h                              Show Markdown, text, JSON, and schema output

Examples:
  mantui git
  mantui README.md
  mantui printf --section 3
  mantui --force-libmandoc tar`;

// ── Parser ─────────────────────────────────────────────────────────────────

/** Converts raw argv tokens into one validated interactive command. */
export function parseCliArguments(args: readonly string[]): CliCommand {
  let showHelp = false;
  let parseOptions = true;
  let section: string | undefined;
  let forceLibmandoc = false;
  let forceGroff = false;
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
    } else if (parseOptions && arg === "--force-libmandoc") {
      forceLibmandoc = true;
    } else if (parseOptions && arg === "--force-groff") {
      forceGroff = true;
    } else if (parseOptions && arg.startsWith("-")) {
      throw new CliUsageError(
        `unknown option '${arg}'; non-interactive output is provided by mant`,
      );
    } else {
      topicParts.push(arg);
    }
  }

  // Help stays side-effect free even when another token was supplied.
  if (showHelp) return { kind: "help" };

  const value = topicParts.join(" ").trim();
  if (!value) throw new CliUsageError("a manual topic or Markdown path is required");
  if (forceLibmandoc && forceGroff) {
    throw new CliUsageError("--force-libmandoc and --force-groff cannot be used together");
  }
  if (section !== undefined && !section.trim()) {
    throw new CliUsageError("manual section must not be empty");
  }
  const markdown = isMarkdownPath(value);
  if (markdown && section !== undefined) {
    throw new CliUsageError("--section applies only to manual topics");
  }
  if (markdown && (forceLibmandoc || forceGroff)) {
    throw new CliUsageError("manual renderer policies do not apply to Markdown input");
  }
  return {
    kind: "query",
    input: markdown
      ? { kind: "markdown-file", path: value }
      : {
          kind: "manual",
          topic: value,
          ...(section === undefined ? {} : { section: section.trim() }),
        },
    ...(forceLibmandoc ? { forceLibmandoc: true } : {}),
    ...(forceGroff ? { forceGroff: true } : {}),
  };
}

function isMarkdownPath(value: string): boolean {
  const lower = value.toLocaleLowerCase();
  return lower.endsWith(".md")
    || lower.endsWith(".markdown")
    || value.startsWith(".")
    || value.includes("/")
    || value.includes("\\");
}
