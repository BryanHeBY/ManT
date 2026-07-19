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
  const entry = join(import.meta.dir, "../../src/cli.ts");
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
  test("prints help without loading or starting the TUI", async () => {
    const result = await invokeCli("--help");

    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("Usage:");
    expect(result.stdout).toContain("--md");
    expect(result.stdout).toContain("--markdown");
    expect(result.stderr).toBe("");
  });

  test("reports unknown options without Bun source excerpts", async () => {
    const result = await invokeCli("--definitely-unknown");

    expect(result.exitCode).toBe(2);
    expect(result.stderr).toContain("mant: unknown option '--definitely-unknown'");
    expect(result.stderr).not.toContain("src/cli.ts:");
    expect(result.stderr).not.toContain(" at main");
  });

  test("reports an unknown topic without a renderer stack", async () => {
    const result = await invokeCli("__mant_missing_topic_7f93c1__", "--json");

    expect(result.exitCode).toBe(1);
    expect(result.stderr).toStartWith("mant: ");
    expect(result.stderr).not.toContain("src/core/");
    expect(result.stderr).not.toContain(" at renderWithMan");
  });
});
