/**
 * @file Serializes Mant's renderer-neutral query model as portable CommonMark.
 *
 * The parsers have already reconciled groff and mandoc HTML differences, so
 * this renderer deliberately consumes the structured AST instead of converting
 * renderer-specific HTML a second time. Presentation-only roff indentation is
 * omitted where Markdown would otherwise misinterpret prose as a code block.
 */

import type {
  Break,
  Code,
  Heading,
  List,
  ListItem,
  Paragraph,
  PhrasingContent,
  Root,
  RootContent,
} from "mdast";
import { toMarkdown } from "mdast-util-to-markdown";
import type {
  BlockNode,
  DefinitionListItem,
  InlineNode,
  SectionNode,
} from "../core";
import type { QueryResult } from "../query";
import type { TldrPage } from "../tldr";

// ── Public renderer ────────────────────────────────────────

/** Converts a complete Mant query result into deterministic CommonMark. */
export function renderMarkdown(result: QueryResult): string {
  const children: RootContent[] = [heading(1, result.topic)];

  if (result.tldr) {
    children.push(...renderTldr(result.tldr));
    if (result.sections.length > 0) children.push({ type: "thematicBreak" });
  }

  for (const section of result.sections) {
    children.push(...renderSection(section, 2));
  }

  const root: Root = { type: "root", children };
  // The CLI writer adds the process-level trailing newline. Returning a string
  // without one also makes the public renderer convenient to compose or test.
  return toMarkdown(root, {
    bullet: "-",
    emphasis: "*",
    fences: true,
    strong: "*",
  }).trimEnd();
}

// ── Document structure ─────────────────────────────────────

function renderSection(section: SectionNode, depth: number): RootContent[] {
  const children: RootContent[] = [heading(clampHeadingDepth(depth), section.title)];
  children.push(...renderBlocks(section.blocks));

  for (const child of section.children) {
    children.push(...renderSection(child, depth + 1));
  }
  return children;
}

function renderBlocks(blocks: readonly BlockNode[]): RootContent[] {
  const children: RootContent[] = [];

  for (const block of blocks) {
    switch (block.type) {
      case "paragraph":
        pushParagraph(children, block.children);
        break;
      case "pre":
        children.push(codeBlock(block.children));
        break;
      case "list": {
        const items = block.items
          .map((item): ListItem | null => {
            const paragraph = inlineParagraph(item);
            return paragraph ? { type: "listItem", children: [paragraph] } : null;
          })
          .filter((item): item is ListItem => item !== null);
        if (items.length > 0) children.push(unorderedList(items));
        break;
      }
      case "definition-list": {
        const items = block.items
          .map(renderDefinitionItem)
          .filter((item): item is ListItem => item !== null);
        if (items.length > 0) children.push(unorderedList(items));
        break;
      }
      case "spacer":
        // CommonMark already separates adjacent block nodes with one blank
        // line. It has no semantic blank-row node, so no extra node is needed.
        break;
    }
  }
  return children;
}

/** Represents a man definition list portably as term/description list items. */
function renderDefinitionItem(item: DefinitionListItem): ListItem | null {
  const children: Array<Paragraph | List> = [];
  const terms = item.terms
    .map(inlineParagraph)
    .filter((term): term is Paragraph => term !== null);

  if (terms.length > 0) {
    const termChildren: PhrasingContent[] = [];
    terms.forEach((term, index) => {
      if (index > 0) termChildren.push(hardBreak());
      termChildren.push(...term.children);
    });
    children.push({ type: "paragraph", children: termChildren });
  }

  const description = inlineParagraph(item.description);
  if (description) children.push(description);
  return children.length > 0 ? { type: "listItem", children } : null;
}

// ── TLDR quick reference ───────────────────────────────────

