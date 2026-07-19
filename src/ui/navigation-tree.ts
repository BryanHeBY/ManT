/**
 * @file Provides pure section-tree traversal and terminal label formatting for
 * the manual sidebar. Keeping it UI-framework-free makes tree behavior easy
 * to test and reuse from stateful UI code.
 */

import type { MantSection } from "../native";

export interface FlatNode {
  node: MantSection;
  depth: number;
  hasChildren: boolean;
  isLast: boolean;
  /** Whether each ancestor has another visible sibling after it. */
  ancestorHasNext: boolean[];
}

export function flattenVisibleNodes(
  nodes: MantSection[],
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

export function findNodeById(nodes: MantSection[], id: string): MantSection | null {
  for (const node of nodes) {
    if (node.id === id) return node;
    const found = findNodeById(node.children, id);
    if (found) return found;
  }
  return null;
}

export function findParentById(
  nodes: MantSection[],
  id: string,
  parent: MantSection | null = null,
): MantSection | null {
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

export function collectBranchIds(nodes: MantSection[]): Set<string> {
  const ids = new Set<string>();
  const visit = (node: MantSection) => {
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
