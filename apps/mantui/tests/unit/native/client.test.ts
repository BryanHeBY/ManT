/**
 * @file Verifies mant resolution, handshake, and closed query transport.
 */

import { describe, expect, test } from "bun:test";
import {
  createMantClient,
  resolveMantPath,
} from "../../../src/native/client";
import type {
  CommandOptions,
  CommandResult,
  CommandRunner,
} from "../../../src/native/process";

const encode = (value: string) => new TextEncoder().encode(value);

function result(stdout: string, stderr = "", exitCode = 0): CommandResult {
  return { stdout: encode(stdout), stderr, exitCode };
}

const protocol = JSON.stringify({
  protocol: "mant.cli/v3",
  nativeApiVersion: "3",
  requestSchema: "mant.request/v3",
  querySchema: "mant.query/v3",
  documentSchema: "mant.document/v3",
  outlineSchema: "mant.outline/v3",
  excerptSchema: "mant.excerpt/v3",
  searchSchema: "mant.search/v2",
});

describe("native mant client", () => {
  test("uses MANT_PATH before PATH and rejects an empty override", () => {
    expect(resolveMantPath(
      { MANT_PATH: " /opt/mant/bin/mant " },
      () => "/usr/bin/mant",
    )).toBe("/opt/mant/bin/mant");
    expect(() => resolveMantPath({ MANT_PATH: "  " }, () => null))
      .toThrow("MANT_PATH is set but empty");
  });

  test("uses PATH and gives source checkouts an actionable missing-binary error", () => {
    expect(resolveMantPath({}, (command) => `/tools/${command}`))
      .toBe("/tools/mant");
    expect(() => resolveMantPath({}, () => null))
      .toThrow("bun run dev -- <topic>");
  });

  test("probes once, sends closed JSON on stdin, and validates every response", async () => {
    const calls: Array<{
      command: string[];
      options: CommandOptions | undefined;
    }> = [];
    const runCommand: CommandRunner = async (command, options) => {
      calls.push({ command, options });
      if (command.includes("--protocol-version")) return result(protocol);
      return result(JSON.stringify({
        schema: "mant.query/v3",
        label: "git",
      }));
    };
    const client = createMantClient({
      env: { MANT_PATH: "/tools/mant" },
      which: () => {
        throw new Error("PATH must not be consulted");
      },
      runCommand,
    });

    const input = { kind: "manual", topic: "git", section: "1" } as const;
    const first = await client.query({ input });
    const second = await client.query({
      input,
      forceLibmandoc: true,
    });
    const third = await client.query({
      input,
      forceGroff: true,
    });
    expect(first.schema).toBe("mant.query/v3");
    expect(second.label).toBe("git");
    expect(third.label).toBe("git");
    expect(calls.map((call) => call.command)).toEqual([
      ["/tools/mant", "--protocol-version", "--compact"],
      ["/tools/mant", "--request-json", "--format", "json", "--compact"],
      [
        "/tools/mant",
        "--request-json",
        "--force-libmandoc",
        "--format",
        "json",
        "--compact",
      ],
      [
        "/tools/mant",
        "--request-json",
        "--force-groff",
        "--format",
        "json",
        "--compact",
      ],
    ]);
    expect(new TextDecoder().decode(calls[1]?.options?.stdin))
      .toBe('{"schema":"mant.request/v3","input":{"kind":"manual","topic":"git","section":"1"},"view":{"kind":"full"}}');
  });

  test("rejects incompatible binaries before issuing a query", async () => {
    let calls = 0;
    const client = createMantClient({
      env: { MANT_PATH: "/tools/mant" },
      runCommand: async () => {
        calls += 1;
        return result(protocol.replace("mant.cli/v3", "mant.cli/v1"));
      },
    });

    await expect(client.query({ input: { kind: "manual", topic: "git" } }))
      .rejects.toThrow("expected 'mant.cli/v3'");
    expect(calls).toBe(1);
  });

  test("turns native stderr into a concise host error", async () => {
    const client = createMantClient({
      env: { MANT_PATH: "/tools/mant" },
      runCommand: async (command) => command.includes("--protocol-version")
        ? result(protocol)
        : result("", "mant: no readable manual content was found for 'missing'\n", 1),
    });

    await expect(client.query({ input: { kind: "manual", topic: "missing" } }))
      .rejects.toThrow("no readable manual content was found for 'missing'");
  });
});
