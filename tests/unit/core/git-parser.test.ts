/**
 * @file Protects parsing regressions found in the large real git manual page.
 */

import { describe, expect, test } from "bun:test";
import { parseManHtml } from "../../../src/core/parser";
import { parseMandoc } from "../../../src/core/mandoc-parser";
import type { BlockNode, InlineNode, SectionNode } from "../../../src/core/types";
import { loadManPageFixture } from "../../fixtures/man-pages";

const html = loadManPageFixture("mandoc-git");
const sections = parseMandoc(html);

// ── Helpers ────────────────────────────────────────────────

/** Recursively collect all blocks from sections and subsections. */
function allBlocks(secs: SectionNode[]): { block: BlockNode; path: string }[] {
  const result: { block: BlockNode; path: string }[] = [];
  function walk(s: SectionNode, prefix: string) {
    for (const b of s.blocks) {
      result.push({ block: b, path: `${prefix}${s.title}` });
    }
    for (const c of s.children) {
      walk(c, `${prefix}${s.title}/`);
    }
  }
  for (const s of secs) walk(s, "");
  return result;
}

/** Flatten inline nodes to a plain-text preview. */
function inlinePreview(nodes: InlineNode[]): string {
  return nodes
    .map((n) => {
      switch (n.type) {
        case "text":
          return n.content;
        case "bold":
          return `<b>${inlinePreview(n.children)}</b>`;
        case "italic":
          return `<i>${inlinePreview(n.children)}</i>`;
        case "break":
          return `\n`;
        default:
          return n.type;
      }
    })
    .join("");
}

/** Check if any text node contains literal HTML tags. */
function hasLiteralHtmlTag(nodes: InlineNode[]): boolean {
  return nodes.some(
    (n) =>
      n.type === "text" &&
      (n.content.includes("<i>") ||
        n.content.includes("</i>") ||
        n.content.includes("<br") ||
        n.content.includes("<b>") ||
        n.content.includes("</b>")),
  );
}

// ── Tests ─────────────────────────────────────────────────

describe("git man page — format detection", () => {
  test("parseManHtml dispatches to mandoc parser", () => {
    const result = parseManHtml(html);
    expect(result.length).toBeGreaterThan(0);
    expect(result[0]!.title).toBe("NAME");
  });
});

describe("git man page — section structure", () => {
  test("parses 24 top-level sections", () => {
    expect(sections.length).toBe(24);
  });

  test("key section titles are present", () => {
    const titles = sections.map((s) => s.title);
    expect(titles).toContain("NAME");
    expect(titles).toContain("SYNOPSIS");
    expect(titles).toContain("DESCRIPTION");
    expect(titles).toContain("OPTIONS");
    expect(titles).toContain("CONFIGURATION MECHANISM");
    expect(titles).toContain("ENVIRONMENT VARIABLES");
    expect(titles).toContain("SEE ALSO");
  });

  test("ENVIRONMENT VARIABLES has Ss subsections", () => {
    const env = sections.find((s) => s.title === "ENVIRONMENT VARIABLES");
    expect(env).toBeDefined();
    expect(env!.children.length).toBeGreaterThan(0);
    const childTitles = env!.children.map((c) => c.title);
    expect(childTitles).toContain("Git Diffs");
  });

  test("GIT COMMANDS section exists with content", () => {
    const gc = sections.find((s) => s.title === "GIT COMMANDS");
    expect(gc).toBeDefined();
    expect(gc!.blocks.length).toBeGreaterThan(0);
  });
});

// ── Pre block parsing (THE KEY FIX) ──────────────────────

