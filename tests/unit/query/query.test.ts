/**
 * @file Tests query composition of authoritative man and cached tldr data.
 */

import { describe, expect, test } from "bun:test";
import { createQuery } from "../../../src/query";
import type { TldrPage } from "../../../src/tldr";

const tldr: TldrPage = {
  title: "ls",
  description: ["List directory contents."],
  examples: [],
  platform: "common",
  language: "en",
  sourcePath: "/cache/pages/common/ls.md",
};

describe("query with cached tldr pages", () => {
  test("rejects an empty topic before starting cache or man work", async () => {
    let fetched = false;
    const query = createQuery({
      fetchManHtml: async () => { fetched = true; return ""; },
      fetchTldrPage: async () => { fetched = true; return null; },
    });

    await expect(query({ topic: "   " })).rejects.toThrow("manual topic must not be empty");
    expect(fetched).toBeFalse();
  });

  test("places an available quick reference alongside parsed man sections", async () => {
    const query = createQuery({
      fetchManHtml: async () => "<html />",
      parseManHtml: () => [{ id: "name", title: "NAME", level: 2, blocks: [], children: [] }],
      fetchTldrPage: async () => tldr,
    });

    await expect(query({ topic: "ls" })).resolves.toEqual({
      topic: "ls",
      section: undefined,
      sections: [{ id: "name", title: "NAME", level: 2, blocks: [], children: [] }],
      tldr,
    });
  });

  test("still returns a cached quick reference when no local man page exists", async () => {
    const query = createQuery({
      fetchManHtml: async () => { throw new Error("man page not found"); },
      fetchTldrPage: async () => tldr,
    });

    const result = await query({ topic: "ls" });
    expect(result.sections).toEqual([]);
    expect(result.tldr).toBe(tldr);
  });

  test("does not open an empty UI when neither parser nor tldr has content", async () => {
    const query = createQuery({
      fetchManHtml: async () => "<html />",
      parseManHtml: () => [],
      fetchTldrPage: async () => null,
    });

    await expect(query({ topic: "broken" })).rejects.toThrow(
      "no readable manual content was found for 'broken'",
    );
  });
});
