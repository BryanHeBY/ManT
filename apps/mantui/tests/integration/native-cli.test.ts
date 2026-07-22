/**
 * @file Exercises the real staged Rust mant through the TypeScript process client.
 */

import { describe, expect, test } from "bun:test";
import { createMantClient } from "../../src/native/client";
import { buildPageSearchIndex, queryPageSearchIndex } from "../../src/ui/search";

const mantPath = new URL("../../../../engine/bin/mant", import.meta.url).pathname;
const mantAvailable = Bun.spawnSync(
  [mantPath, "--protocol-version", "--compact"],
  { stdout: "ignore", stderr: "ignore" },
).exitCode === 0;
const localManualAvailable = mantAvailable
  && Bun.spawnSync(["man", "-w", "ls"], {
    stdout: "ignore",
    stderr: "ignore",
  }).exitCode === 0;
const tarManualAvailable = mantAvailable
  && Bun.spawnSync(["man", "-w", "tar"], {
    stdout: "ignore",
    stderr: "ignore",
  }).exitCode === 0;

const describeMant = mantAvailable ? describe : describe.skip;
const testWithManual = localManualAvailable ? test : test.skip;
const testWithTarManual = tarManualAvailable ? test : test.skip;

describeMant("real native mant process boundary", () => {
  test("negotiates the exact protocol through an explicit path", async () => {
    const client = createMantClient({
      env: { MANT_PATH: mantPath },
      which: () => null,
    });

    const protocol = await client.protocol();
    expect(protocol.protocol).toBe("mant.cli/v2");
    expect(protocol.requestSchema).toBe("mant.request/v2");
    expect(protocol.documentSchema).toBe("mant.document/v2");
    expect(protocol.outlineSchema).toBe("mant.outline/v2");
    expect(protocol.excerptSchema).toBe("mant.excerpt/v2");
    expect(protocol.searchSchema).toBe("mant.search/v1");
  });

  testWithManual("returns a validated manual through the native query pipeline", async () => {
    const client = createMantClient({
      env: { MANT_PATH: mantPath },
      which: () => null,
    });

    const query = await client.query({ topic: "ls" });
    expect(query.schema).toBe("mant.query/v2");
    // Host manual sources vary by operating system and distribution. The
    // native query pipeline prefers libmandoc, but may legitimately fall back
    // to groff HTML for source constructs libmandoc cannot lower. Fixed roff
    // fixtures exercise the libmandoc-only path independently.
    expect(query.manual?.producer.name).toBe("mant");
    expect(query.manual?.sections.length).toBeGreaterThan(0);
  });

  testWithTarManual("passes semantic tar options through the TUI search index", async () => {
    const client = createMantClient({
      env: { MANT_PATH: mantPath },
      which: () => null,
    });

    const query = await client.query({ topic: "tar" });
    expect(query.manual?.producer.engine?.name).toBe("libmandoc");

    const index = buildPageSearchIndex(query.manual?.sections ?? [], query.tldr);
    const match = queryPageSearchIndex(index, "--acls")[0];
    expect(match?.text).toBe("--acls");
    expect(match?.targetPath).toContain(".definition-");
    expect(match?.targetPath).toContain(".term-");
  });
});
