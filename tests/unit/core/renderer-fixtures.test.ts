/**
 * @file Confirms real renderer fixtures exclude document chrome from sections.
 */

import { describe, expect, test } from "bun:test";
import { parseManHtml } from "../../../src/core/parser";
import type { InlineNode, SectionNode } from "../../../src/core/types";
import { loadManPageFixture } from "../../fixtures/man-pages";

function walkSections(nodes: SectionNode[]): SectionNode[] {
  return nodes.flatMap((node) => [node, ...walkSections(node.children)]);
}

const RENDERER_HTML_TAG = /<\/?(?:i|b|br|font|small|span|a|code|pre|div|section|h[1-6]|p|dl|dt|dd|ul|ol|li|table|tr|td|th|thead|tbody|tfoot|em|strong|tt|u)\b[^>]*>/i;

function containsRendererHtmlTag(nodes: InlineNode[]): boolean {
  return nodes.some((node) => {
    if (node.type === "text") return RENDERER_HTML_TAG.test(node.content);
    if (node.type === "break") return false;
    return containsRendererHtmlTag(node.children);
  });
}

const rendererFixtures = [
  {
    name: "man -Thtml ls",
    fixture: "ls" as const,
    marker: "groff -Thtml",
    requiredTitles: ["NAME", "SYNOPSIS", "DESCRIPTION", "SEE ALSO"],
  },
  {
    name: "mandoc -Thtml ls",
    fixture: "mandoc-ls" as const,
    marker: "manual-text",
    requiredTitles: ["NAME", "SYNOPSIS", "DESCRIPTION", "SEE ALSO"],
  },
  {
    name: "man -Thtml git",
    fixture: "groff-git" as const,
    marker: "groff -Thtml",
    requiredTitles: ["NAME", "SYNOPSIS", "DESCRIPTION", "OPTIONS", "SEE ALSO"],
  },
  {
    name: "mandoc -Thtml git",
    fixture: "mandoc-git" as const,
    marker: "manual-text",
    requiredTitles: ["NAME", "SYNOPSIS", "DESCRIPTION", "OPTIONS", "SEE ALSO"],
  },
];

describe("real renderer HTML fixtures", () => {
  for (const renderer of rendererFixtures) {
    test(`${renderer.name} keeps document chrome out of the section tree`, () => {
      const html = loadManPageFixture(renderer.fixture);
      const sections = parseManHtml(html);
      const allSections = walkSections(sections);
      const titles = sections.map((section) => section.title);

      expect(html).toContain(renderer.marker);
      for (const title of renderer.requiredTitles) {
        expect(titles).toContain(title);
      }

      // Renderer-specific page headers, footers, and table-of-contents links
      // are present in the HTML but are not document sections.
      expect(titles).not.toContain("LS(1)");
      expect(titles).not.toContain("GIT(1)");
      expect(new Set(allSections.map((section) => section.id)).size).toBe(
        allSections.length,
      );

      for (const section of allSections) {
        expect(section.blocks.length + section.children.length).toBeGreaterThan(0);
        for (const block of section.blocks) {
          if (block.type === "list") {
            expect(block.items.length).toBeGreaterThan(0);
            expect(block.items.some(containsRendererHtmlTag)).toBe(false);
          } else if (block.type === "definition-list") {
            expect(block.items.length).toBeGreaterThan(0);
            expect(block.items.some((item) => (
              item.terms.some(containsRendererHtmlTag)
              || containsRendererHtmlTag(item.description)
            ))).toBe(false);
          } else if (block.type === "spacer") {
            expect(block.indent).toBeGreaterThanOrEqual(0);
          } else {
            expect(containsRendererHtmlTag(block.children)).toBe(false);
          }
        }
      }
    });
  }
});
