/**
 * @file Tests the JSON sidecar boundary used for source-level roff ASTs.
 */

import { describe, expect, test } from "bun:test";
import {
  createRoffAstFetcher,
  type RoffAstDocument,
} from "../../../src/core/roff-ast";
import type { CommandResult } from "../../../src/core/fetcher";

const encoder = new TextEncoder();

function result(stdout: string, exitCode = 0, stderr = ""): CommandResult {
  return { stdout: encoder.encode(stdout), exitCode, stderr };
}

const ast: RoffAstDocument = {
  schema: "mant.roff-ast/v1",
  engine: { name: "libmandoc", version: "1.14.6" },
  source: { path: "/fixtures/tool.1.gz" },
  macroSet: "man",
  resultLevel: "ok",
  meta: {
    title: "TOOL",
    section: "1",
    volume: null,
    os: null,
    name: "tool",
    aliasTarget: null,
    hasBody: true,
  },
  root: {
    kind: "root",
    loc: { line: 0, column: 1 },
    flags: { generated: false, sentenceEnd: false, noPrint: false },
    children: [],
  },
};

describe("fetchRoffAst", () => {
  test("locates a source and invokes the bundled sidecar by path", async () => {
    const commands: string[][] = [];
    const fetchRoffAst = createRoffAstFetcher({
      getSidecarPath: () => "/tools/mant-mandoc-json",
      isSidecarAvailable: async () => true,
      runCommand: async (command) => {
        commands.push(command);
        if (command[0] === "man") return result("/fixtures/tool.1.gz\n");
        if (command[0] === "zcat") {
          // Decompressor output is written to a temp file before the sidecar
          // sees it, so the exact bytes do not matter for this test.
          return result(".TH TOOL 1");
        }
        if (command[0] === "/tools/mant-mandoc-json") {
          return result(JSON.stringify(ast), 0, "unsupported roff request\n");
        }
        throw new Error(`unexpected command: ${command.join(" ")}`);
      },
    });

    await expect(fetchRoffAst("tool")).resolves.toEqual({
      document: ast,
      diagnostics: ["unsupported roff request"],
    });
    expect(commands[0]).toEqual(["man", "-w", "tool"]);
    expect(commands[1]).toEqual(["zcat", "/fixtures/tool.1.gz"]);
    expect(commands[2]![0]).toBe("/tools/mant-mandoc-json");
    expect(commands[2]![1]).toMatch(/source\.roff$/);
  });

  test("explains how to build a missing sidecar", async () => {
    const fetchRoffAst = createRoffAstFetcher({
      isSidecarAvailable: async () => false,
      runCommand: async () => {
        throw new Error("sidecar availability should be checked first");
      },
    });

    await expect(fetchRoffAst("tool")).rejects.toThrow("build:mandoc-json");
  });

  test("rejects output outside the sidecar protocol", async () => {
    const fetchRoffAst = createRoffAstFetcher({
      getSidecarPath: () => "/tools/mant-mandoc-json",
      isSidecarAvailable: async () => true,
      runCommand: async (command) => {
        if (command[0] === "man") return result("/fixtures/tool.1\n");
        return result("not json");
      },
    });

    await expect(fetchRoffAst("tool")).rejects.toThrow("invalid JSON");
  });
});
