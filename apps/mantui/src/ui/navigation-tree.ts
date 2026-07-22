/**
 * @file Builds and traverses the sidebar's section and semantic-option tree.
 * Keeping it UI-framework-free makes tree behavior and terminal label
 * formatting easy to test outside stateful UI code.
 */

import type { MantBlock, MantDefinitionItem, MantSection } from "../native";

export type NavigationNodeKind = "section" | "entry-group" | "option";

/** One sidebar node, including virtual groups and semantic manual entries. */
export interface NavigationNode {
  id: string;
  title: string;
  kind: NavigationNodeKind;
  /** ID of the section heading or inline anchor rendered in the content pane. */
  targetId: string;
  children: NavigationNode[];
}

export interface FlatNode {
  node: NavigationNode;
  depth: number;
  hasChildren: boolean;
  isLast: boolean;
  /** Whether each ancestor has another visible sibling after it. */
  ancestorHasNext: boolean[];
}

export function flattenVisibleNodes(
  nodes: NavigationNode[],
  expanded: ReadonlySet<string>,
  depth = 0,
  ancestorHasNext: boolean[] = [],
): FlatNode[] {
  const result: FlatNode[] = [];
  for (let index = 0; index < nodes.length; index++) {
    const node = nodes[index]!;
    const isLast = index === nodes.length - 1;
    const hasChildren = node.children.length > 0;
    result.push({ node, depth, hasChildren, isLast, ancestorHasNext });
    if (hasChildren && expanded.has(node.id)) {
      result.push(
        ...flattenVisibleNodes(node.children, expanded, depth + 1, [
          ...ancestorHasNext,
          !isLast,
        ]),
      );
    }
  }
  return result;
}

/** Build the sidebar model without mutating the renderer-neutral AST. */
export function buildNavigationNodes(sections: MantSection[]): NavigationNode[] {
  return sections.map((section) => {
    const entries: MantDefinitionItem[] = [];
    collectDefinitionEntries(section.blocks, entries);
    const children: NavigationNode[] = [];
    if (entries.length > 0) {
      children.push({
        id: `__mant-options__${section.id}`,
        title: `OPTIONS (${entries.length})`,
        kind: "entry-group",
        targetId: section.id,
        children: entries.flatMap((entry) => entry.identity
          ? [{
              id: entry.identity.id,
              title: entry.identity.names.join(", "),
              kind: "option" as const,
              targetId: entry.identity.id,
              children: [],
            }]
          : []),
      });
    }
    children.push(...buildNavigationNodes(section.children));
    return {
      id: section.id,
      title: section.title,
      kind: "section" as const,
      targetId: section.id,
      children,
    };
  });
}

function collectDefinitionEntries(
  blocks: MantBlock[],
  output: MantDefinitionItem[],
): void {
  for (const block of blocks) {
    switch (block.type) {
      case "definition-list":
        for (const item of block.items) {
          if (item.identity?.role === "option") output.push(item);
          collectDefinitionEntries(item.description, output);
        }
        break;
      case "list":
        for (const item of block.items) collectDefinitionEntries(item.blocks, output);
        break;
      case "table":
        for (const row of block.rows) {
          for (const cell of row.cells) collectDefinitionEntries(cell.blocks, output);
        }
        break;
      case "paragraph":
      case "preformatted":
      case "equation":
      case "vertical-space":
      case "unsupported":
        break;
    }
  }
}

export function findNodeById<T extends { id: string; children: T[] }>(
  nodes: T[],
  id: string,
): T | null {
  for (const node of nodes) {
    if (node.id === id) return node;
    const found = findNodeById(node.children, id);
    if (found) return found;
  }
  return null;
}

export function findParentById(
  nodes: NavigationNode[],
  id: string,
  parent: NavigationNode | null = null,
): NavigationNode | null {
  for (const node of nodes) {
    if (node.id === id) return parent;
    const found = findParentById(node.children, id, node);
    if (found !== null) return found;
  }
  return null;
}

