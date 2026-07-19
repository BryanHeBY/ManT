/**
 * @file Converts the structured manual model into navigable page-search
 * matches. Search is deliberately model-based so it is independent of TUI
 * layout and can target a precise body block.
 */

import type {
  MantBlock,
  MantInline,
  MantSection,
  TldrDocument,
} from "../native";
import { contentBlockId, contentId, TLDR_NAV_ID } from "./ids";

export interface SearchMatch {
  targetId: string;
  sectionId: string;
  title: string;
  blockIndex?: number;
}

function inlineText(nodes: MantInline[]): string {
  return nodes
    .map((node) => {
      switch (node.type) {
        case "text":
        case "code":
          return node.value;
        case "line-break":
          return "\n";
        case "strong":
        case "emphasis":
        case "external-link":
        case "email-link":
        case "manual-reference":
        case "section-reference":
          return inlineText(node.children);
        case "anchor":
          return "";
      }
    })
    .join("");
}

function blockText(block: MantBlock): string {
  switch (block.type) {
    case "paragraph":
    case "preformatted":
      return inlineText(block.children);
    case "list":
      return block.items.flatMap((item) => item.blocks.map(blockText)).join("\n");
    case "definition-list":
      return block.items
        .flatMap((item) => [
          ...item.terms.map(inlineText),
          ...item.description.map(blockText),
        ])
        .join("\n");
    case "table":
      return block.rows.flatMap((row) => row.cells)
        .flatMap((cell) => cell.blocks.map(blockText)).join("\n");
    case "equation":
      return block.value;
    case "unsupported":
      return block.text;
    case "vertical-space":
      return "";
  }
}

function tldrText(page: TldrDocument): string {
  return [
    page.title,
    ...page.description,
    page.moreInformation ?? "",
    ...page.examples.flatMap((example) => [example.description, example.command]),
  ].join("\n");
}

export function findSearchMatches(
  nodes: MantSection[],
  tldr: TldrDocument | undefined,
  query: string,
): SearchMatch[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  if (!normalizedQuery) return [];

  const matches: SearchMatch[] = [];
  if (tldr && tldrText(tldr).toLocaleLowerCase().includes(normalizedQuery)) {
    matches.push({
      targetId: contentId(TLDR_NAV_ID),
      sectionId: TLDR_NAV_ID,
      title: "TLDR QUICK REFERENCE",
    });
  }

  const visit = (node: MantSection) => {
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
