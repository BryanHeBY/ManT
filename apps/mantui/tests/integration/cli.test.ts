/**
 * @file Exercises the real executable entry point so Bun stack traces cannot
 * accidentally leak back in through top-level wiring changes.
 */

import { describe, expect, test } from "bun:test";
import { join } from "node:path";

interface CliProcessResult {
  stdout: string;
  stderr: string;
  exitCode: number;
}

async function invokeCli(...args: string[]): Promise<CliProcessResult> {
  const entry = join(import.meta.dir, "../../src/mantui.ts");
  const process = Bun.spawn([processExecPath(), entry, ...args], {
    stdout: "pipe",
    stderr: "pipe",
    env: {
      ...globalThis.process.env,
      MANT_DEBUG: "",
      MANT_TLDR_DIR: join(import.meta.dir, ".missing-tldr-cache"),
    },
  });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(process.stdout).text(),
    new Response(process.stderr).text(),
    process.exited,
  ]);
  return { stdout, stderr, exitCode };
}

// Kept behind a function so invoking the entry reads clearly in the test host.
function processExecPath(): string {
  return globalThis.process.execPath;
}

describe("real CLI entry point", () => {
  test("prints either help alias without loading or starting the TUI", async () => {
    const [short, long] = await Promise.all([
      invokeCli("-h"),
      invokeCli("--help"),
    ]);

    expect(short.exitCode).toBe(0);
    expect(short.stdout).toContain("Usage:");
    expect(short.stdout).toContain("mant <topic>");
    expect(short.stdout).toContain("--explain=--option");
    expect(short.stdout).not.toContain("--roff-ast");
    expect(short.stderr).toBe("");
    expect(long).toEqual(short);
  });

  test("reports unknown options without Bun source excerpts", async () => {
    const result = await invokeCli("--definitely-unknown");

    expect(result.exitCode).toBe(2);
    expect(result.stderr).toContain("mantui: unknown option '--definitely-unknown'");
    expect(result.stderr).not.toContain("src/mantui.ts:");
    expect(result.stderr).not.toContain(" at main");
  });

  test("redirected TUI use points callers to mant without loading native code", async () => {
    const result = await invokeCli("__mant_missing_topic_7f93c1__");

    expect(result.exitCode).toBe(1);
    expect(result.stderr).toContain("use mant for Markdown or JSON output");
    expect(result.stderr).not.toContain("src/");
    expect(result.stderr).not.toContain("mant was not found");
  });
});