describe("git man page — pre block parsing", () => {
  const preBlocks = allBlocks(sections).filter((b) => b.block.type === "pre");

  test("finds 4 pre blocks total", () => {
    expect(preBlocks.length).toBe(4);
  });

  test("all pre blocks have numeric indent (not undefined)", () => {
    for (const { block } of preBlocks) {
      expect(block.type).toBe("pre");
      if (block.type === "pre") {
        expect(typeof block.indent).toBe("number");
        expect(Number.isNaN(block.indent)).toBe(false);
      }
    }
  });

  test("no pre block contains literal HTML tags", () => {
    for (const { block } of preBlocks) {
      if (block.type === "pre") {
        const hasTags = hasLiteralHtmlTag(block.children);
        expect(hasTags).toBe(false);
      }
    }
  });

  test("SYNOPSIS pre contains italic and break nodes", () => {
    const synopsis = sections.find((s) => s.title === "SYNOPSIS");
    expect(synopsis).toBeDefined();
    const pre = synopsis!.blocks.find((b) => b.type === "pre");
    expect(pre).toBeDefined();
    if (pre?.type === "pre") {
      const types = pre.children.map((n) => n.type);
      expect(types).toContain("italic");
      expect(types).toContain("break");
      expect(types).toContain("text");

      // The first child should be italic (git)
      const firstItalic = pre.children.find((n) => n.type === "italic");
      expect(firstItalic).toBeDefined();
      if (firstItalic?.type === "italic") {
        const text = firstItalic.children.find((c) => c.type === "text");
        expect(text).toBeDefined();
        if (text?.type === "text") {
          expect(text.content).toBe("git");
        }
      }
    }
  });

  test("SYNOPSIS pre break nodes are proper break type", () => {
    const synopsis = sections.find((s) => s.title === "SYNOPSIS");
    const pre = synopsis!.blocks.find((b) => b.type === "pre");
    if (pre?.type === "pre") {
      const breaks = pre.children.filter((n) => n.type === "break");
      expect(breaks.length).toBeGreaterThan(3);
      // No text node should contain "<br"
      const brInText = pre.children.some(
        (n) => n.type === "text" && n.content.includes("<br"),
      );
      expect(brInText).toBe(false);
    }
  });

  test("OPTIONS pre block has indent=8", () => {
    const options = sections.find((s) => s.title === "OPTIONS");
    const pre = options!.blocks.find((b) => b.type === "pre");
    expect(pre).toBeDefined();
    if (pre?.type === "pre") {
      expect(pre.indent).toBe(8);
      // Content should be the git command example
      const text = pre.children.find((n) => n.type === "text");
      if (text?.type === "text") {
        expect(text.content).toContain("git --git-dir");
      }
    }
  });

  test("keeps the explicit blank paragraph around the -C command example", () => {
    const options = sections.find((s) => s.title === "OPTIONS")!;
    const preIndex = options.blocks.findIndex(
      (block) =>
        block.type === "pre" &&
        block.children.some(
          (node) => node.type === "text" && node.content.includes("git --git-dir=a.git"),
        ),
    );

    expect(preIndex).toBeGreaterThan(0);
    const preceding = options.blocks[preIndex - 1];
    const following = options.blocks[preIndex + 1];
    expect(preceding?.type).toBe("spacer");
    expect(preceding?.type === "spacer" && preceding.indent).toBe(4);
    expect(following?.type).toBe("paragraph");
    if (following?.type === "paragraph") {
      expect(inlinePreview(following.children)).toContain("-c");
    }
  });

  test("CONFIGURATION MECHANISM pre has indent=4 and break nodes", () => {
    const cfg = sections.find((s) => s.title === "CONFIGURATION MECHANISM");
    const pre = cfg!.blocks.find((b) => b.type === "pre");
    expect(pre).toBeDefined();
    if (pre?.type === "pre") {
      expect(pre.indent).toBe(4);
      const breaks = pre.children.filter((n) => n.type === "break");
      expect(breaks.length).toBeGreaterThan(2);
      // Should contain config-like content
      const text = pre.children
        .filter((n) => n.type === "text")
        .map((n) => (n as { type: "text"; content: string }).content)
        .join("");
      expect(text).toContain("[core]");
    }
  });

  test("Git Diffs pre is in subsection with indent=8", () => {
    const env = sections.find((s) => s.title === "ENVIRONMENT VARIABLES");
    const diffs = env!.children.find((c) => c.title === "Git Diffs");
    expect(diffs).toBeDefined();
    const pre = diffs!.blocks.find((b) => b.type === "pre");
    expect(pre).toBeDefined();
    if (pre?.type === "pre") {
      expect(pre.indent).toBe(8);
      const text = pre.children
        .filter((n) => n.type === "text")
        .map((n) => (n as { type: "text"; content: string }).content)
        .join("");
      expect(text).toContain("path old-file");
    }
  });
});

// ── Inline formatting ─────────────────────────────────────

