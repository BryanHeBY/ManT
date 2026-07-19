/**
 * @file Verifies native CLI resolution, handshake, and closed query transport.
 */

import { describe, expect, test } from "bun:test";
import {
  createNativeCliClient,
  resolveMantCliPath,
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
  protocol: "mant.cli/v1",
  nativeApiVersion: "1",
  querySchema: "mant.query/v1",
  documentSchema: "mant.document/v1",
});

describe("native mant-cli client", () => {
  test("uses MANT_CLI_PATH before PATH and rejects an empty override", () => {
    expect(resolveMantCliPath(
      { MANT_CLI_PATH: " /opt/mant/bin/mant-cli " },
      () => "/usr/bin/mant-cli",
    )).toBe("/opt/mant/bin/mant-cli");
    expect(() => resolveMantCliPath({ MANT_CLI_PATH: "  " }, () => null))
      .toThrow("MANT_CLI_PATH is set but empty");
  });

  test("uses PATH and gives source checkouts an actionable missing-binary error", () => {
    expect(resolveMantCliPath({}, (command) => `/tools/${command}`))
      .toBe("/tools/mant-cli");
    expect(() => resolveMantCliPath({}, () => null))
      .toThrow("bun run dev -- <topic>");
  });

  test("probes once, sends closed JSON on stdin, and validates every response", async () => {
    const calls: Array<{
      command: string[];
      options: CommandOptions | undefined;
    }> = [];
    const runCommand: CommandRunner = async (command, options) => {
      calls.push({ command, options });
      if (command.includes("protocol-version")) return result(protocol);
      return result(JSON.stringify({
        schema: "mant.query/v1",
        topic: "git",
        section: "1",
      }));
    };
    const client = createNativeCliClient({
      env: { MANT_CLI_PATH: "/tools/mant-cli" },
      which: () => {
        throw new Error("PATH must not be consulted");
      },
      runCommand,
    });

    const first = await client.query({ topic: "git", section: "1" });
    const second = await client.query({ topic: "git", section: "1" });
    expect(first.schema).toBe("mant.query/v1");
    expect(second.topic).toBe("git");
    expect(calls.map((call) => call.command)).toEqual([
      ["/tools/mant-cli", "protocol-version", "--compact"],
      ["/tools/mant-cli", "--request-json", "--json", "--compact"],
      ["/tools/mant-cli", "--request-json", "--json", "--compact"],
    ]);
    expect(new TextDecoder().decode(calls[1]?.options?.stdin))
      .toBe('{"topic":"git","section":"1"}');
  });

  test("rejects incompatible binaries before issuing a query", async () => {
    let calls = 0;
    const client = createNativeCliClient({
      env: { MANT_CLI_PATH: "/tools/mant-cli" },
      runCommand: async () => {
        calls += 1;
        return result(protocol.replace("mant.cli/v1", "mant.cli/v2"));
      },
    });

    await expect(client.query({ topic: "git" }))
      .rejects.toThrow("expected 'mant.cli/v1'");
    expect(calls).toBe(1);
  });

  test("turns native stderr into a concise host error", async () => {
    const client = createNativeCliClient({
      env: { MANT_CLI_PATH: "/tools/mant-cli" },
      runCommand: async (command) => command.includes("protocol-version")
        ? result(protocol)
        : result("", "mant-cli: no readable manual content was found for 'missing'\n", 1),
    });

    await expect(client.query({ topic: "missing" }))
      .rejects.toThrow("no readable manual content was found for 'missing'");
  });
});
