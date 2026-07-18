/**
 * @file Parses the HTML layout emitted by man-db/groff into Mant section nodes.
 */

import { parse, HTMLElement } from "node-html-parser";
import type { SectionNode } from "./types";
import { SectionTree } from "./section-tree";
import {
  isElement,
  isText,
  trimText,
  getHeadingText,
  parseBlockElement,
  parseBlockElementWithIndent,
  isHeading,
} from "./parser-utils";

// ── Groff HTML parser ──────────────────────────────────────
//
// Groff (man -Thtml) HTML structure:
//
//   <body>
//     <h1 align="center">TITLE</h1>
//     <a href="#NAME">NAME</a><br>          <!-- TOC -->
//     <a href="#SYNOPSIS">SYNOPSIS</a><br>
//     ...
//     <hr>
//     <h2>NAME<a name="NAME"></a></h2>       <!-- Section -->
//     <p style="margin-left:9%; margin-top: 1em">...</p>
//     <p style="margin-left:14%;">...</p>     <!-- Description -->
//     <p style="margin-left:19%; margin-top: 1em">...</p>  <!-- Example -->
//     <h3>Subsection</h3>
//     <pre>code block</pre>
//     ...
//   </body>
//
// Indent levels (margin-left percentage → terminal columns):
//   0%   → 0   (no style)
//   9%   → 7   (section content, option names)
//   14%  → 11  (descriptions)
//   19%  → 15  (examples, sub-descriptions)
//
// Inline elements: <b>, <i>, <font color>, <small>, <br>, <a>

function isSkipElement(node: HTMLElement): boolean {
  const tag = node.tagName.toLowerCase();
  // Skip <h1> (man page title), <hr>, and TOC <a href> links + <br>.
  if (tag === "h1" || tag === "hr") return true;
  if (tag === "a") {
    // <a href="#..."> is a TOC link; <a name="..."> is a section anchor (inside <h2>).
    return node.getAttribute("href") !== null;
  }
  return false;
}

function parseGroffBody(body: HTMLElement, tree: SectionTree): void {
  for (const node of body.childNodes) {
    if (!isElement(node) && !isText(node)) continue;

    // Skip pure whitespace text nodes at the top level.
    if (isText(node)) {
      const text = trimText(node.text);
      if (text) {
        // Only add text if there's a current section (after first heading).
        if (tree.currentSection()) {
          tree.addBlock({
            type: "paragraph",
            children: [{ type: "text", content: text }],
            indent: 0,
          });
        }
      }
      continue;
    }

    // Skip TOC links, <hr>, <h1> title.
    if (isSkipElement(node)) continue;

    // Skip <br> elements at top level (TOC line breaks).
    const tag = node.tagName.toLowerCase();
    if (tag === "br") continue;

    // Handle headings: <h2> → section (level 2), <h3> → subsection (level 3).
    if (isHeading(tag)) {
      const level = Number.parseInt(tag[1]!, 10);
      const title = getHeadingText(node);
      if (title) {
        tree.pushSection(title, level);
      }
      continue;
    }

    // Handle layout tables (groff uses <table> for complex option layouts).
    if (tag === "table") {
      parseTable(node, tree);
      continue;
    }

    // Parse block-level elements (<p>, <pre>, <ul>, <ol>, <dl>).
    const block = parseBlockElement(node);
    if (block) tree.addBlock(block);
  }
}

// ── Table parsing ─────────────────────────────────────────
//
// Groff uses layout tables for options that need column alignment.
// Structure:
//   <table>
//     <tr>
//       <td width="9%"></td>           <!-- spacer -->
//       <td width="3%"><p>-c</p></td>    <!-- option flag -->
//       <td width="6%"></td>            <!-- spacer -->
//       <td width="82%"><p>desc</p></td> <!-- description -->
//     </tr>
//   </table>
//
// We convert each <td> with content into a paragraph/pre block,
// using the cumulative width of preceding <td> elements as indent.

function parseTable(table: HTMLElement, tree: SectionTree): void {
  for (const tr of table.childNodes) {
    if (!isElement(tr)) continue;
    if (tr.tagName.toLowerCase() !== "tr") continue;

    let cumulativeWidth = 0;
    for (const td of tr.childNodes) {
      if (!isElement(td)) continue;
      if (td.tagName.toLowerCase() !== "td") continue;

      const widthAttr = td.getAttribute("width");
      const width = widthAttr ? Number.parseInt(widthAttr, 10) : 0;
      const indent = Math.round((cumulativeWidth / 100) * 80);

      // Parse block-level content within this <td>.
      for (const child of td.childNodes) {
        if (!isElement(child)) continue;
        const childTag = child.tagName.toLowerCase();
        if (
          childTag === "p" || childTag === "pre" ||
          childTag === "ul" || childTag === "ol" || childTag === "dl"
        ) {
          const block = parseBlockElementWithIndent(child, indent);
          if (block) tree.addBlock(block);
        }
      }

      cumulativeWidth += width;
    }
  }
}

// ── Public entry point ─────────────────────────────────────

export function parseGroff(html: string): SectionNode[] {
  const root = parse(html);
  const body = root.querySelector("body");
  if (!body) return [];

  const tree = new SectionTree();
  parseGroffBody(body, tree);

  return tree.getSections();
}
