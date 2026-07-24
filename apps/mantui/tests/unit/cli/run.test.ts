/**
 * @file Tests interactive CLI dispatch, native query input, and diagnostics.
 */

import { describe, expect, test } from "bun:test";
import type { MantQueryBundle, NativeQueryRequest } from "../../../src/native";
import { runCli } from "../../../src/cli/run";

const result: MantQueryBundle = {
  schema: "mant.query/v3",
  label: "git",
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

describe("interactive CLI execution", () => {
  test("prints help successfully and performs no native query", async () => {
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
    expect(output.stdout[0]).toContain("Usage:\n  mantui <topic|markdown>");
    expect(output.stderr).toEqual([]);
  });

  test("reports usage failures with exit code 2 and no stack", async () => {
    const output = captureOutput();
    const exitCode = await runCli(["--wat"], output.dependencies);

    expect(exitCode).toBe(2);
    expect(output.stdout).toEqual([]);
    expect(output.stderr[0]).toContain("non-interactive output is provided by mant");
    expect(output.stderr[0]).not.toContain(" at ");
  });

  test("checks for an interactive terminal before starting native work", async () => {
    const output = captureOutput();
    let queried = false;
    const exitCode = await runCli(["git"], {
      ...output.dependencies,
      isInteractive: () => false,
      query: async () => {
        queried = true;
        return result;
      },
    });

    expect(exitCode).toBe(1);
    expect(queried).toBeFalse();
    expect(output.stderr).toEqual([
      "mantui: interactive view requires a terminal; use mant for Markdown or JSON output",
    ]);
  });

  test("forwards the closed topic, section, and renderer policy then starts the TUI", async () => {
    const output = captureOutput();
    let request: NativeQueryRequest | undefined;
    let received: MantQueryBundle | undefined;
    const exitCode = await runCli([
      "printf",
      "--section",
      "3",
      "--force-groff",
    ], {
      ...output.dependencies,
      isInteractive: () => true,
      query: async (nativeRequest) => {
        request = nativeRequest;
        return result;
      },
      runTui: async (queryResult) => {
        received = queryResult;
      },
    });

    expect(exitCode).toBe(0);
    expect(request).toEqual({
      input: { kind: "manual", topic: "printf", section: "3" },
      forceGroff: true,
    });
    expect(received).toBe(result);
    expect(output.stderr).toEqual([]);
  });

  test("forwards a Markdown path through the closed native request", async () => {
    const output = captureOutput();
    let request: NativeQueryRequest | undefined;
    const exitCode = await runCli(["docs/guide.md"], {
      ...output.dependencies,
      isInteractive: () => true,
      query: async (nativeRequest) => {
        request = nativeRequest;
        return result;
      },
      runTui: async () => {},
    });

    expect(exitCode).toBe(0);
    expect(request).toEqual({
      input: { kind: "markdown-file", path: "docs/guide.md" },
    });
    expect(output.stderr).toEqual([]);
  });

  test("reports native operational failures without exposing their stack", async () => {
    const output = captureOutput();
    const exitCode = await runCli(["missing"], {
      ...output.dependencies,
      isInteractive: () => true,
      query: async () => {
        throw new Error("no readable manual content was found for 'missing'");
      },
    });

    expect(exitCode).toBe(1);
    expect(output.stderr).toEqual([
      "mantui: no readable manual content was found for 'missing'",
    ]);
    expect(output.stderr[0]).not.toContain("run.test.ts");
  });

  test("keeps full stack diagnostics behind MANT_DEBUG", async () => {
    const output = captureOutput();
    await runCli(["missing"], {
      ...output.dependencies,
      env: { MANT_DEBUG: "1" },
      isInteractive: () => true,
      query: async () => {
        throw new Error("debug failure");
      },
    });

    expect(output.stderr[0]).toContain("Error: debug failure");
    expect(output.stderr[0]).toContain("run.test.ts");
  });
});
