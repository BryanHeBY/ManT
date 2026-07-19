/**
 * @file Exercises the real staged Rust CLI through the TypeScript process client.
 */

import { describe, expect, test } from "bun:test";
import { createNativeCliClient } from "../../src/native/client";

const nativeCliPath = new URL("../../native/bin/mant-cli", import.meta.url).pathname;
const nativeCliAvailable = Bun.spawnSync(
  [nativeCliPath, "--protocol-version", "--compact"],
  { stdout: "ignore", stderr: "ignore" },
).exitCode === 0;
const localManualAvailable = nativeCliAvailable
  && Bun.spawnSync(["man", "-w", "ls"], {
    stdout: "ignore",
    stderr: "ignore",
  }).exitCode === 0;

const describeNativeCli = nativeCliAvailable ? describe : describe.skip;
const testWithManual = localManualAvailable ? test : test.skip;

describeNativeCli("real native CLI process boundary", () => {
  test("negotiates the exact protocol through an explicit path", async () => {
    const client = createNativeCliClient({
      env: { MANT_CLI_PATH: nativeCliPath },
      which: () => null,
    });

    const protocol = await client.protocol();
    expect(protocol.protocol).toBe("mant.cli/v1");
    expect(protocol.documentSchema).toBe("mant.document/v1");
    expect(protocol.outlineSchema).toBe("mant.outline/v1");
    expect(protocol.excerptSchema).toBe("mant.excerpt/v1");
  });

  testWithManual("returns a validated source-lowered manual", async () => {
    const client = createNativeCliClient({
      env: { MANT_CLI_PATH: nativeCliPath },
      which: () => null,
    });

    const query = await client.query({ topic: "ls" });
    expect(query.schema).toBe("mant.query/v1");
    expect(query.manual?.producer.engine?.name).toBe("libmandoc");
    expect(query.manual?.sections.length).toBeGreaterThan(0);
  });
});
