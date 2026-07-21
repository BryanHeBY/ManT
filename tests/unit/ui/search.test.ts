/**
 * @file Verifies immutable leaf indexing, occurrence counts, and structural
 * target paths independently of the terminal renderer.
 */

import { describe, expect, test } from "bun:test";
import type { MantSection } from "../../../src/native";
import {
  buildPageSearchIndex,
  queryPageSearchIndex,
  searchPath,
} from "../../../src/ui/search";

const sections: MantSection[] = [{
  id: "options",
  title: "OPTIONS",
  blocks: [{
    type: "definition-list",
    compact: true,
    items: [
      {
        terms: [[{ type: "strong", children: [{ type: "text", value: "--alpha" }] }]],
        description: [{
          type: "paragraph",
          children: [
            { type: "text", value: "Needle crosses " },
            { type: "emphasis", children: [{ type: "text", value: "formatting" }] },
            { type: "text", value: ". Needle again." },
          ],
        }],
      },
      {
        terms: [[{ type: "text", value: "--needle-option" }]],
        description: [{
          type: "paragraph",
          children: [{ type: "text", value: "Second definition." }],
        }],
      },
    ],
  }],
  children: [],
}];

describe("page search index", () => {
  test("builds frozen leaf records instead of aggregate definition-list records", () => {
    const index = buildPageSearchIndex(sections, undefined);

    expect(Object.isFrozen(index)).toBe(true);
    expect(Object.isFrozen(index.records)).toBe(true);
    expect(index.records.map((record) => record.text)).toEqual([
      "OPTIONS",
      "--alpha",
      "Needle crosses formatting. Needle again.",
      "--needle-option",
      "Second definition.",
    ]);
  });

  test("returns every occurrence with an exact nested render path", () => {
    const matches = queryPageSearchIndex(buildPageSearchIndex(sections, undefined), "needle");
    const firstDescription = searchPath.block(
      searchPath.definition(searchPath.block("", 0), 0),
      0,
    );
    const secondTerm = searchPath.term(
      searchPath.definition(searchPath.block("", 0), 1),
      0,
    );

    expect(matches).toHaveLength(3);
    expect(matches.map((match) => match.targetPath)).toEqual([
      firstDescription,
      firstDescription,
      secondTerm,
    ]);
    expect(matches.map((match) => match.range)).toEqual([
      { start: 0, end: 6 },
      { start: 27, end: 33 },
      { start: 2, end: 8 },
    ]);
  });

  test("matches visible text across inline formatting boundaries", () => {
    const matches = queryPageSearchIndex(
      buildPageSearchIndex(sections, undefined),
      "crosses formatting",
    );

    expect(matches).toHaveLength(1);
    expect(matches[0]?.range).toEqual({ start: 7, end: 25 });
  });

  test("keeps ranges in source positions when case folding changes length", () => {
    // "İ".toLocaleLowerCase() expands to two code units, so a naive folded
    // offset would drift right of the source character the renderer draws.
    const widening: MantSection[] = [{
      id: "options",
      title: "OPTIONS",
      blocks: [{
        type: "paragraph",
        children: [{ type: "text", value: "İstanbul needle here" }],
      }],
      children: [],
    }];
    const index = buildPageSearchIndex(widening, undefined);
    const record = index.records.find((entry) => entry.text.includes("needle"));
    const [match] = queryPageSearchIndex(index, "needle");

    // The record still folds for matching, but retains a source-offset map.
    expect(record?.normalizedText.length).toBe(record!.text.length + 1);
    expect(record?.sourceOffsets).toBeDefined();
    // Source index of "needle" is 9 ("İstanbul " is nine source characters).
    expect(match?.range).toEqual({ start: 9, end: 15 });
    expect(match?.text.slice(match.range.start, match.range.end)).toBe("needle");
  });

  test("matches a widening character itself at its source position", () => {
    const widening: MantSection[] = [{
      id: "options",
      title: "OPTIONS",
      blocks: [{
        type: "paragraph",
        children: [{ type: "text", value: "aİb" }],
      }],
      children: [],
    }];
    const index = buildPageSearchIndex(widening, undefined);
    const [match] = queryPageSearchIndex(index, "İ");

    expect(match?.range).toEqual({ start: 1, end: 2 });
    expect(match?.text.slice(match.range.start, match.range.end)).toBe("İ");
  });

  test("groups adjacent prose exactly like the shared terminal text buffer", () => {
    const grouped: MantSection[] = [{
      id: "description",
      title: "DESCRIPTION",
      blocks: [
        { type: "paragraph", children: [{ type: "text", value: "First paragraph." }] },
        { type: "paragraph", children: [{ type: "text", value: "Needle paragraph." }] },
        {
          type: "paragraph",
          layout: { indentColumns: 4 },
          children: [{ type: "text", value: "Indented paragraph." }],
        },
      ],
      children: [],
    }];
    const index = buildPageSearchIndex(grouped, undefined);
    const match = queryPageSearchIndex(index, "needle")[0];

    expect(index.records.map((record) => record.text)).toEqual([
      "DESCRIPTION",
      "First paragraph.\nNeedle paragraph.",
      "Indented paragraph.",
    ]);
    expect(match?.targetPath).toBe(searchPath.block("", 0));
    expect(match?.range).toEqual({ start: 17, end: 23 });
  });

  test("indexes clickable reference fragments as separate text targets", () => {
    const linked: MantSection[] = [{
      id: "see-also",
      title: "SEE ALSO",
      blocks: [{
        type: "paragraph",
        children: [
          { type: "text", value: "Read " },
          {
            type: "section-reference",
            target: "options",
            children: [{ type: "text", value: "the options" }],
          },
          { type: "text", value: " for details." },
        ],
      }],
      children: [],
    }];
    const index = buildPageSearchIndex(linked, undefined);

    expect(index.records.map((record) => record.targetPath)).toEqual([
      "heading",
      "block-0.inline-0",
      "block-0.inline-1",
      "block-0.inline-2",
    ]);
    expect(queryPageSearchIndex(index, "options")[0]?.targetPath)
      .toBe("block-0.inline-1");
  });
});
