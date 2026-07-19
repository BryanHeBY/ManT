/**
 * @file Verifies CommonMark serialization from Mant's normalized document AST.
 */

import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import { parseManHtml } from "../../../src/core";
import { renderMarkdown } from "../../../src/output";
import type { QueryResult } from "../../../src/query";

describe("Markdown output", () => {
  test("renders the combined TLDR and man page in document order", () => {
    const markdown = renderMarkdown({
      topic: "ls",
      sections: [
        {
          id: "name",
          title: "NAME",
          level: 2,
          blocks: [{
            type: "paragraph",
            indent: 0,
            children: [{ type: "text", content: "ls - list files" }],
          }],
          children: [],
        },
        {
          id: "synopsis",
          title: "SYNOPSIS",
          level: 2,
          blocks: [{
            type: "paragraph",
            indent: 0,
            children: [{
              type: "bold",
              children: [{ type: "text", content: "ls" }],
            }],
          }],
          children: [],
        },
      ],
      tldr: {
        title: "ls",
        description: ["List directory contents."],
        examples: [{
          description: "List hidden entries",
          command: "ls {{[-a|--all]}}",
          commandParts: [
            { type: "text", content: "ls " },
            { type: "placeholder", content: "--all" },
          ],
        }],
        platform: "common",
        language: "en",
        sourcePath: "/cache/tldr/ls.md",
        moreInformation: "https://example.com/manual_page.html.",
      },
    });

    expect(markdown).toStartWith("# ls\n");
    expect(markdown.indexOf("## TLDR")).toBeLessThan(markdown.indexOf("## NAME"));
    expect(markdown).toContain("### Examples");
    expect(markdown).toContain("```sh\nls --all\n```");
    expect(markdown).not.toContain("{{[-a|--all]}}");
    expect(markdown).toContain("**More information:** <https://example.com/manual_page.html>.");
    expect(markdown).toContain("*tldr-pages · CC BY 4.0 · common · en*");
    expect(markdown).toContain("## SYNOPSIS\n\n**ls**");
    expect(markdown.endsWith("\n")).toBeFalse();
  });

  test("preserves inline semantics, lists, definitions, and nested headings", () => {
    const result: QueryResult = {
      topic: "demo * command",
      sections: [{
        id: "options",
        title: "OPTIONS",
        level: 2,
        blocks: [
          {
            type: "paragraph",
            indent: 4,
            children: [
              { type: "bold", children: [{ type: "text", content: " demo " }] },
              { type: "text", content: "reads " },
              { type: "italic", children: [{ type: "text", content: "files" }] },
              { type: "text", content: " with " },
              { type: "code", children: [{ type: "text", content: "a`b" }] },
              { type: "break" },
              { type: "text", content: " a second line " },
              { type: "text", content: "See <<https://example.com/docs>>. " },
            ],
          },
          {
            type: "list",
            indent: 8,
            items: [[{ type: "text", content: "first item" }]],
          },
          {
            type: "definition-list",
            indent: 8,
            items: [{
              terms: [
                [{ type: "bold", children: [{ type: "text", content: "-a" }] }],
                [{ type: "bold", children: [{ type: "text", content: "--all" }] }],
              ],
              description: [{ type: "text", content: "Show all entries." }],
            }],
          },
        ],
        children: [{
          id: "details",
          title: "DETAILS",
          level: 3,
          blocks: [],
          children: [],
        }],
      }],
    };

    const markdown = renderMarkdown(result);

    expect(markdown).toStartWith("# demo \\* command");
    expect(markdown).toContain("## OPTIONS");
    expect(markdown).toContain("### DETAILS");
    expect(markdown).toContain("**demo** reads *files* with ``a`b``");
    expect(markdown).toContain("a second line");
    expect(markdown).toContain("See <https://example.com/docs>.");
    expect(markdown).not.toContain("&#x20;");
    expect(markdown).toContain("- first item");
    expect(markdown).toContain("**-a**\\\n  **--all**");
    expect(markdown).toContain("Show all entries.");
    expect(markdown).not.toContain("    **demo**");
  });

  test("chooses a safe fence and flattens formatting inside pre blocks", () => {
    const result: QueryResult = {
      topic: "demo",
      sections: [{
        id: "example",
        title: "EXAMPLE",
        level: 2,
        blocks: [{
          type: "pre",
          indent: 4,
          children: [
            { type: "text", content: "before ``` marker" },
            { type: "break" },
            { type: "bold", children: [{ type: "text", content: "after" }] },
          ],
        }],
        children: [],
      }],
    };

    const markdown = renderMarkdown(result);

    expect(markdown).toContain("````\nbefore ``` marker\nafter\n````");
    expect(markdown).not.toContain("**after**");
  });

  test("serializes a large real renderer fixture without leaking HTML", async () => {
    const html = await Bun.file(join(
      import.meta.dir,
      "../../fixtures/man-pages/mandoc-git.html",
    )).text();
    const markdown = renderMarkdown({
      topic: "git",
      sections: parseManHtml(html),
    });

    expect(markdown).toStartWith("# git\n");
    expect(markdown).toContain("## NAME");
    expect(markdown).toContain("## OPTIONS");
    expect(markdown).toContain("### Git Diffs");
    expect(markdown).toContain("**git**");
    expect(markdown).toContain("```\n");
    expect(markdown).not.toMatch(/<(?:br|i|b|pre)\b/i);
  });
});
