# mant

Query local Unix manual pages and Markdown as structured documents.

## TLDR Quick Reference

Inspect a manual outline before requesting a small section:

```sh
mant tar --outline
mant tar --node 5.4
```

Explain one option directly:

```sh
mant tar --explain=--exclude
```

Read a Markdown file or standard input:

```sh
mant README.md
cat guide.md | mant -
```

## Synopsis

```text
mant <TOPIC|MARKDOWN|-> [OPTIONS]
mant --request-json [--format FORMAT] [--compact]
mant --schema CONTRACT [--compact]
mant --update-tldr [--compact]
mant --protocol-version [--compact]
mant --mcp
```

## Description

`mant` is the native, non-interactive ManT command for agents, scripts, and
terminal output. It parses local man and mdoc sources through bundled
libmandoc, and lowers supported Markdown into the same versioned document
model.

Full document queries default to Markdown. Outline, excerpt, search, text, and
JSON projections all derive from the same parsed document.

## Input

A value ending in `.md` or `.markdown`, or another path-like value, is read as
a local Markdown file. The exact value `-` reads Markdown from standard input.
Other values are resolved as local manual topics.

### Manual input

- `--section SECTION`: Select a manual section such as `1` or `3p`.
- `--force-libmandoc`: Require direct libmandoc output and print parser diagnostics.
- `--force-groff`: Use the opt-in `man -Thtml` and groff HTML compatibility path.

Renderer options are rejected for Markdown input.

## Document Selection

- `--outline[=DETAIL]`: Print the addressable tree; `options` is the default and `sections` is the compact form.
- `--node NODE`: Return a node by path or ID; repeat the option to select several nodes.
- `--explain ENTRY`: Return exactly one semantic option, command, or environment entry.

Path `0` is reserved for an external tldr quick reference. Markdown content
before its first heading is exposed as path `root` with ID
`document-overview`. Ordinary headings use one-based paths such as `2.3`, and
semantic entries use paths such as `2.3/o4`.

## Search

- `--search PATTERN`: Search visible text and report reusable nodes plus Markdown coordinates.
- `--grep PATTERN`: Alias for `--search`.
- `--regex`: Interpret the pattern as a regular expression.
- `--case POLICY`: Use `insensitive`, `sensitive`, or `smart` case handling.
- `--word`: Require Unicode-aware word boundaries.
- `--scope SCOPE`: Search `visible` text or generated `markdown`.
- `--context LINES`: Include surrounding Markdown lines.
- `--limit COUNT`: Limit returned matches.
- `--offset COUNT`: Skip matches for deterministic pagination.

Use the `=` form when a value begins with a hyphen:

```sh
mant tar --search=--acls
mant tar --explain=--exclude
```

## Output

- `--format FORMAT`: Select `markdown`, `text`, `man`, or `json`.
- `--compact`: Omit JSON indentation.
- `--preserve-anchors`: Retain addressable HTML anchors in Markdown output.

Clean Markdown output omits internal HTML anchors by default. The `man` format
is plain manual content without an external tldr preface.

## Integration

- `--request-json`: Read one closed `mant.request/v3` object from standard input.
- `--schema CONTRACT`: Print a generated JSON Schema for `request`, `query`, `outline`, `excerpt`, `search`, or `all`.
- `--protocol-version`: Print the exact native protocol versions.
- `--mcp`: Serve read-only ManT tools over MCP stdio.

Standard output is reserved for the requested result. Concise diagnostics use
standard error. MCP tools use a generated `target` union for either a manual
topic or local Markdown path.

## Data

- `--update-tldr`: Update through an installed tldr client when available, otherwise through ManT's private cache.

## General

- `-h`, `--help`: Show command help and exit.

## Exit Status

`0` indicates success, `2` indicates invalid input or usage, and `1` indicates
an operational failure.

## See Also

`mantui` provides the interactive reader for the same structured documents.
