/**
 * @file Verifies Mant's pure CLI grammar and incompatible-option validation.
 */

import { describe, expect, test } from "bun:test";
import {
  CliUsageError,
  parseCliArguments,
} from "../../../src/cli/arguments";

describe("CLI argument parsing", () => {
  test("recognises both help aliases without requiring a topic", () => {
    expect(parseCliArguments(["--help"])).toEqual({ kind: "help" });
    expect(parseCliArguments(["-h"])).toEqual({ kind: "help" });
  });

  test("parses TUI, JSON, and roff AST queries", () => {
    expect(parseCliArguments(["git"])).toEqual({
      kind: "query",
      topic: "git",
      output: "tui",
    });
    expect(parseCliArguments(["git", "--json"])).toEqual({
      kind: "query",
      topic: "git",
      output: "json",
    });
    expect(parseCliArguments(["printf", "--roff-ast"])).toEqual({
      kind: "query",
      topic: "printf",
      output: "roff-ast",
    });
  });

  test("joins multi-part topics and honours the option terminator", () => {
    expect(parseCliArguments(["git", "commit"])).toEqual({
      kind: "query",
      topic: "git commit",
      output: "tui",
    });
    expect(parseCliArguments(["--", "--help"])).toEqual({
      kind: "query",
      topic: "--help",
      output: "tui",
    });
  });

  test("accepts the standalone tldr cache action", () => {
    expect(parseCliArguments(["--update-tldr"])).toEqual({
      kind: "update-tldr",
    });
  });

  test("rejects missing topics, unknown options, and conflicting actions", () => {
    expect(() => parseCliArguments([])).toThrow(CliUsageError);
    expect(() => parseCliArguments(["--unknown"])).toThrow("unknown option '--unknown'");
    expect(() => parseCliArguments(["git", "--json", "--roff-ast"]))
      .toThrow("--json and --roff-ast cannot be used together");
    expect(() => parseCliArguments(["git", "--update-tldr"]))
      .toThrow("--update-tldr cannot be combined");
  });
});
