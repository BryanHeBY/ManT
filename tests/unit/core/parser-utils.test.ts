/**
 * @file Locks in shared DOM-to-inline parsing and whitespace regressions.
 */

import { describe, expect, test } from "bun:test";
import { parse } from "node-html-parser";
import {
  trimText,
  parseBlockElementWithIndent,
} from "../../../src/core/parser-utils";
import type { InlineNode } from "../../../src/core/types";

// ── Regression tests for the shared parser-utils fixes ────
//
// These lock in two root-cause fixes that surfaced while rendering
// the git man page:
//
//   1. trimText must collapse embedded newlines to spaces, so bare
//      text inside mandoc <div class="Bd-indent"> (which arrives with
//      HTML source line-wrap newlines) does not render as line breaks.
//
//   2. <pre> parsing must deduplicate newlines around <br/> break
//      nodes, so a "<br/>" (which yields a break node) surrounded by
//      "\n" text does not produce consecutive blank lines.

// ── Helpers ────────────────────────────────────────────────

/** Build an HTMLElement from a snippet and return its first child element. */
function firstElement(html: string) {
  const root = parse(html);
  const el = root.querySelector("*");
  if (!el) throw new Error("no element parsed from snippet");
  return el;
}

/** Render a pre block's inline children to a flat string (break = \n). */
function flattenPre(children: InlineNode[]): string {
  return children
    .map((n) => {
      switch (n.type) {
        case "text":
          return n.content;
        case "break":
          return "\n";
        case "italic":
        case "bold":
        case "code":
          return n.children
            .map((c) => (c.type === "text" ? c.content : ""))
            .join("");
        default:
          return "";
      }
    })
    .join("");
}

// ── trimText ──────────────────────────────────────────────

describe("parser-utils — trimText newline collapse", () => {
  test("collapses a single embedded newline into a space", () => {
    expect(trimText("Prints the synopsis\n commands")).toBe(
      "Prints the synopsis commands",
    );
  });

  test("collapses multiple embedded newlines and surrounding spaces", () => {
    expect(trimText("the option\n\n  or\n  -a is given")).toBe(
      "the option or -a is given",
    );
  });

  test("normalises CRLF then collapses to spaces", () => {
    expect(trimText("line one\r\nline two")).toBe("line one line two");
  });

  test("collapses runs of tabs and spaces", () => {
    expect(trimText("a\t\t  b")).toBe("a b");
  });

  test("trims leading and trailing whitespace/newlines", () => {
    expect(trimText("\n  hello world  \n")).toBe("hello world");
  });

  test("empty / whitespace-only input yields empty string", () => {
    expect(trimText("   \n\t ")).toBe("");
    expect(trimText("")).toBe("");
  });
});

// ── <pre> newline dedup ────────────────────────────────────

describe("parser-utils — pre block newline dedup", () => {
  test("does not emit consecutive newlines around <br/>", () => {
    // Mimics mandoc SYNOPSIS: text ends with \n, then <br/>, then next
    // line begins with \n. Without dedup this yields three newlines.
    const pre = firstElement(
      "<pre>git [-v]\n<br/>\n    [--bare]\n<br/>\n    <command></pre>",
    );
    const block = parseBlockElementWithIndent(pre, 0);
    expect(block).not.toBeNull();
    expect(block!.type).toBe("pre");
    if (block!.type === "pre") {
      const flat = flattenPre(block!.children);
      expect(/\n{2,}/.test(flat)).toBe(false);
    }
  });

  test("preserves single line breaks between content", () => {
    const pre = firstElement(
      "<pre>line one\n<br/>\nline two</pre>",
    );
    const block = parseBlockElementWithIndent(pre, 0);
    if (block?.type === "pre") {
      const flat = flattenPre(block.children);
      expect(flat).toBe("line one\nline two");
    }
  });

  test("re-parses inner <i> as italic node, not literal text", () => {
    const pre = firstElement("<pre><i>git</i> [-v]</pre>");
    const block = parseBlockElementWithIndent(pre, 0);
    if (block?.type === "pre") {
      const hasItalic = block.children.some((n) => n.type === "italic");
      const hasLiteral = block.children.some(
        (n) => n.type === "text" && n.content.includes("<i>"),
      );
      expect(hasItalic).toBe(true);
      expect(hasLiteral).toBe(false);
    }
  });

  test("<br/> becomes a break node, never literal text", () => {
    const pre = firstElement("<pre>a\n<br/>\nb</pre>");
    const block = parseBlockElementWithIndent(pre, 0);
    if (block?.type === "pre") {
      const hasBreak = block.children.some((n) => n.type === "break");
      const brInText = block.children.some(
        (n) => n.type === "text" && n.content.includes("<br"),
      );
      expect(hasBreak).toBe(true);
      expect(brInText).toBe(false);
    }
  });

  test("pre block carries the supplied indent", () => {
    const pre = firstElement("<pre>example</pre>");
    const block = parseBlockElementWithIndent(pre, 8);
    expect(block?.type).toBe("pre");
    if (block?.type === "pre") {
      expect(block.indent).toBe(8);
    }
  });

  test("a pre containing only newlines and breaks collapses cleanly", () => {
    const pre = firstElement("<pre>content<br/>\nmore</pre>");
    const block = parseBlockElementWithIndent(pre, 0);
    if (block?.type === "pre") {
      const flat = flattenPre(block.children);
      expect(flat).toBe("content\nmore");
    }
  });

  test("removes renderer-only newlines at pre boundaries", () => {
    const pre = firstElement(
      "<pre>\nline one\nline two\n    </pre>",
    );
    const block = parseBlockElementWithIndent(pre, 0);
    if (block?.type === "pre") {
      expect(flattenPre(block.children)).toBe("line one\nline two");
    }
  });

  test("preserves intentional internal and final blank lines", () => {
    const pre = firstElement("<pre>line one\n\n</pre>");
    const block = parseBlockElementWithIndent(pre, 0);
    if (block?.type === "pre") {
      // One closing-tag formatting newline is removed; the other remains as
      // the blank line authored inside the display.
      expect(flattenPre(block.children)).toBe("line one\n");
    }
  });
});
