/**
 * @file Tests CLI dispatch, exit codes, and concise user-facing diagnostics.
 */

import { describe, expect, test } from "bun:test";
import type { QueryResult } from "../../../src/query";
import { runCli } from "../../../src/cli/run";

const result: QueryResult = {
  topic: "git",
  sections: [],
};

function captureOutput() {
  const stdout: string[] = [];
  const stderr: string[] = [];
  return {
    stdout,
    stderr,
    dependencies: {
      stdout: (message: string) => stdout.push(message),
      stderr: (message: string) => stderr.push(message),
      env: {},
    },
  };
}

describe("CLI execution", () => {
  test("prints help successfully and performs no query", async () => {
    const output = captureOutput();
    let queried = false;

    const exitCode = await runCli(["--help"], {
      ...output.dependencies,
      query: async () => {
        queried = true;
        return result;
      },
    });

    expect(exitCode).toBe(0);
    expect(queried).toBeFalse();
    expect(output.stdout[0]).toContain("Usage:");
    expect(output.stderr).toEqual([]);
  });

  test("reports usage failures with exit code 2 and no stack", async () => {
    const output = captureOutput();
    const exitCode = await runCli(["--wat"], output.dependencies);

    expect(exitCode).toBe(2);
    expect(output.stdout).toEqual([]);
    expect(output.stderr).toEqual([
      "mant: unknown option '--wat'\nTry 'mant --help' for more information.",
    ]);
    expect(output.stderr[0]).not.toContain(" at ");
  });

  test("reports operational failures without exposing their stack", async () => {
    const output = captureOutput();
    const exitCode = await runCli(["missing", "--json"], {
      ...output.dependencies,
      query: async () => {
        throw new Error("No manual entry for missing");
      },
    });

    expect(exitCode).toBe(1);
    expect(output.stderr).toEqual(["mant: No manual entry for missing"]);
    expect(output.stderr[0]).not.toContain("run.test.ts");
  });

  test("keeps full stack diagnostics behind MANT_DEBUG", async () => {
    const output = captureOutput();
    await runCli(["missing", "--json"], {
      ...output.dependencies,
      env: { MANT_DEBUG: "1" },
      query: async () => {
        throw new Error("debug failure");
      },
    });

    expect(output.stderr[0]).toContain("Error: debug failure");
    expect(output.stderr[0]).toContain("run.test.ts");
  });

  test("writes structured query results without starting the TUI", async () => {
    const output = captureOutput();
    let tuiStarted = false;
    const exitCode = await runCli(["git", "--json"], {
      ...output.dependencies,
      query: async () => result,
      runTui: async () => { tuiStarted = true; },
    });

    expect(exitCode).toBe(0);
    expect(JSON.parse(output.stdout[0]!)).toEqual(result);
    expect(tuiStarted).toBeFalse();
  });

  test("explains how to use Mant when no interactive terminal is available", async () => {
    const output = captureOutput();
    let tuiStarted = false;
    let queried = false;
    const exitCode = await runCli(["git"], {
      ...output.dependencies,
      query: async () => { queried = true; return result; },
      isInteractive: () => false,
      runTui: async () => { tuiStarted = true; },
    });

    expect(exitCode).toBe(1);
    expect(queried).toBeFalse();
    expect(tuiStarted).toBeFalse();
    expect(output.stderr[0]).toBe(
      "mant: interactive view requires a terminal; use --json for redirected or scripted output",
    );
  });

  test("starts the TUI only after a successful interactive query", async () => {
    const output = captureOutput();
    let received: QueryResult | undefined;
    const exitCode = await runCli(["git"], {
      ...output.dependencies,
      query: async () => result,
      isInteractive: () => true,
      runTui: async (queryResult) => { received = queryResult; },
    });

    expect(exitCode).toBe(0);
    expect(received).toBe(result);
    expect(output.stderr).toEqual([]);
  });

  test("runs the tldr update action and reports its revision", async () => {
    const output = captureOutput();
    const exitCode = await runCli(["--update-tldr"], {
      ...output.dependencies,
      updateTldrCache: async () => ({
        action: "updated",
        cacheDir: "/cache/tldr",
        revision: "abc123",
      }),
    });

    expect(exitCode).toBe(0);
    expect(output.stdout).toEqual(["tldr cache updated: /cache/tldr (abc123)"]);
  });
});
