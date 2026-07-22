/**
 * @file Verifies the intentionally small interactive `mantui` grammar.
 */

import { describe, expect, test } from "bun:test";
import {
  CliUsageError,
  parseCliArguments,
} from "../../../src/cli/arguments";

describe("interactive CLI argument parsing", () => {
  test("recognises both help aliases without requiring a topic", () => {
    expect(parseCliArguments(["--help"])).toEqual({ kind: "help" });
    expect(parseCliArguments(["-h"])).toEqual({ kind: "help" });
  });

  test("parses a topic and optional manual section", () => {
    expect(parseCliArguments(["git"])).toEqual({
      kind: "query",
      topic: "git",
    });
    expect(parseCliArguments(["printf", "--section", " 3p "])).toEqual({
      kind: "query",
      topic: "printf",
      section: "3p",
    });
    expect(parseCliArguments(["-s", "1", "git"])).toEqual({
      kind: "query",
      topic: "git",
      section: "1",
    });
    expect(parseCliArguments(["tar", "--force-libmandoc"])).toEqual({
      kind: "query",
      topic: "tar",
      forceLibmandoc: true,
    });
    expect(parseCliArguments(["tar", "--force-groff"])).toEqual({
      kind: "query",
      topic: "tar",
      forceGroff: true,
    });
  });

  test("joins multi-part topics and honours the option terminator", () => {
    expect(parseCliArguments(["git", "commit"])).toEqual({
      kind: "query",
      topic: "git commit",
    });
    expect(parseCliArguments(["--", "--help"])).toEqual({
      kind: "query",
      topic: "--help",
    });
  });

  test("routes non-interactive options to mant", () => {
    expect(() => parseCliArguments(["git", "--format", "json"]))
      .toThrow("non-interactive output is provided by mant");
    expect(() => parseCliArguments(["--update-tldr"]))
      .toThrow("non-interactive output is provided by mant");
  });

  test("rejects missing topics and malformed section options", () => {
    expect(() => parseCliArguments([])).toThrow(CliUsageError);
    expect(() => parseCliArguments(["--section"])).toThrow("requires a value");
    expect(() => parseCliArguments(["git", "-s", "1", "-s", "2"]))
      .toThrow("only be supplied once");
    expect(() => parseCliArguments(["git", "-s", " "]))
      .toThrow("must not be empty");
    expect(() => parseCliArguments(["git", "--force-libmandoc", "--force-groff"]))
      .toThrow("cannot be used together");
  });
});
