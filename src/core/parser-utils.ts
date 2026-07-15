import { parse, HTMLElement, TextNode } from "node-html-parser";
import type { BlockNode, InlineNode } from "./types";

// ── Type guards ────────────────────────────────────────────

export function isElement(node: unknown): node is HTMLElement {
  return node instanceof HTMLElement;
}

export function isText(node: unknown): node is TextNode {
  return node instanceof TextNode;
}

// ── Text normalisation ────────────────────────────────────

export function trimText(text: string): string {
  return text.replace(/\r\n/g, "\n").replace(/\n/g, " ").replace(/[ \t]+/g, " ").trim();
}

export function normalizeInlineText(text: string): string {
  return text
    .replace(/\r\n/g, "\n")
    .replace(/\n/g, " ")
    .replace(/[ \t]+/g, " ");
}

export function normalizePreText(text: string): string {
  return text.replace(/\r\n/g, "\n").replace(/\t/g, "        ");
}

export function getHeadingText(node: HTMLElement): string {
  return trimText(node.textContent).replace(/\n/g, " ").replace(/[ \t]+/g, " ");
}

// ── Inline parsing ─────────────────────────────────────────
//
// Handles <b>/<strong>, <i>/<em>, <code>/<tt>, <span class="Li">,
// <br>, <a>, <font>, <small> — all elements common to both
// mandoc and groff HTML output.
//
// Unknown tags fall through and their children are processed
// recursively, making <font>, <small> etc. transparent.

export function parseInline(
  node: HTMLElement | TextNode,
  preserveNewlines = false
): InlineNode[] {
  if (isText(node)) {
    const text = preserveNewlines
      ? normalizePreText(node.text)
      : normalizeInlineText(node.text);
    if (!text) return [];
    return [{ type: "text", content: text }];
  }

  const tag = node.tagName.toLowerCase();
  const children: InlineNode[] = [];
  for (const child of node.childNodes) {
    if (isText(child) || isElement(child)) {
      children.push(...parseInline(child, preserveNewlines));
    }
  }

  switch (tag) {
    case "b":
    case "strong":
      return [{ type: "bold", children }];
    case "i":
    case "em":
      return [{ type: "italic", children }];
    case "code":
    case "tt":
      return [{ type: "code", children }];
    case "span":
      // mandoc uses <span class="Li"> for inline literals.
      if (node.classList.contains("Li")) {
        return [{ type: "code", children }];
      }
      return children;
    case "br":
      return [{ type: "break" }];
    case "a":
      // Treat links as plain text.
      return children;
    default:
      // Transparent passthrough for <font>, <small>, <u>, etc.
      return children;
  }
}

// ── Indent parsing ─────────────────────────────────────────

export function parseIndent(node: HTMLElement): number {
  // groff uses inline style="margin-left:X%;" on <p> elements.
  const style = node.getAttribute("style") ?? "";
  const match = style.match(/margin-left:\s*(\d+)%/);
  if (match) {
    const percent = Number.parseInt(match[1]!, 10);
    return Math.max(0, Math.round((percent / 100) * 80));
  }

  // mandoc uses class-based indent: Bd-indent divs and <dd> elements.
  const tag = node.tagName.toLowerCase();
  const classes = node.classList;
  if (classes.contains("Bd-indent") || tag === "dd") {
    return 4;
  }
  return 0;
}

// ── Block parsing ──────────────────────────────────────────
//
// Shared block-level element parser. Both mandoc and groff
// produce <p>, <pre>, <ul>, <ol>, <dl> elements.

export function parseListItems(
  listNode: HTMLElement,
  preserveNewlines = false
): InlineNode[][] {
  const items: InlineNode[][] = [];
  for (const li of listNode.querySelectorAll(":scope > li, :scope > dt, :scope > dd")) {
    const itemChildren: InlineNode[] = [];
    for (const child of li.childNodes) {
      if (isText(child) || isElement(child)) {
        itemChildren.push(...parseInline(child, preserveNewlines));
      }
    }
    if (itemChildren.length > 0) items.push(itemChildren);
  }
  return items;
}

export function parseBlockElementWithIndent(
  node: HTMLElement,
  indent: number
): BlockNode | null {
  const tag = node.tagName.toLowerCase();

  if (tag === "p") {
    const children: InlineNode[] = [];
    for (const child of node.childNodes) {
      if (isText(child) || isElement(child)) {
        children.push(...parseInline(child));
      }
    }
    if (children.length === 0) return null;
    return { type: "paragraph", children, indent };
  }

  if (tag === "pre") {
    // node-html-parser treats <pre> content as a single raw text
    // node, so inner <i>, <br/> tags appear as literal text.
    // Re-parse the inner HTML to get proper element nodes.
    const reParsed = parse(node.innerHTML);
    const children: InlineNode[] = [];
    for (const child of reParsed.childNodes) {
      if (isText(child) || isElement(child)) {
        children.push(...parseInline(child, true));
      }
    }
    // Deduplicate newlines: <br/> already produces a break node,
    // and surrounding text from HTML source formatting may carry
    // leading/trailing \n.  Strip these to avoid blank lines.
    const normalized: InlineNode[] = [];
    for (let i = 0; i < children.length; i++) {
      const child = children[i]!;
      const prev = children[i - 1];
      const next = children[i + 1];
      if (child.type === "text") {
        let content = child.content;
        if (prev?.type === "break") content = content.replace(/^\n+/, "");
        if (next?.type === "break") content = content.replace(/\n+$/, "");
        if (content) normalized.push({ type: "text", content });
      } else {
        normalized.push(child);
      }
    }
    if (normalized.length === 0) return null;
    return { type: "pre", children: normalized, indent };
  }

  if (tag === "ul" || tag === "ol" || tag === "dl") {
    const items = parseListItems(node);
    if (items.length === 0) return null;
    return { type: "list", items, indent };
  }

  // Fallback: treat unknown block-level elements as paragraphs.
  const children: InlineNode[] = [];
  for (const child of node.childNodes) {
    if (isText(child) || isElement(child)) {
      children.push(...parseInline(child));
    }
  }
  if (children.length === 0) return null;
  return { type: "paragraph", children, indent };
}

export function parseBlockElement(node: HTMLElement): BlockNode | null {
  return parseBlockElementWithIndent(node, parseIndent(node));
}

// ── Heading detection (groff) ──────────────────────────────

export function isHeading(tag: string): boolean {
  return tag === "h2" || tag === "h3" || tag === "h4" || tag === "h5" || tag === "h6";
}