describe("git man page — inline formatting", () => {
  test("DESCRIPTION has bold nodes", () => {
    const desc = sections.find((s) => s.title === "DESCRIPTION");
    expect(desc).toBeDefined();
    const hasBold = desc!.blocks.some(
      (b) =>
        b.type === "paragraph" &&
        b.children.some((n) => n.type === "bold"),
    );
    expect(hasBold).toBe(true);
  });

  test("SYNOPSIS pre italic node renders as italic, not literal <i>", () => {
    const synopsis = sections.find((s) => s.title === "SYNOPSIS");
    const pre = synopsis!.blocks.find((b) => b.type === "pre");
    if (pre?.type === "pre") {
      // Verify <i>git</i> is parsed as {type:"italic"} not as text "<i>git</i>"
      const hasItalicNode = pre.children.some((n) => n.type === "italic");
      const hasLiteralTag = pre.children.some(
        (n) => n.type === "text" && n.content.includes("<i>"),
      );
      expect(hasItalicNode).toBe(true);
      expect(hasLiteralTag).toBe(false);
    }
  });

  test("no paragraph has literal HTML tags", () => {
    const blocks = allBlocks(sections).filter((b) => b.block.type === "paragraph");
    for (const { block } of blocks) {
      if (block.type === "paragraph") {
        expect(hasLiteralHtmlTag(block.children)).toBe(false);
      }
    }
  });
});

// ── Inline content grouping (Bd-indent bare text + <b>/<i>) ──

describe("git man page — inline content grouping", () => {
  test("-h option description is a single paragraph, not scattered blocks", () => {
    // Bd-indent divs interleave bare text with inline <b>/<i> elements.
    // These must merge into one paragraph, otherwise '--all', 'or', '-a'
    // each land on their own line.
    const options = sections.find((s) => s.title === "OPTIONS")!;
    const idx = options.blocks.findIndex(
      (b) =>
        b.type === "paragraph" &&
        b.children.some(
          (n) => n.type === "text" && n.content.includes("Prints the synopsis"),
        ),
    );
    expect(idx).toBeGreaterThanOrEqual(0);
    const block = options.blocks[idx]!;
    expect(block.type).toBe("paragraph");
    if (block.type === "paragraph") {
      // The single paragraph must contain the surrounding text AND the
      // inline <b>--all</b> / <b>-a</b> flags.
      const flat = inlinePreview(block.children);
      expect(flat).toContain("Prints the synopsis");
      expect(flat).toContain("<b>--all</b>");
      expect(flat).toContain("<b>-a</b>");
      expect(flat).toContain("is given then all available");
    }
  });

  test("bare '--all' / 'or' / '-a' never appear as standalone blocks", () => {
    // Regression: previously each inline fragment became its own
    // paragraph. A paragraph whose entire text is just these fragments
    // indicates the grouping regressed.
    const options = sections.find((s) => s.title === "OPTIONS")!;
    const standalone = options.blocks.filter((b) => {
      if (b.type !== "paragraph") return false;
      const flat = inlinePreview(b.children).trim();
      return (
        flat === "--all" ||
        flat === "<b>--all</b>" ||
        flat === "or" ||
        flat === "-a" ||
        flat === "<b>-a</b>"
      );
    });
    expect(standalone).toEqual([]);
  });

  test("no paragraph text node contains embedded newlines", () => {
    // Bd-indent bare text arrives with HTML source line-wrap newlines.
    // These must be normalised to spaces, not preserved as line breaks.
    const offenders: string[] = [];
    for (const { block } of allBlocks(sections)) {
      if (block.type !== "paragraph") continue;
      for (const n of block.children) {
        if (n.type === "text" && n.content.includes("\n")) {
          offenders.push(n.content.slice(0, 60));
        }
      }
    }
    expect(offenders).toEqual([]);
  });

  test("SYNOPSIS pre has no consecutive blank lines", () => {
    const synopsis = sections.find((s) => s.title === "SYNOPSIS")!;
    const pre = synopsis.blocks.find((b) => b.type === "pre");
    if (pre?.type === "pre") {
      const flat = pre.children
        .map((n) => {
          if (n.type === "text") return n.content;
          if (n.type === "break") return "\n";
          if (n.type === "italic")
            return n.children
              .map((c) => (c.type === "text" ? c.content : ""))
              .join("");
          return "";
        })
        .join("");
      expect(/\n{2,}/.test(flat)).toBe(false);
    }
  });
});

// ── Indent verification ────────────────────────────────────

