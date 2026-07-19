/**
 * @file Parses mandoc HTML into Mant's renderer-neutral section tree.
 */

import { parse, HTMLElement } from "node-html-parser";
import type { InlineNode, SectionNode } from "./types";
import { SectionTree } from "./section-tree";
import {
  isElement,
  isText,
  parseInline,
  parseIndent,
  parseBlockElementWithIndent,
} from "./parser-utils";

// ── Mandoc-specific helpers ───────────────────────────────

// Block-level tags in mandoc HTML. Everything else (text, <b>, <i>,
// <code>, <span>, <a>, <br>, <font>, <small>, <u>) is inline and must
// be merged into the surrounding paragraph rather than emitted as its
// own block.
const MANDOC_BLOCK_TAGS = new Set([
  "p",
  "pre",
  "div",
  "dl",
  "ul",
  "ol",
  "section",
  "table",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "br",
]);

function isMandocSection(node: HTMLElement): boolean {
  const tag = node.tagName.toLowerCase();
  if (tag !== "section") return false;
  return node.classList.contains("Sh") || node.classList.contains("Ss");
}

function getMandocSectionLevel(node: HTMLElement): number {
  return node.classList.contains("Ss") ? 3 : 2;
}

function findMandocHeading(section: HTMLElement): HTMLElement | null {
  return (
    section.querySelector("h1.Sh, h2.Ss, h1, h2") ??
    null
  );
}

// ── Mandoc content parser ──────────────────────────────────
//
// Mandoc HTML structure (from mandoc -Thtml):
//
//   <div class="manual-text">
//     <section class="Sh">
//       <h1 class="Sh" id="...">SECTION TITLE</h1>
//       <p class="Pp">...</p>
//       <dl class="Bl-tag">...</dl>
//       <div class="Bd-indent">...</div>
//     </section>
//     <section class="Ss">
//       <h2 class="Ss" id="...">Subsection</h2>
//       ...
//     </section>
//   </div>
//
// Key mandoc classes:
//   .Sh   — section heading (<h1>, level 2)
//   .Ss   — subsection heading (<h2>, level 3)
//   .Pp   — paragraph
//   .Bd-indent — indented block (4-space indent)
//   .Bl-tag    — tag/definition list (<dl>)
//   .Bl-bullet — bullet list (<ul>)
//   .Bl-enum   — enumerated list (<ol>)