/** Returns a node's ancestry in document order, including the node itself. */
export function findNodePath(
  nodes: MantSection[],
  id: string,
  path: string[] = [],
): string[] | null {
  for (const node of nodes) {
    const nextPath = [...path, node.id];
    if (node.id === id) return nextPath;
    const found = findNodePath(node.children, id, nextPath);
    if (found) return found;
  }
  return null;
}

export function sectionIdsInDocumentOrder(nodes: MantSection[]): string[] {
  const ids: string[] = [];
  const visit = (node: MantSection) => {
    ids.push(node.id);
    for (const child of node.children) visit(child);
  };
  for (const node of nodes) visit(node);
  return ids;
}

export function collectBranchIds(nodes: NavigationNode[]): Set<string> {
  const ids = new Set<string>();
  const visit = (node: NavigationNode) => {
    if (node.children.length > 0) ids.add(node.id);
    for (const child of node.children) visit(child);
  };
  for (const node of nodes) visit(node);
  return ids;
}

export function treePrefix({ depth, isLast, ancestorHasNext }: FlatNode): string {
  if (depth === 0) return "";

  const ancestorGuides = ancestorHasNext
    .slice(0, -1)
    .map((hasNext) => (hasNext ? "│ " : "  "))
    .join("");
  return `${ancestorGuides}${isLast ? "╰─" : "├─"}`;
}

/** Keeps guide columns visible after a selected navigation label wraps. */
export function treeContinuationPrefix({ ancestorHasNext }: FlatNode): string {
  return `${ancestorHasNext.map((hasNext) => (hasNext ? "│ " : "  ")).join("")}  `;
}

export function terminalColumnWidth(text: string): number {
  let width = 0;
  for (const character of text) {
    const codePoint = character.codePointAt(0) ?? 0;
    if (codePoint <= 0x1f || (codePoint >= 0x7f && codePoint <= 0x9f)) continue;
    width +=
      codePoint >= 0x1100 &&
      (codePoint <= 0x115f ||
        codePoint === 0x2329 ||
        codePoint === 0x232a ||
        (codePoint >= 0x2e80 && codePoint <= 0xa4cf) ||
        (codePoint >= 0xac00 && codePoint <= 0xd7a3) ||
        (codePoint >= 0xf900 && codePoint <= 0xfaff) ||
        (codePoint >= 0xfe10 && codePoint <= 0xfe19) ||
        (codePoint >= 0xfe30 && codePoint <= 0xfe6f) ||
        (codePoint >= 0xff00 && codePoint <= 0xff60) ||
        (codePoint >= 0xffe0 && codePoint <= 0xffe6) ||
        (codePoint >= 0x20000 && codePoint <= 0x3fffd))
        ? 2
        : 1;
  }
  return width;
}

function splitLongNavigationWord(word: string, maxColumns: number): string[] {
  const lines: string[] = [];
  let line = "";
  let lineWidth = 0;

  for (const character of word) {
    const characterWidth = terminalColumnWidth(character);
    if (line && lineWidth + characterWidth > maxColumns) {
      lines.push(line);
      line = "";
      lineWidth = 0;
    }
    line += character;
    lineWidth += characterWidth;
  }
  if (line) lines.push(line);
  return lines;
}

/** Wraps selected titles while retaining a prefix column on continuation rows. */
export function wrapNavigationTitle(title: string, maxColumns: number): string[] {
  const availableColumns = Math.max(1, maxColumns);
  const words = title.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [""];

  const lines: string[] = [];
  let line = "";
  for (const word of words) {
    const candidate = line ? `${line} ${word}` : word;
    if (terminalColumnWidth(candidate) <= availableColumns) {
      line = candidate;
      continue;
    }
    if (line) lines.push(line);

    const fragments = splitLongNavigationWord(word, availableColumns);
    line = fragments.pop() ?? "";
    lines.push(...fragments);
  }
  if (line) lines.push(line);
  return lines;
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
