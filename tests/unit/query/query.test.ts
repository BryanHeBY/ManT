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
});