function parseMandocChildren(
  parent: HTMLElement,
  tree: SectionTree,
  baseIndent: number,
  skipNode?: HTMLElement
): void {
  // Accumulate consecutive inline content (bare text, <b>, <i>, <code>,
  // ...) into a single paragraph. Mandoc's Bd-indent divs interleave
  // bare text with inline <b>/<i> elements, so emitting each as its own
  // block would scatter one sentence across many lines.
  let inlineBuffer: InlineNode[] = [];

  function flushInline(): void {
    if (inlineBuffer.length === 0) return;
    const meaningful = inlineBuffer.some(
      (n) =>
        (n.type === "text" && n.content.trim().length > 0) ||
        n.type === "bold" ||
        n.type === "italic" ||
        n.type === "code",
    );
    if (meaningful) {
      tree.addBlock({
        type: "paragraph",
        children: inlineBuffer,
        indent: baseIndent,
      });
    }
    inlineBuffer = [];
  }

  for (const child of parent.childNodes) {
    if (skipNode && child === skipNode) continue;
    if (!isElement(child) && !isText(child)) continue;

    if (isText(child)) {
      inlineBuffer.push(...parseInline(child));
      continue;
    }

    if (isMandocSection(child)) {
      flushInline();
      parseMandocSection(child, tree);
      continue;
    }

    const tag = child.tagName.toLowerCase();

    // Inline-level elements are merged into the current paragraph.
    if (!MANDOC_BLOCK_TAGS.has(tag)) {
      inlineBuffer.push(...parseInline(child));
      continue;
    }

    // Block-level element: flush any pending inline content first.
    flushInline();

    const childIndent = baseIndent + parseIndent(child);

    // A direct <br> between block wrappers is mandoc's vertical spacing
    // signal. A <br> inside a paragraph or pre remains an inline line break.
    if (tag === "br") {
      tree.addBlock({ type: "spacer", indent: baseIndent });
      continue;
    }

    // Bd-indent is a wrapper that adds indentation to its children.
    if (tag === "div" && child.classList.contains("Bd-indent")) {
      parseMandocChildren(child, tree, childIndent);
      continue;
    }

    // Definition lists that contain block-level content (e.g. <pre> inside <dd>)
    // should be rendered as a sequence of blocks rather than flattened into a
    // bulleted list. This preserves code examples and other nested blocks.
    if (tag === "dl" && child.querySelector("dd pre, dd div, dd p") !== null) {
      for (const sub of child.childNodes) {
        if (!isElement(sub)) continue;
        const subTag = sub.tagName.toLowerCase();
        if (subTag === "dt") {
          const dtChildren: InlineNode[] = [];
          for (const n of sub.childNodes) {
            if (isText(n) || isElement(n)) {
              dtChildren.push(...parseInline(n));
            }
          }
          if (dtChildren.length > 0) {
            tree.addBlock({
              type: "paragraph",
              children: dtChildren,
              indent: childIndent,
            });
          }
        } else if (subTag === "dd") {
          // <dd> elements get +4 indent (from parseIndent returning 4 for dd tag).
          const ddIndent = childIndent + parseIndent(sub);
          // Group consecutive inline content (text, <b>, <i>, etc.) into
          // a single paragraph. Only block-level children (<p>, <pre>,
          // <div>, <dl>, <ul>, <ol>) are processed as separate blocks.
          let ddInline: InlineNode[] = [];
          for (const n of sub.childNodes) {
            if (!isText(n) && !isElement(n)) continue;
            if (isElement(n)) {
              const nTag = n.tagName.toLowerCase();
              if (
                nTag === "p" || nTag === "pre" || nTag === "div" ||
                nTag === "dl" || nTag === "ul" || nTag === "ol"
              ) {
                if (ddInline.length > 0) {
                  tree.addBlock({
                    type: "paragraph",
                    children: ddInline,
                    indent: ddIndent,
                  });
                  ddInline = [];
                }
                if (nTag === "div" && n.classList.contains("Bd-indent")) {
                  parseMandocChildren(n, tree, ddIndent + parseIndent(n));
                } else {
                  const block = parseBlockElementWithIndent(
                    n,
                    ddIndent + parseIndent(n),
                  );
                  if (block) tree.addBlock(block);
                }
                continue;
              }
            }
            ddInline.push(...parseInline(n));
          }
          if (ddInline.length > 0) {
            tree.addBlock({
              type: "paragraph",
              children: ddInline,
              indent: ddIndent,
            });
          }
        }
      }
      continue;
    }

    const block = parseBlockElementWithIndent(child, childIndent);
    if (block) tree.addBlock(block);
  }

  flushInline();
}

function parseMandocSection(section: HTMLElement, tree: SectionTree): void {
  const heading = findMandocHeading(section);
  if (!heading) return;

  const title = heading.textContent.replace(/\n/g, " ").replace(/[ \t]+/g, " ").trim();
  if (!title) return;

  const level = getMandocSectionLevel(section);
  tree.pushSection(title, level);

  parseMandocChildren(section, tree, 0, heading);
}

// ── Public entry point ─────────────────────────────────────

export function parseMandoc(html: string): SectionNode[] {
  const root = parse(html);
  const body = root.querySelector("body");
  if (!body) return [];

  const contentRoot = body.querySelector("div.manual-text") ?? body;
  const tree = new SectionTree();

  for (const node of contentRoot.childNodes) {
    if (isElement(node) && isMandocSection(node)) {
      parseMandocSection(node, tree);
    }
  }

  return tree.getSections();
}
