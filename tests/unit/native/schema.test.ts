/**
 * @file Verifies the TypeScript guard for Rust's shared query contract.
 */

import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import {
  decodeMantQuery,
  decodeNativeCliProtocol,
} from "../../../src/native/schema";

const fixturePath = join(import.meta.dir, "../../contracts/minimal-query-v1.json");

describe("native query schema", () => {
  test("accepts only the exact native CLI protocol and contract versions", () => {
    const protocol = JSON.stringify({
      protocol: "mant.cli/v1",
      nativeApiVersion: "1",
      querySchema: "mant.query/v1",
      documentSchema: "mant.document/v1",
    });
    expect(decodeNativeCliProtocol(protocol).protocol).toBe("mant.cli/v1");
    expect(() => decodeNativeCliProtocol(protocol.replace("mant.cli/v1", "mant.cli/v2")))
      .toThrow("expected 'mant.cli/v1'");
  });

  test("decodes the shared Rust query fixture", async () => {
    const query = decodeMantQuery(await Bun.file(fixturePath).text());

    expect(query.schema).toBe("mant.query/v1");
    expect(query.manual?.schema).toBe("mant.document/v1");
    expect(query.manual?.sections[0]?.title).toBe("NAME");
    expect(query.tldr?.examples[0]?.commandParts[0]).toEqual({
      type: "text",
      value: "ls --all",
    });
  });

  test("rejects incompatible schema versions", async () => {
    const fixture = await Bun.file(fixturePath).text();

    expect(() => decodeMantQuery(fixture.replace("mant.query/v1", "mant.query/v2")))
      .toThrow("expected 'mant.query/v1'");
  });

  test("rejects malformed nested nodes before they reach React", async () => {
    const fixture = JSON.parse(await Bun.file(fixturePath).text());
    fixture.manual.sections[0].blocks[0].children[0].type = "mystery-style";

    expect(() => decodeMantQuery(JSON.stringify(fixture)))
      .toThrow("unknown inline type 'mystery-style'");
  });
});
