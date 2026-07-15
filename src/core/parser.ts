import { parse } from "node-html-parser";
import type { SectionNode } from "./types";
import { parseMandoc } from "./mandoc-parser";
import { parseGroff } from "./groff-parser";

// ── Format detection ───────────────────────────────────────
//
// Mandoc HTML output contains:
//   <div class="manual-text">
//     <section class="Sh"> ... </section>
//
// Groff HTML output contains:
//   <!-- Creator : groff version X.X.X -->
//   <h2>SECTION<a name="..."></a></h2>
//
// We detect mandoc by looking for <section class="Sh"> or
// <div class="manual-text">.  If neither is found, fall back
// to the groff parser.

function isMandocHtml(root: ReturnType<typeof parse>): boolean {
  const body = root.querySelector("body");
  if (!body) return false;
  return (
    body.querySelector("section.Sh, div.manual-text") !== null
  );
}

// ── Public entry point ─────────────────────────────────────

export function parseManHtml(html: string): SectionNode[] {
  const root = parse(html);

  if (isMandocHtml(root)) {
    return parseMandoc(html);
  }

  return parseGroff(html);
}

// Re-export shared utilities for consumers that need them.
export { parseInline } from "./parser-utils";
export type { InlineNode, BlockNode, SectionNode } from "./types";
