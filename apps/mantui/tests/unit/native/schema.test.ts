/**
 * @file Verifies the TypeScript guard for Rust's shared query contract.
 */

import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import {
  decodeMantQuery,
  decodeNativeCliProtocol,
} from "../../../src/native/schema";

const fixturePath = join(import.meta.dir, "../../../../../tests/contracts/minimal-query-v3.json");

describe("native query schema", () => {
  test("accepts only the exact native CLI protocol and contract versions", () => {
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
    expect(decodeNativeCliProtocol(protocol).protocol).toBe("mant.cli/v3");
    expect(() => decodeNativeCliProtocol(protocol.replace("mant.cli/v3", "mant.cli/v1")))
      .toThrow("expected 'mant.cli/v3'");
  });

  test("decodes the shared Rust query fixture", async () => {
    const query = decodeMantQuery(await Bun.file(fixturePath).text());

    expect(query.schema).toBe("mant.query/v3");
    expect(query.document?.schema).toBe("mant.document/v3");
    expect(query.document?.sections[0]?.title).toBe("NAME");
    expect(query.document?.sections[0]?.blocks[0]).toMatchObject({
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
    expect(query.document?.sections[1]?.blocks[0]).toMatchObject({
      type: "paragraph",
      children: expect.arrayContaining([{ type: "anchor", id: "all-option" }]),
    });
    expect(query.document?.sections[1]?.blocks[1]).toMatchObject({
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

  test("accepts Markdown root blocks, thematic breaks, and quick-reference sections", async () => {
    const fixture = JSON.parse(await Bun.file(fixturePath).text());
    fixture.document.source = { format: "markdown", path: "guide.md" };
    fixture.document.blocks = [
      {
        type: "paragraph",
        children: [{ type: "text", value: "Document preface." }],
      },
      { type: "thematic-break" },
    ];
    fixture.document.sections[0].role = "quick-reference";

    const query = decodeMantQuery(JSON.stringify(fixture));

    expect(query.document?.source.format).toBe("markdown");
    expect(query.document?.blocks?.[1]).toEqual({ type: "thematic-break" });
    expect(query.document?.sections[0]?.role).toBe("quick-reference");
  });

  test("rejects incompatible schema versions", async () => {
    const fixture = await Bun.file(fixturePath).text();

    expect(() => decodeMantQuery(fixture.replace("mant.query/v3", "mant.query/v1")))
      .toThrow("expected 'mant.query/v3'");
  });

  test("rejects malformed nested nodes before they reach React", async () => {
    const fixture = JSON.parse(await Bun.file(fixturePath).text());
    fixture.document.sections[0].blocks[0].children[0].type = "mystery-style";

    expect(() => decodeMantQuery(JSON.stringify(fixture)))
      .toThrow("unknown inline type 'mystery-style'");
  });

  test("rejects an unknown semantic definition role", async () => {
    const fixture = JSON.parse(await Bun.file(fixturePath).text());
    fixture.document.sections[1].blocks[1].items[0].identity.role = "mystery";

    expect(() => decodeMantQuery(JSON.stringify(fixture)))
      .toThrow("expected one of option, command, environment-variable");
  });
});
