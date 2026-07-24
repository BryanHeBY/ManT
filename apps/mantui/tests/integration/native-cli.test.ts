/**
 * @file Exercises the real staged Rust mant through the TypeScript process client.
 */

import { describe, expect, test } from "bun:test";
import { createMantClient } from "../../src/native/client";
import { buildPageSearchIndex, queryPageSearchIndex } from "../../src/ui/search";

const mantPath = new URL("../../../../engine/bin/mant", import.meta.url).pathname;

// A missing executable makes spawnSync throw ENOENT rather than return a
// non-zero code, so a bare probe would crash module load before describe.skip
// can guard it. Treat any spawn failure as "unavailable" so the suite skips.
function canRun(command: string[]): boolean {
  try {
    return Bun.spawnSync(command, { stdout: "ignore", stderr: "ignore" })
      .exitCode === 0;
  } catch {
    return false;
  }
}

const mantAvailable = canRun([mantPath, "--protocol-version", "--compact"]);
const localManualAvailable = mantAvailable && canRun(["man", "-w", "ls"]);
const tarManualAvailable = mantAvailable && canRun(["man", "-w", "tar"]);

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
    expect(protocol.protocol).toBe("mant.cli/v3");
    expect(protocol.requestSchema).toBe("mant.request/v3");
    expect(protocol.documentSchema).toBe("mant.document/v3");
    expect(protocol.outlineSchema).toBe("mant.outline/v3");
    expect(protocol.excerptSchema).toBe("mant.excerpt/v3");
    expect(protocol.searchSchema).toBe("mant.search/v2");
  });

  testWithManual("returns a validated manual through the native query pipeline", async () => {
    const client = createMantClient({
      env: { MANT_PATH: mantPath },
      which: () => null,
    });

    const query = await client.query({ input: { kind: "manual", topic: "ls" } });
    expect(query.schema).toBe("mant.query/v3");
    // Host manual sources vary by operating system and distribution. The
    // native query pipeline prefers libmandoc, but may legitimately fall back
    // to groff HTML for source constructs libmandoc cannot lower. Fixed roff
    // fixtures exercise the libmandoc-only path independently.
    expect(query.document?.producer.name).toBe("mant");
    expect(query.document?.sections.length).toBeGreaterThan(0);
  });

  testWithTarManual("passes semantic tar options through the TUI search index", async () => {
    const client = createMantClient({
      env: { MANT_PATH: mantPath },
      which: () => null,
    });

    const query = await client.query({ input: { kind: "manual", topic: "tar" } });
    expect(query.document?.producer.engine?.name).toBe("libmandoc");

    const index = buildPageSearchIndex(query.document?.sections ?? [], query.tldr);
    const match = queryPageSearchIndex(index, "--acls")[0];
    expect(match?.text).toBe("--acls");
    expect(match?.targetPath).toContain(".definition-");
    expect(match?.targetPath).toContain(".term-");
  });
});
