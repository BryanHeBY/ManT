/**
 * @file Converts the structured manual model into navigable page-search
 * matches. Search is deliberately model-based so it is independent of TUI
 * layout and can target a precise body block.
 */

import type { BlockNode, InlineNode, SectionNode } from "../core";
import { tldrPageText, type TldrPage } from "../tldr";
import { contentBlockId, contentId, TLDR_NAV_ID } from "./ids";

export interface SearchMatch {
  targetId: string;
  sectionId: string;
  title: string;
  blockIndex?: number;
}

function inlineText(nodes: InlineNode[]): string {
  return nodes
    .map((node) => {
      if (node.type === "text") return node.content;
      if (node.type === "break") return "\n";
      return inlineText(node.children);
    })
    .join("");
}

function blockText(block: BlockNode): string {
  switch (block.type) {
    case "paragraph":
    case "pre":
      return inlineText(block.children);
    case "list":
      return block.items.map(inlineText).join("\n");
    case "definition-list":
      return block.items
        .flatMap((item) => [...item.terms.map(inlineText), inlineText(item.description)])
        .join("\n");
    case "spacer":
      return "";
  }
}

export function findSearchMatches(
  nodes: SectionNode[],
  tldr: TldrPage | undefined,
  query: string,
): SearchMatch[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  if (!normalizedQuery) return [];

  const matches: SearchMatch[] = [];
  if (tldr && tldrPageText(tldr).toLocaleLowerCase().includes(normalizedQuery)) {
    matches.push({
      targetId: contentId(TLDR_NAV_ID),
      sectionId: TLDR_NAV_ID,
      title: "TLDR QUICK REFERENCE",
    });
  }

  const visit = (node: SectionNode) => {
    if (node.title.toLocaleLowerCase().includes(normalizedQuery)) {
      matches.push({
        targetId: contentId(node.id),
        sectionId: node.id,
        title: node.title,
      });
    }
    node.blocks.forEach((block, blockIndex) => {
      if (!blockText(block).toLocaleLowerCase().includes(normalizedQuery)) return;
      matches.push({
        targetId: contentBlockId(node.id, blockIndex),
        sectionId: node.id,
        title: node.title,
        blockIndex,
      });
    });
    for (const child of node.children) visit(child);
  };

  for (const node of nodes) visit(node);
  return matches;
}