function renderTldr(page: TldrPage): RootContent[] {
  const children: RootContent[] = [heading(2, "TLDR")];

  for (const line of page.description) {
    if (line.trim()) children.push(textParagraph(line));
  }

  if (page.moreInformation) {
    children.push({
      type: "paragraph",
      children: renderMoreInformation(page.moreInformation),
    });
  }

  if (page.examples.length > 0) {
    children.push(heading(3, "Examples"));
    for (const example of page.examples) {
      if (example.description.trim()) {
        children.push({
          type: "paragraph",
          children: [{
            type: "strong",
            children: [{ type: "text", value: example.description }],
          }],
        });
      }
      if (example.command) {
        children.push({
          type: "code",
          lang: "sh",
          value: example.commandParts.map((part) => part.content).join(""),
        });
      }
    }
  }

  children.push({
    type: "paragraph",
    children: [{
      type: "emphasis",
      children: [{
        type: "text",
        value: `tldr-pages · CC BY 4.0 · ${page.platform} · ${page.language}`,
      }],
    }],
  });
  return children;
}

function renderMoreInformation(value: string): PhrasingContent[] {
  const children: PhrasingContent[] = [
    { type: "strong", children: [{ type: "text", value: "More information:" }] },
    { type: "text", value: " " },
  ];
  if (!value.startsWith("http://") && !value.startsWith("https://")) {
    children.push({ type: "text", value });
    return children;
  }

  // tldr prose conventionally puts a full stop after its angle-bracket URL;
  // keep that punctuation outside the link destination.
  const punctuation = value.endsWith(".") ? "." : "";
  const url = punctuation ? value.slice(0, -1) : value;
  children.push({
    type: "link",
    url,
    children: [{ type: "text", value: url }],
  });
  if (punctuation) children.push({ type: "text", value: punctuation });
  return children;
}

// ── Inline conversion ──────────────────────────────────────

function renderInline(nodes: readonly InlineNode[]): PhrasingContent[] {
  const children: PhrasingContent[] = [];

  for (const node of nodes) {
    switch (node.type) {
      case "text":
        children.push(...renderText(node.content));
        break;
      case "bold": {
        const nested = renderInline(node.children);
        pushStyled(children, "strong", nested);
        break;
      }
      case "italic": {
        const nested = renderInline(node.children);
        pushStyled(children, "emphasis", nested);
        break;
      }
      case "code": {
        const value = flattenInline(node.children);
        if (value) children.push({ type: "inlineCode", value });
        break;
      }
      case "break":
        children.push(hardBreak());
        break;
    }
  }
  return children;
}

/**
 * CommonMark cannot put boundary whitespace inside emphasis delimiters. Move
 * it just outside the styled node so the serializer does not resort to noisy
 * numeric character references such as `&#x20;` or `&#x6F;`.
 */
function pushStyled(
  target: PhrasingContent[],
  type: "strong" | "emphasis",
  children: PhrasingContent[],
): void {
  const leading = takeLeadingWhitespace(children);
  const trailing = takeTrailingWhitespace(children);
  if (leading) target.push({ type: "text", value: leading });
  if (children.length > 0) target.push({ type, children });
  if (trailing) target.push({ type: "text", value: trailing });
}

function takeLeadingWhitespace(children: PhrasingContent[]): string {
  let whitespace = "";
  while (children[0]?.type === "text") {
    const first = children[0];
    const match = /^[ \t]+/.exec(first.value);
    if (!match) break;
    whitespace += match[0];
    first.value = first.value.slice(match[0].length);
    if (first.value) break;
    children.shift();
  }
  return whitespace;
}

function takeTrailingWhitespace(children: PhrasingContent[]): string {
  let whitespace = "";
  while (children.at(-1)?.type === "text") {
    const last = children.at(-1)!;
    if (last.type !== "text") break;
    const match = /[ \t]+$/.exec(last.value);
    if (!match) break;
    whitespace = match[0] + whitespace;
    last.value = last.value.slice(0, -match[0].length);
    if (last.value) break;
    children.pop();
  }
  return whitespace;
}

