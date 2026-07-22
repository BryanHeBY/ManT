/**
 * @file Verifies the TypeScript guard for Rust's shared query contract.
 */

import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import {
  decodeMantQuery,
  decodeNativeCliProtocol,
} from "../../../src/native/schema";

const fixturePath = join(import.meta.dir, "../../../../../tests/contracts/minimal-query-v2.json");

describe("native query schema", () => {
  test("accepts only the exact native CLI protocol and contract versions", () => {
    const protocol = JSON.stringify({
      protocol: "mant.cli/v2",
      nativeApiVersion: "2",
      requestSchema: "mant.request/v2",
      querySchema: "mant.query/v2",
      documentSchema: "mant.document/v2",
      outlineSchema: "mant.outline/v2",
      excerptSchema: "mant.excerpt/v2",
      searchSchema: "mant.search/v1",
    });
    expect(decodeNativeCliProtocol(protocol).protocol).toBe("mant.cli/v2");
    expect(() => decodeNativeCliProtocol(protocol.replace("mant.cli/v2", "mant.cli/v1")))
      .toThrow("expected 'mant.cli/v2'");
  });

  test("decodes the shared Rust query fixture", async () => {
    const query = decodeMantQuery(await Bun.file(fixturePath).text());

    expect(query.schema).toBe("mant.query/v2");
    expect(query.manual?.schema).toBe("mant.document/v2");
    expect(query.manual?.sections[0]?.title).toBe("NAME");
    expect(query.manual?.sections[0]?.blocks[0]).toMatchObject({
      type: "paragraph",
      children: expect.arrayContaining([
        {
          type: "external-link",
          uri: "https://example.test/ls",
          title: "Project documentation",
          children: [{ type: "text", value: "the project site" }],
        },
        {
          type: "email-link",
          address: "docs@example.test",
          children: [{ type: "text", value: "the documentation team" }],
        },
        {
          type: "section-reference",
          target: "options-1",
          children: [{ type: "text", value: "OPTIONS" }],
        },
      ]),
    });
    expect(query.manual?.sections[1]?.blocks[0]).toMatchObject({
      type: "paragraph",
      children: expect.arrayContaining([{ type: "anchor", id: "all-option" }]),
    });
    expect(query.manual?.sections[1]?.blocks[1]).toMatchObject({
      type: "definition-list",
      items: [{
        identity: {
          id: "almost-all-option",
          role: "option",
          names: ["-A", "--almost-all"],
        },
      }],
    });
    expect(query.tldr?.examples[0]?.commandParts[0]).toEqual({
      type: "text",
      value: "ls --all",
    });
  });

  test("rejects incompatible schema versions", async () => {
    const fixture = await Bun.file(fixturePath).text();

    expect(() => decodeMantQuery(fixture.replace("mant.query/v2", "mant.query/v1")))
      .toThrow("expected 'mant.query/v2'");
  });

  test("rejects malformed nested nodes before they reach React", async () => {
    const fixture = JSON.parse(await Bun.file(fixturePath).text());
    fixture.manual.sections[0].blocks[0].children[0].type = "mystery-style";

    expect(() => decodeMantQuery(JSON.stringify(fixture)))
      .toThrow("unknown inline type 'mystery-style'");
  });

  test("rejects an unknown semantic definition role", async () => {
    const fixture = JSON.parse(await Bun.file(fixturePath).text());
    fixture.manual.sections[1].blocks[1].items[0].identity.role = "mystery";

    expect(() => decodeMantQuery(JSON.stringify(fixture)))
      .toThrow("expected one of option, command, environment-variable");
  });
});