describe("git man page — indent structure", () => {
  test("OPTIONS has multi-level indent", () => {
    const options = sections.find((s) => s.title === "OPTIONS");
    expect(options).toBeDefined();
    const indents = new Set(
      options!.blocks
        .filter((b) => b.type === "paragraph")
        .map((b) => (b.type === "paragraph" ? b.indent : -1)),
    );
    expect(indents.has(0)).toBe(true);
    expect(indents.has(4)).toBe(true);
  });

  test("CONFIGURATION MECHANISM pre has proper indent", () => {
    const cfg = sections.find((s) => s.title === "CONFIGURATION MECHANISM");
    const pre = cfg!.blocks.find((b) => b.type === "pre");
    if (pre?.type === "pre") {
      expect(pre.indent).toBeGreaterThanOrEqual(4);
    }
  });

  test("Git Diffs pre in subsection has accumulated indent", () => {
    const env = sections.find((s) => s.title === "ENVIRONMENT VARIABLES");
    const diffs = env!.children.find((c) => c.title === "Git Diffs");
    const pre = diffs!.blocks.find((b) => b.type === "pre");
    if (pre?.type === "pre") {
      // Should have indent from Bd-indent (+4) + dd (+4) = 8
      expect(pre.indent).toBe(8);
    }
  });
});

// ── Regression tests ──────────────────────────────────────

describe("git man page — regression tests", () => {
  test("pre block indent is never undefined", () => {
    const blocks = allBlocks(sections).filter((b) => b.block.type === "pre");
    for (const { block } of blocks) {
      if (block.type === "pre") {
        expect(block.indent).toBeDefined();
        expect(typeof block.indent).toBe("number");
      }
    }
  });

  test("SYNOPSIS pre does not show <br/> as literal text", () => {
    const synopsis = sections.find((s) => s.title === "SYNOPSIS");
    const pre = synopsis!.blocks.find((b) => b.type === "pre");
    if (pre?.type === "pre") {
      const allText = pre.children
        .filter((n) => n.type === "text")
        .map((n) => (n as { type: "text"; content: string }).content)
        .join("");
      expect(allText).not.toContain("<br");
      expect(allText).not.toContain("<i>");
      expect(allText).not.toContain("</i>");
    }
  });

  test("all sections have non-empty blocks or children", () => {
    for (const s of sections) {
      const hasContent = s.blocks.length > 0 || s.children.length > 0;
      expect(hasContent).toBe(true);
    }
  });

  test("Pre component import works (no MantCode reference)", () => {
    // This test verifies that the import path is correct.
    // If Pre.tsx doesn't exist or has errors, this will fail.
    const mod = require("../../../src/ui/Pre");
    expect(mod.Pre).toBeDefined();
    expect(typeof mod.Pre).toBe("function");
  });
});

// ── Global: no HTML tags anywhere in parse output ─────────

describe("git man page — no HTML tags in parse output", () => {
  /** Recursively extract every text string from inline nodes. */
  function extractAllText(nodes: InlineNode[]): string[] {
    const result: string[] = [];
    for (const n of nodes) {
      switch (n.type) {
        case "text":
          result.push(n.content);
          break;
        case "bold":
        case "italic":
        case "code":
          result.push(...extractAllText(n.children));
          break;
        // break nodes produce no text
      }
    }
    return result;
  }

  /** Collect every text string across all blocks (paragraphs, pre, list items). */
  function allTextStrings(): string[] {
    const texts: string[] = [];
    for (const { block } of allBlocks(sections)) {
      switch (block.type) {
        case "paragraph":
        case "pre":
          texts.push(...extractAllText(block.children));
          break;
        case "list":
          for (const item of block.items) {
            texts.push(...extractAllText(item));
          }
          break;
        case "spacer":
          break;
      }
    }
    return texts;
  }

  test("no text node contains any HTML tag", () => {
    // Match HTML tags that could leak from the parser — inline
    // formatting and structural tags used in man page HTML output.
    // Excludes tags like <head> that are also valid man-page placeholders.
    const htmlTagRe = /<\/?(?:i|b|br|font|small|span|a|code|pre|div|section|h[1-6]|p|dl|dt|dd|ul|ol|li|table|tr|td|th|thead|tbody|tfoot|em|strong|tt|u)\b[^>]*>/i;
    const offenders: string[] = [];
    for (const text of allTextStrings()) {
      if (htmlTagRe.test(text)) {
        offenders.push(text.slice(0, 80));
      }
    }
    expect(offenders).toEqual([]);
  });
});