/** Turns renderer-style angle URLs into semantic Markdown autolinks. */
function renderText(value: string): PhrasingContent[] {
  const children: PhrasingContent[] = [];
  const linkPattern = /<{1,2}(https?:\/\/[^<>\s]+)>{1,2}/g;
  let cursor = 0;

  for (const match of value.matchAll(linkPattern)) {
    const index = match.index;
    const url = match[1]!;
    if (index > cursor) {
      children.push({ type: "text", value: value.slice(cursor, index) });
    }
    children.push({
      type: "link",
      url,
      children: [{ type: "text", value: url }],
    });
    cursor = index + match[0].length;
  }

  if (cursor < value.length) children.push({ type: "text", value: value.slice(cursor) });
  return children;
}

/** Code blocks cannot express nested emphasis, so retain their visible text. */
function flattenInline(nodes: readonly InlineNode[]): string {
  return nodes.map((node) => {
    switch (node.type) {
      case "text":
        return node.content;
      case "break":
        return "\n";
      case "bold":
      case "italic":
      case "code":
        return flattenInline(node.children);
    }
  }).join("");
}

// ── Small mdast constructors ───────────────────────────────

function heading(depth: Heading["depth"], title: string): Heading {
  return { type: "heading", depth, children: [{ type: "text", value: title }] };
}

function clampHeadingDepth(depth: number): Heading["depth"] {
  return Math.min(6, Math.max(1, depth)) as Heading["depth"];
}

function textParagraph(value: string): Paragraph {
  return { type: "paragraph", children: [{ type: "text", value }] };
}

function inlineParagraph(nodes: readonly InlineNode[]): Paragraph | null {
  const children = normalizePhrasingLines(renderInline(nodes));
  return children.length > 0 ? { type: "paragraph", children } : null;
}

/** Trims every visual line independently around explicit roff breaks. */
function normalizePhrasingLines(children: PhrasingContent[]): PhrasingContent[] {
  const lines: PhrasingContent[][] = [[]];
  for (const child of children) {
    if (child.type === "break") lines.push([]);
    else lines.at(-1)!.push(child);
  }

  const nonEmpty = lines
    .map(trimPhrasingBoundaries)
    .filter((line) => line.length > 0);
  return nonEmpty.flatMap((line, index) => (
    index === 0 ? line : [hardBreak(), ...line]
  ));
}

/**
 * Removes renderer-only whitespace at block boundaries. Leaving a trailing
 * space makes the CommonMark serializer emit visible `&#x20;` references.
 */
function trimPhrasingBoundaries(children: PhrasingContent[]): PhrasingContent[] {
  trimPhrasingStart(children);
  trimPhrasingEnd(children);
  return children;
}

function trimPhrasingStart(children: PhrasingContent[]): void {
  while (children.length > 0) {
    const first = children[0]!;
    if (first.type === "text") {
      first.value = first.value.trimStart();
      if (!first.value) {
        children.shift();
        continue;
      }
    } else if (first.type === "break") {
      children.shift();
      continue;
    } else if (first.type === "strong" || first.type === "emphasis" || first.type === "link") {
      trimPhrasingStart(first.children);
      if (first.children.length === 0) {
        children.shift();
        continue;
      }
    }
    return;
  }
}

function trimPhrasingEnd(children: PhrasingContent[]): void {
  while (children.length > 0) {
    const last = children.at(-1)!;
    if (last.type === "text") {
      last.value = last.value.trimEnd();
      if (!last.value) {
        children.pop();
        continue;
      }
    } else if (last.type === "break") {
      children.pop();
      continue;
    } else if (last.type === "strong" || last.type === "emphasis" || last.type === "link") {
      trimPhrasingEnd(last.children);
      if (last.children.length === 0) {
        children.pop();
        continue;
      }
    }
    return;
  }
}

function pushParagraph(target: RootContent[], nodes: readonly InlineNode[]): void {
  const paragraph = inlineParagraph(nodes);
  if (paragraph) target.push(paragraph);
}

function codeBlock(nodes: readonly InlineNode[]): Code {
  return { type: "code", value: flattenInline(nodes) };
}

function unorderedList(items: ListItem[]): List {
  return { type: "list", ordered: false, spread: true, children: items };
}

function hardBreak(): Break {
  return { type: "break" };
}
