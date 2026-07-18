import {
  createCliRenderer,
  type BoxRenderable,
  type InputRenderable,
  type MouseEvent as TuiMouseEvent,
  type ScrollBoxRenderable,
} from "@opentui/core";
import { createRoot, useKeyboard, useTerminalDimensions } from "@opentui/react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { QueryResult } from "../query";
import type { BlockNode, InlineNode, SectionNode } from "../core";
import { tldrPageText, type TldrCommandPart, type TldrPage } from "../tldr";
import { Pre } from "./Pre";
import { renderSearchHighlights } from "./search-highlight";

interface AppProps {
  result: QueryResult;
  onQuit: () => void;
}

const DEFAULT_NAV_WIDTH = 32;
const MIN_NAV_WIDTH = 24;
const MIN_CONTENT_WIDTH = 32;
const TLDR_NAV_ID = "tldr-quick-reference";
const NAVIGATION_SYNC_DELAY_MS = 180;

const MENU_BAR = [
  { id: "file", label: "File", left: 0 },
  { id: "view", label: "View", left: 6 },
  { id: "navigate", label: "Navigate", left: 12 },
  { id: "search", label: "Search", left: 22 },
  { id: "help", label: "Help", left: 30 },
] as const;

type MenuId = (typeof MENU_BAR)[number]["id"];

interface MenuEntry {
  label: string;
  shortcut?: string;
  checked?: boolean;
  action: () => void;
}

interface SearchMatch {
  targetId: string;
  sectionId: string;
  title: string;
  blockIndex?: number;
}

interface FlatNode {
  node: SectionNode;
  depth: number;
  hasChildren: boolean;
  isLast: boolean;
  /** Whether each ancestor has another visible sibling after it. */
  ancestorHasNext: boolean[];
}

function flattenVisibleNodes(
  nodes: SectionNode[],
  expanded: Set<string>,
  depth = 0,
  ancestorHasNext: boolean[] = []
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
        ])
      );
    }
  }
  return result;
}

function findNodeById(nodes: SectionNode[], id: string): SectionNode | null {
  for (const node of nodes) {
    if (node.id === id) return node;
    const found = findNodeById(node.children, id);
    if (found) return found;
  }
  return null;
}

function findParentById(
  nodes: SectionNode[],
  id: string,
  parent: SectionNode | null = null
): SectionNode | null {
  for (const node of nodes) {
    if (node.id === id) return parent;
    const found = findParentById(node.children, id, node);
    if (found !== null) return found;
  }
  return null;
}

/** Return a node's ancestry in document order, including the node itself. */
function findNodePath(nodes: SectionNode[], id: string, path: string[] = []): string[] | null {
  for (const node of nodes) {
    const nextPath = [...path, node.id];
    if (node.id === id) return nextPath;
    const found = findNodePath(node.children, id, nextPath);
    if (found) return found;
  }
  return null;
}

function sectionIdsInDocumentOrder(nodes: SectionNode[]): string[] {
  const ids: string[] = [];
  const visit = (node: SectionNode) => {
    ids.push(node.id);
    for (const child of node.children) visit(child);
  };
  for (const node of nodes) visit(node);
  return ids;
}

function navId(id: string): string {
  return `nav-${id}`;
}

function contentId(id: string): string {
  return `content-${id}`;
}

function contentBlockId(sectionId: string, blockIndex: number): string {
  return `${contentId(sectionId)}-block-${blockIndex}`;
}

function treePrefix({ depth, isLast, ancestorHasNext }: FlatNode): string {
  if (depth === 0) return "";

  const ancestorGuides = ancestorHasNext
    .slice(0, -1)
    .map((hasNext) => (hasNext ? "│ " : "  "))
    .join("");
  return `${ancestorGuides}${isLast ? "╰─" : "├─"}`;
}

/**
 * Keep ancestor guide columns visible after a selected navigation label wraps.
 * The final two spaces reserve the current node's branch/disclosure columns.
 */
function treeContinuationPrefix({ ancestorHasNext }: FlatNode): string {
  return `${ancestorHasNext.map((hasNext) => (hasNext ? "│ " : "  ")).join("")}  `;
}

function terminalColumnWidth(text: string): number {
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

/** Wrap a selected navigation title while retaining a prefix column per row. */
function wrapNavigationTitle(title: string, maxColumns: number): string[] {
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

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
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
    case "spacer":
      return "";
  }
}

function findSearchMatches(
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
    for (const child of node.children) {
      visit(child);
    }
  };

  for (const node of nodes) visit(node);
  return matches;
}

function collectBranchIds(nodes: SectionNode[]): Set<string> {
  const ids = new Set<string>();
  const visit = (node: SectionNode) => {
    if (node.children.length > 0) ids.add(node.id);
    for (const child of node.children) visit(child);
  };
  for (const node of nodes) visit(node);
  return ids;
}

function splitByBreak(nodes: InlineNode[]): InlineNode[][] {
  const segments: InlineNode[][] = [[]];
  for (const node of nodes) {
    if (node.type === "break") {
      segments.push([]);
    } else {
      segments[segments.length - 1]!.push(node);
    }
  }
  return segments.filter((s) => s.length > 0).map(trimSegmentWhitespace);
}

function trimSegmentWhitespace(nodes: InlineNode[]): InlineNode[] {
  if (nodes.length === 0) return nodes;

  const trimmed = [...nodes];
  const first = trimmed[0];
  if (first?.type === "text") {
    first.content = first.content.replace(/^\s+/, "");
  }

  const last = trimmed[trimmed.length - 1];
  if (last?.type === "text") {
    last.content = last.content.replace(/\s+$/, "");
  }

  return trimmed;
}

/**
 * Merges consecutive paragraph and list blocks into a single `<text>` element
 * to avoid creating one TextBuffer per block (which can exceed the native
 * TextBuffer limit on large man pages like gcc ~16k blocks).
 *
 * Pre blocks are kept separate because they need char-wrap mode and a
 * distinct visual style.
 */
function renderBlockNodes(
  blocks: BlockNode[],
  baseIndent = 0,
  searchQuery = "",
  sectionId?: string,
  activeBlockIndex?: number,
): ReactNode[] {
  const result: ReactNode[] = [];
  let inlineBuffer: ReactNode[] = [];
  let bufferIndent = 0;
  let keyCounter = 0;
  let inlineKey = 0;

  function renderInlineNodes(nodes: InlineNode[]): ReactNode[] {
    return nodes.map((node) => {
      const key = inlineKey++;
      switch (node.type) {
        case "text":
          return renderSearchHighlights(node.content, searchQuery, `inline-${key}`);
        case "bold":
          return (
            <span key={key} fg="#cdd6f4">
              <b>{renderInlineNodes(node.children)}</b>
            </span>
          );
        case "italic":
          return (
            <span key={key} fg="#7f849c">
              <u>{renderInlineNodes(node.children)}</u>
            </span>
          );
        case "code":
          return <Pre key={key} children={node.children} searchQuery={searchQuery} />;
        case "break":
          return null;
        default:
          return null;
      }
    });
  }

  function flushInline(anchorId?: string) {
    if (inlineBuffer.length === 0) return;
    result.push(
      <box
        key={`merged-${keyCounter++}`}
        {...(anchorId ? { id: anchorId } : {})}
        paddingLeft={bufferIndent}
        shouldFill={true}
      >
        <text fg="#a6adc8" wrapMode="word">
          {inlineBuffer}
        </text>
      </box>
    );
    inlineBuffer = [];
  }

  for (let blockIndex = 0; blockIndex < blocks.length; blockIndex++) {
    const block = blocks[blockIndex]!;
    const isActiveBlock = sectionId !== undefined && blockIndex === activeBlockIndex;
    if (isActiveBlock) flushInline();
    switch (block.type) {
      case "paragraph": {
        if (inlineBuffer.length > 0 && baseIndent + block.indent !== bufferIndent) {
          flushInline();
        }
        if (inlineBuffer.length === 0) {
          bufferIndent = baseIndent + block.indent;
        }
        const segments = splitByBreak(block.children);
        for (const segment of segments) {
          inlineBuffer.push(...renderInlineNodes(segment));
          inlineBuffer.push("\n");
        }
        if (isActiveBlock) flushInline(contentBlockId(sectionId, blockIndex));
        break;
      }
      case "list": {
        if (inlineBuffer.length > 0 && baseIndent + block.indent !== bufferIndent) {
          flushInline();
        }
        if (inlineBuffer.length === 0) {
          bufferIndent = baseIndent + block.indent;
        }
        for (const item of block.items) {
          inlineBuffer.push(<span key={`bullet-${inlineKey++}`} fg="#94e2d5">{"• "}</span>);
          inlineBuffer.push(...renderInlineNodes(item));
          inlineBuffer.push("\n");
        }
        if (isActiveBlock) flushInline(contentBlockId(sectionId, blockIndex));
        break;
      }
      case "pre": {
        flushInline();
        const pre = (
          <Pre
            key={`pre-${keyCounter++}`}
            children={block.children}
            block
            indent={baseIndent + block.indent}
            searchQuery={searchQuery}
          />
        );
        result.push(
          isActiveBlock
            ? <box key={`pre-anchor-${keyCounter++}`} id={contentBlockId(sectionId, blockIndex)}>{pre}</box>
            : pre
        );
        // Display blocks are separated from the next paragraph in both groff
        // and mandoc output.  The parser may also provide an explicit spacer
        // before the block; avoid adding a duplicate only when one follows it.
        if (blocks[blockIndex + 1]?.type !== "spacer" && blockIndex < blocks.length - 1) {
          result.push(<box key={`pre-gap-${keyCounter++}`} height={1} />);
        }
        break;
      }
      case "spacer": {
        flushInline();
        result.push(<box key={`spacer-${keyCounter++}`} height={1} />);
        break;
      }
      default:
        break;
    }
  }
  flushInline();
  return result;
}

function SectionContent({
  node,
  baseIndent = 3,
  searchQuery = "",
  activeSearchSectionId,
  activeBlockIndex,
}: {
  node: SectionNode;
  baseIndent?: number;
  searchQuery?: string;
  activeSearchSectionId?: string | undefined;
  activeBlockIndex?: number | undefined;
}) {
  return (
    <box flexDirection="column" gap={0}>
      <text id={contentId(node.id)} fg="#94e2d5">
        <b>{renderSearchHighlights(node.title, searchQuery, `heading-${node.id}`)}</b>
      </text>
      {renderBlockNodes(
        node.blocks,
        baseIndent,
        searchQuery,
        node.id,
        activeSearchSectionId === node.id ? activeBlockIndex : undefined,
      )}
      <box flexDirection="column" gap={0}>
        {node.children.map((child: SectionNode) => (
          <SectionContent
            key={child.id}
            node={child}
            baseIndent={baseIndent + 4}
            searchQuery={searchQuery}
            activeSearchSectionId={activeSearchSectionId}
            activeBlockIndex={activeBlockIndex}
          />
        ))}
      </box>
    </box>
  );
}

function TldrCommand({ parts, searchQuery }: { parts: TldrCommandPart[]; searchQuery: string }) {
  return (
    <text fg="#cdd6f4" wrapMode="char">
      {parts.map((part, index) => (
        <span key={index} fg={part.type === "placeholder" ? "#f9e2af" : "#cdd6f4"}>
          {renderSearchHighlights(part.content, searchQuery, `tldr-command-${index}`)}
        </span>
      ))}
    </text>
  );
}

function TldrQuickReference({ page, searchQuery }: { page: TldrPage; searchQuery: string }) {
  return (
    <box
      id={contentId(TLDR_NAV_ID)}
      flexDirection="column"
      backgroundColor="#28243a"
      border={["top", "right", "bottom", "left"]}
      borderColor="#cba6f7"
      paddingLeft={1}
      paddingRight={1}
      paddingTop={1}
      paddingBottom={1}
    >
      <text fg="#cba6f7">
        <b>{renderSearchHighlights(`TLDR QUICK REFERENCE · ${page.title}`, searchQuery, "tldr-title")}</b>
      </text>
      {page.description.map((line, index) => (
        <text key={`description-${index}`} fg="#bac2de" wrapMode="word">
          {renderSearchHighlights(line, searchQuery, `tldr-description-${index}`)}
        </text>
      ))}
      {page.examples.map((example, index) => (
        <box key={`example-${index}`} flexDirection="column" paddingTop={index === 0 ? 1 : 0}>
          <text fg="#a6e3a1" wrapMode="word">
            {renderSearchHighlights(example.description, searchQuery, `tldr-example-${index}`)}
          </text>
          {example.command && (
            <box paddingLeft={2}>
              <TldrCommand parts={example.commandParts} searchQuery={searchQuery} />
            </box>
          )}
        </box>
      ))}
      {page.moreInformation && (
        <text fg="#89b4fa" wrapMode="char">
          {renderSearchHighlights(`More information: ${page.moreInformation}`, searchQuery, "tldr-more-information")}
        </text>
      )}
      <text fg="#7f849c">
        {`tldr-pages · CC BY 4.0 · ${page.platform} · ${page.language}`}
      </text>
    </box>
  );
}

export function App({ result, onQuit }: AppProps) {
  const [selectedId, setSelectedId] = useState<string>(
    result.tldr ? TLDR_NAV_ID : result.sections[0]?.id ?? ""
  );
  const [expanded, setExpanded] = useState<Set<string>>(() => {
    const initial = new Set<string>();
    for (const section of result.sections) {
      initial.add(section.id);
    }
    return initial;
  });
  const [isNavigationVisible, setIsNavigationVisible] = useState(true);
  const [navigationWidth, setNavigationWidth] = useState(DEFAULT_NAV_WIDTH);
  const [openMenu, setOpenMenu] = useState<MenuId | null>(null);
  const [menuCursor, setMenuCursor] = useState(0);
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  // Keep the editing buffer separate from the applied query. Updating the
  // latter re-renders and searches the entire manual, so it must happen only
  // after explicit confirmation.
  const [searchDraft, setSearchDraft] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [searchIndex, setSearchIndex] = useState(0);
  const [pendingSearchTarget, setPendingSearchTarget] = useState<string | null>(null);
  const [isHelpOpen, setIsHelpOpen] = useState(false);
  const contentScrollRef = useRef<ScrollBoxRenderable | null>(null);
  const navScrollRef = useRef<ScrollBoxRenderable | null>(null);
  const searchInputRef = useRef<InputRenderable | null>(null);
  const navigationSyncTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const navigationResizeRef = useRef<{
    startX: number;
    startWidth: number;
  } | null>(null);
  const { width: terminalWidth, height: terminalHeight } = useTerminalDimensions();
  const maxNavigationWidth = Math.max(
    MIN_NAV_WIDTH,
    terminalWidth - MIN_CONTENT_WIDTH - 1
  );

  useEffect(() => {
    setNavigationWidth((currentWidth) =>
      clamp(currentWidth, MIN_NAV_WIDTH, maxNavigationWidth)
    );
  }, [maxNavigationWidth]);

  const visibleNodes = useMemo(
    () => flattenVisibleNodes(result.sections, expanded),
    [result.sections, expanded]
  );
  const navigationItems = useMemo(
    () => [
      ...(result.tldr
        ? [{ id: TLDR_NAV_ID, title: "TLDR QUICK REFERENCE" }]
        : []),
      ...visibleNodes.map(({ node }) => ({ id: node.id, title: node.title })),
    ],
    [result.tldr, visibleNodes]
  );
  const selectedNavigationItem = navigationItems.find((item) => item.id === selectedId);
  const searchMatches = useMemo(
    () => findSearchMatches(result.sections, result.tldr, searchQuery),
    [result.sections, result.tldr, searchQuery]
  );
  const branchIds = useMemo(
    () => collectBranchIds(result.sections),
    [result.sections]
  );
  const contentSectionIds = useMemo(
    () => [
      ...(result.tldr ? [TLDR_NAV_ID] : []),
      ...sectionIdsInDocumentOrder(result.sections),
    ],
    [result.sections, result.tldr]
  );

  const scrollToContent = (targetId: string) => {
    const scrollbox = contentScrollRef.current;
    if (!scrollbox) return;

    const target = scrollbox.content.findDescendantById(targetId);
    if (!target) return;

    // scrollChildIntoView deliberately chooses the nearest edge, which often
    // leaves a newly selected later section at the bottom of the viewport.
    // Translate the heading's current screen coordinate into a scroll offset
    // so every selected section starts at the top of the content viewport.
    const offsetToViewportTop = target.y - scrollbox.viewport.y;
    scrollbox.scrollTo({
      x: scrollbox.scrollLeft,
      y: Math.max(0, scrollbox.scrollTop + offsetToViewportTop),
    });
  };

  const scrollToNode = (id: string) => scrollToContent(contentId(id));

  const syncNavigationToContent = () => {
    const scrollbox = contentScrollRef.current;
    if (!scrollbox || contentSectionIds.length === 0) return;

    // The selected row is the last heading at or above the first content row.
    // This runs only after scrolling becomes idle, never for every wheel tick.
    let activeId = contentSectionIds[0]!;
    for (const id of contentSectionIds) {
      const heading = scrollbox.content.findDescendantById(contentId(id));
      if (!heading) continue;
      if (heading.y > scrollbox.viewport.y) break;
      activeId = id;
    }

    setSelectedId((current) => (current === activeId ? current : activeId));

    // Reveal a subsection that reaches the top while its ancestors are folded.
    if (activeId !== TLDR_NAV_ID) {
      const path = findNodePath(result.sections, activeId);
      if (path) {
        setExpanded((current) => {
          let changed = false;
          const next = new Set(current);
          for (const id of path) {
            if (!next.has(id)) {
              next.add(id);
              changed = true;
            }
          }
          return changed ? next : current;
        });
      }
    }
  };

  const scheduleNavigationSync = () => {
    if (navigationSyncTimerRef.current) {
      clearTimeout(navigationSyncTimerRef.current);
    }
    navigationSyncTimerRef.current = setTimeout(() => {
      navigationSyncTimerRef.current = null;
      syncNavigationToContent();
    }, NAVIGATION_SYNC_DELAY_MS);
  };

  const selectSection = (id: string) => {
    setSelectedId(id);
    scrollToNode(id);
    navScrollRef.current?.scrollChildIntoView(navId(id));
  };

  const selectSearchMatch = (index: number) => {
    if (searchMatches.length === 0) return;
    const nextIndex = ((index % searchMatches.length) + searchMatches.length) % searchMatches.length;
    const match = searchMatches[nextIndex]!;
    setSearchIndex(nextIndex);
    setSelectedId(match.sectionId);
    navScrollRef.current?.scrollChildIntoView(navId(match.sectionId));
    setPendingSearchTarget(match.targetId);
  };

  const selectRelativeSection = (offset: number) => {
    const currentIndex = navigationItems.findIndex((item) => item.id === selectedId);
    const nextIndex = clamp(
      currentIndex + offset,
      0,
      Math.max(navigationItems.length - 1, 0)
    );
    const next = navigationItems[nextIndex];
    if (next) selectSection(next.id);
  };

  const openSearch = () => {
    setOpenMenu(null);
    setIsHelpOpen(false);
    setSearchDraft(searchQuery);
    setIsSearchOpen(true);
  };

  const closeSearch = () => {
    setSearchDraft(searchQuery);
    setIsSearchOpen(false);
  };

  const submitSearch = () => {
    if (searchDraft === searchQuery) {
      selectSearchMatch(searchIndex + 1);
      return;
    }

    const matches = findSearchMatches(result.sections, result.tldr, searchDraft);
    setSearchQuery(searchDraft);
    setSearchIndex(0);
    if (matches[0]) {
      setSelectedId(matches[0].sectionId);
      navScrollRef.current?.scrollChildIntoView(navId(matches[0].sectionId));
      setPendingSearchTarget(matches[0].targetId);
    }
  };

  const expandAll = () => setExpanded(new Set(branchIds));
  const collapseAll = () => setExpanded(new Set());

  const navigateToParent = () => {
    const parent = findParentById(result.sections, selectedId);
    if (parent) selectSection(parent.id);
  };

  const navigateToFirstChild = () => {
    const node = findNodeById(result.sections, selectedId);
    if (node?.children[0]) selectSection(node.children[0].id);
  };

  const expandCurrentSection = () => {
    const node = findNodeById(result.sections, selectedId);
    if (!node?.children.length) return;
    setExpanded((current) => new Set(current).add(node.id));
  };

  const collapseCurrentSection = () => {
    const node = findNodeById(result.sections, selectedId);
    if (!node?.children.length) return;
    setExpanded((current) => {
      const next = new Set(current);
      next.delete(node.id);
      return next;
    });
  };

  const menuEntries: Record<MenuId, MenuEntry[]> = {
    file: [
      { label: "Quit", shortcut: "q", action: onQuit },
    ],
    view: [
      {
        label: "Sidebar",
        shortcut: "",
        checked: isNavigationVisible,
        action: () => setIsNavigationVisible((visible) => !visible),
      },
      {
        label: "Reset Sidebar Width",
        action: () => setNavigationWidth(DEFAULT_NAV_WIDTH),
      },
      { label: "Expand All", shortcut: "", action: expandAll },
      { label: "Collapse All", shortcut: "", action: collapseAll },
    ],
    navigate: [
      { label: "Previous Section", shortcut: "↑ / k", action: () => selectRelativeSection(-1) },
      { label: "Next Section", shortcut: "↓ / j", action: () => selectRelativeSection(1) },
      { label: "Parent Section", shortcut: "← / h", action: navigateToParent },
      { label: "First Child", shortcut: "→ / l", action: navigateToFirstChild },
      { label: "First Section", shortcut: "", action: () => selectRelativeSection(-navigationItems.length) },
      { label: "Last Section", shortcut: "", action: () => selectRelativeSection(navigationItems.length) },
    ],
    search: [
      { label: "Find in Page…", shortcut: "Ctrl+F / /", action: openSearch },
      {
        label: "Find Next",
        shortcut: "n",
        action: () => selectSearchMatch(searchIndex + 1),
      },
      {
        label: "Find Previous",
        shortcut: "N",
        action: () => selectSearchMatch(searchIndex - 1),
      },
    ],
    help: [
      {
        label: "Keyboard Shortcuts",
        shortcut: "?",
        action: () => {
          setOpenMenu(null);
          setIsSearchOpen(false);
          setIsHelpOpen(true);
        },
      },
    ],
  };

  const activeMenuEntries = openMenu ? menuEntries[openMenu] : [];

  const openMenuById = (menu: MenuId) => {
    setIsSearchOpen(false);
    setIsHelpOpen(false);
    setOpenMenu((current) => (current === menu ? null : menu));
    setMenuCursor(0);
  };

  const activateMenuEntry = (entry: MenuEntry) => {
    entry.action();
    setOpenMenu(null);
    setMenuCursor(0);
  };

  useEffect(() => {
    if (isSearchOpen) searchInputRef.current?.focus();
  }, [isSearchOpen]);

  useEffect(
    () => () => {
      if (navigationSyncTimerRef.current) {
        clearTimeout(navigationSyncTimerRef.current);
      }
    },
    []
  );

  useEffect(() => {
    // A selected long title may grow from one row into several after React
    // commits.  Re-run the visibility adjustment after that layout change.
    if (selectedId) navScrollRef.current?.scrollChildIntoView(navId(selectedId));
  }, [selectedId, visibleNodes]);

  useEffect(() => {
    if (!pendingSearchTarget) return;
    // The block anchor is inserted by the same commit that changes the
    // selected match. Wait for OpenTUI to finish its next layout pass before
    // reading its y coordinate.
    const timer = setTimeout(() => {
      scrollToContent(pendingSearchTarget);
      setPendingSearchTarget(null);
    }, 16);
    return () => clearTimeout(timer);
  }, [pendingSearchTarget, searchIndex, searchQuery]);

  const attachSectionClick = (id: string, hasChildren: boolean) => {
    return (el: BoxRenderable | null) => {
      if (!el) return;
      el.onMouseDown = () => {
        if (hasChildren && selectedId === id) {
          toggleExpanded(id);
        } else {
          selectSection(id);
          if (hasChildren) {
            setExpanded((prev) => {
              const next = new Set(prev);
              next.add(id);
              return next;
            });
          }
        }
      };
    };
  };

  const startNavigationResize = (event: TuiMouseEvent) => {
    if (!isNavigationVisible || Math.abs(event.x - navigationWidth) > 1) {
      return;
    }

    event.stopPropagation();
    event.preventDefault();
    navigationResizeRef.current = {
      startX: event.x,
      startWidth: navigationWidth,
    };
  };

  const resizeNavigation = (event: TuiMouseEvent) => {
    const resize = navigationResizeRef.current;
    if (!resize) return;

    event.stopPropagation();
    event.preventDefault();
    const delta = event.x - resize.startX;
    setNavigationWidth(
      clamp(resize.startWidth + delta, MIN_NAV_WIDTH, maxNavigationWidth)
    );
  };

  const finishNavigationResize = (event: TuiMouseEvent) => {
    const resize = navigationResizeRef.current;
    if (!resize) return;

    event.stopPropagation();
    event.preventDefault();
    navigationResizeRef.current = null;
  };

  useKeyboard((e) => {
    if (isHelpOpen) {
      if (e.name === "escape" || e.name === "?") {
        e.preventDefault();
        setIsHelpOpen(false);
      }
      return;
    }

    if (isSearchOpen) {
      if (e.name === "escape") {
        e.preventDefault();
        closeSearch();
      } else if (e.name === "return" || e.name === "enter") {
        e.preventDefault();
        submitSearch();
      } else if (e.name === "down" && searchDraft === searchQuery) {
        e.preventDefault();
        selectSearchMatch(searchIndex + 1);
      } else if (e.name === "up" && searchDraft === searchQuery) {
        e.preventDefault();
        selectSearchMatch(searchIndex - 1);
      }
      return;
    }

    if (openMenu) {
      const currentMenuIndex = MENU_BAR.findIndex((menu) => menu.id === openMenu);
      if (e.name === "escape") {
        e.preventDefault();
        setOpenMenu(null);
      } else if (e.name === "left" || e.name === "right") {
        e.preventDefault();
        const direction = e.name === "left" ? -1 : 1;
        const nextMenuIndex =
          (currentMenuIndex + direction + MENU_BAR.length) % MENU_BAR.length;
        setOpenMenu(MENU_BAR[nextMenuIndex]!.id);
        setMenuCursor(0);
      } else if (e.name === "down") {
        e.preventDefault();
        setMenuCursor((current) => (current + 1) % activeMenuEntries.length);
      } else if (e.name === "up") {
        e.preventDefault();
        setMenuCursor(
          (current) => (current - 1 + activeMenuEntries.length) % activeMenuEntries.length
        );
      } else if (e.name === "return" || e.name === "enter" || e.name === "space") {
        e.preventDefault();
        const entry = activeMenuEntries[menuCursor];
        if (entry) activateMenuEntry(entry);
      }
      return;
    }

    if (e.name === "f10") {
      e.preventDefault();
      openMenuById("file");
      return;
    }

    if ((e.ctrl && e.name === "f") || e.name === "/") {
      e.preventDefault();
      openSearch();
      return;
    }

    if (e.name === "?") {
      e.preventDefault();
      setIsHelpOpen(true);
      return;
    }

    if (e.name === "n" && searchMatches.length > 0) {
      e.preventDefault();
      selectSearchMatch(searchIndex + (e.shift ? -1 : 1));
      return;
    }

    if (e.name === "q" || e.name === "Q") {
      onQuit();
      return;
    }

    if (e.name === "j" || e.name === "down") {
      selectRelativeSection(1);
    } else if (e.name === "k" || e.name === "up") {
      selectRelativeSection(-1);
    } else if (e.name === "h" || e.name === "left") {
      const node = findNodeById(result.sections, selectedId);
      if (node && expanded.has(node.id) && node.children.length > 0) {
        collapseCurrentSection();
      } else {
        navigateToParent();
      }
    } else if (e.name === "l" || e.name === "right") {
      const node = findNodeById(result.sections, selectedId);
      if (node && node.children.length > 0) {
        if (!expanded.has(node.id)) {
          expandCurrentSection();
        } else {
          navigateToFirstChild();
        }
      }
    } else if (e.name === "return" || e.name === "enter" || e.name === "space") {
      const node = findNodeById(result.sections, selectedId);
      if (node?.children.length) {
        toggleExpanded(node.id);
      }
    } else if (e.name === "d" || e.name === "pagedown") {
      contentScrollRef.current?.scrollBy({ x: 0, y: 10 }, "step");
      scheduleNavigationSync();
    } else if (e.name === "u" || e.name === "pageup") {
      contentScrollRef.current?.scrollBy({ x: 0, y: -10 }, "step");
      scheduleNavigationSync();
    }
  });

  function toggleExpanded(id: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  return (
    <box flexDirection="column" shouldFill={true}>
      <box
        height={1}
        flexDirection="row"
        backgroundColor="#181825"
        border={["bottom"]}
        borderColor="#313244"
      >
        {MENU_BAR.map((menu) => {
          const isOpen = openMenu === menu.id;
          return (
            <box
              key={menu.id}
              height={1}
              paddingLeft={1}
              paddingRight={1}
              backgroundColor={isOpen ? "#45475a" : "#181825"}
              onMouseDown={() => openMenuById(menu.id)}
            >
              <text fg={isOpen ? "#f5e0dc" : "#bac2de"}>{menu.label}</text>
            </box>
          );
        })}
        <box flexGrow={1} flexDirection="row" justifyContent="flex-end" paddingRight={1}>
          <text fg="#7f849c" truncate wrapMode="none">{`${result.topic}${result.section ? `(${result.section})` : ""}`}</text>
        </box>
      </box>
      <box
        flexDirection="row"
        shouldFill={true}
        flexGrow={1}
        onMouseDown={startNavigationResize}
        onMouseDrag={resizeNavigation}
        onMouseUp={finishNavigationResize}
      >
        {isNavigationVisible && (
          <box
            width={navigationWidth}
            flexDirection="column"
            flexShrink={0}
            backgroundColor="#11111b"
          >
            <box
              flexDirection="column"
              paddingLeft={1}
              paddingRight={1}
              paddingTop={1}
              paddingBottom={1}
              border={["bottom"]}
              borderColor="#313244"
            >
              <text height={1} fg="#cdd6f4" truncate wrapMode="none" selectable={false}>
                {`MANUAL · ${result.topic}`}
              </text>
              <text height={1} fg="#7f849c" selectable={false}>
                {`${result.sections.length} top-level · ${visibleNodes.length} manual${result.tldr ? " · TLDR" : ""}`}
              </text>
            </box>
            <box height={1} paddingLeft={1} paddingRight={1}>
              <text fg="#6c7086" selectable={false}>SECTIONS</text>
            </box>
            <scrollbox
              ref={navScrollRef}
              flexGrow={1}
              scrollY
              focusable={false}
              horizontalScrollbarOptions={{ visible: false }}
              verticalScrollbarOptions={{
                trackOptions: {
                  foregroundColor: "#45475a",
                  backgroundColor: "#11111b",
                },
              }}
            >
              {result.tldr && (
                <box
                  id={navId(TLDR_NAV_ID)}
                  width="100%"
                  height={1}
                  flexShrink={0}
                  paddingLeft={1}
                  backgroundColor={selectedId === TLDR_NAV_ID ? "#49405f" : "#1d1a2b"}
                  onMouseDown={() => selectSection(TLDR_NAV_ID)}
                >
                  <text truncate wrapMode="none" selectable={false}>
                    <span fg={selectedId === TLDR_NAV_ID ? "#f5e0dc" : "#cba6f7"}>
                      {selectedId === TLDR_NAV_ID ? "› ◆ " : "  ◆ "}
                    </span>
                    <span fg="#cba6f7">
                      <b>TLDR QUICK REFERENCE</b>
                    </span>
                  </text>
                </box>
              )}
              {visibleNodes.map((flatNode) => {
                const { node, hasChildren } = flatNode;
                const isSelected = node.id === selectedId;
                const titleColor = isSelected
                  ? "#f5e0dc"
                  : flatNode.depth === 0
                    ? "#cdd6f4"
                    : flatNode.depth === 1
                      ? "#89b4fa"
                      : "#a6adc8";
                const disclosure = hasChildren
                  ? expanded.has(node.id)
                    ? "▾ "
                    : "▸ "
                  : "· ";
                const labelPrefix = `${isSelected ? "› " : "  "}${treePrefix(flatNode)}${disclosure}`;
                const selectedTitleLines = isSelected
                  ? wrapNavigationTitle(
                      node.title,
                      navigationWidth - 1 - terminalColumnWidth(labelPrefix),
                    )
                  : [];
                return (
                  <box
                    key={navId(node.id)}
                    id={navId(node.id)}
                    ref={attachSectionClick(node.id, hasChildren)}
                    width="100%"
                    height={isSelected ? "auto" : 1}
                    flexDirection={isSelected ? "column" : "row"}
                    flexShrink={0}
                    paddingLeft={1}
                    backgroundColor={isSelected ? "#313244" : "#11111b"}
                  >
                    {isSelected ? (
                      selectedTitleLines.map((line, index) => {
                        const prefix = index === 0
                          ? labelPrefix
                          : `  ${treeContinuationPrefix(flatNode)}`;
                        return (
                          <box
                            key={`${node.id}-line-${index}`}
                            width="100%"
                            height={1}
                            flexDirection="row"
                            backgroundColor="#313244"
                          >
                            <text
                              width={terminalColumnWidth(prefix)}
                              fg={index === 0 ? "#fab387" : "#f5c2e7"}
                              wrapMode="none"
                              selectable={false}
                            >
                              {prefix}
                            </text>
                            <text fg={titleColor} wrapMode="none" selectable={false}>
                              {line}
                            </text>
                          </box>
                        );
                      })
                    ) : (
                      <text truncate wrapMode="none" selectable={false}>
                        <span fg="#6c7086">{labelPrefix}</span>
                        <span fg={titleColor}>{node.title}</span>
                      </text>
                    )}
                  </box>
                );
              })}
            </scrollbox>
          </box>
        )}
        <box
          flexGrow={1}
          flexDirection="column"
          paddingLeft={1}
          paddingTop={1}
          paddingBottom={1}
          paddingRight={1}
        >
          <scrollbox
            ref={contentScrollRef}
            flexGrow={1}
            scrollY
            focusable={false}
            onMouseScroll={scheduleNavigationSync}
            onMouseDrag={scheduleNavigationSync}
            onMouseUp={scheduleNavigationSync}
          >
            <box flexDirection="column" gap={1}>
              {result.tldr && <TldrQuickReference page={result.tldr} searchQuery={searchQuery} />}
              {result.tldr && result.sections.length > 0 && (
                <box height={1} border={["top"]} borderColor="#45475a" paddingLeft={1}>
                  <text fg="#6c7086">MANUAL</text>
                </box>
              )}
              {result.tldr && result.sections.length === 0 && (
                <box
                  backgroundColor="#1e1e2e"
                  border={["top"]}
                  borderColor="#45475a"
                  paddingLeft={1}
                  paddingRight={1}
                >
                  <text fg="#f9e2af" wrapMode="word">
                    No local man page was found; showing the cached tldr quick reference.
                  </text>
                </box>
              )}
              {result.sections.map((node) => (
                <SectionContent
                  key={node.id}
                  node={node}
                  searchQuery={searchQuery}
                  activeSearchSectionId={searchMatches[searchIndex]?.sectionId}
                  activeBlockIndex={
                    searchMatches[searchIndex]?.blockIndex
                  }
                />
              ))}
              <box height={terminalHeight} flexShrink={0} />
            </box>
          </scrollbox>
        </box>
      </box>

      {isSearchOpen ? (
        <box
          height={1}
          flexDirection="row"
          backgroundColor="#181825"
          paddingLeft={1}
          paddingRight={1}
        >
          <text fg="#f9e2af">Find:</text>
          <box width={1} />
          <input
            ref={searchInputRef}
            flexGrow={1}
            value={searchDraft}
            focused
            placeholder="Search this page"
            placeholderColor="#6c7086"
            backgroundColor="#313244"
            focusedBackgroundColor="#313244"
            textColor="#cdd6f4"
            focusedTextColor="#cdd6f4"
            onInput={setSearchDraft}
            onSubmit={submitSearch}
          />
          <box width={1} />
          <text fg="#7f849c">
            {searchDraft !== searchQuery
              ? "Enter search · Esc cancel"
              : searchMatches.length > 0
              ? `${searchIndex + 1}/${searchMatches.length}  Enter next · Esc close`
              : "0 matches  Esc close"}
          </text>
        </box>
      ) : (
        <box
          height={1}
          flexDirection="row"
          backgroundColor="#1e1e2e"
          paddingLeft={1}
          paddingRight={1}
        >
          <text fg="#a6adc8" truncate wrapMode="none">
            {navigationItems.length > 0
              ? `${navigationItems.findIndex((item) => item.id === selectedId) + 1}/${navigationItems.length} · ${selectedNavigationItem?.title ?? ""}`
              : "No content"}
          </text>
          <box flexGrow={1} />
          <text fg="#6c7086" truncate wrapMode="none">
            {searchQuery && searchMatches.length > 0
              ? `Find “${searchQuery}” · ${searchMatches.length} matches`
              : `${visibleNodes.length} visible manual sections${result.tldr ? " · TLDR cached" : ""}`}
          </text>
        </box>
      )}

      {openMenu && (
        <box
          position="absolute"
          left={MENU_BAR.find((menu) => menu.id === openMenu)!.left}
          top={1}
          width={30}
          flexDirection="column"
          zIndex={10}
          backgroundColor="#1e1e2e"
          border={["left", "right", "bottom"]}
          borderColor="#585b70"
        >
          {activeMenuEntries.map((entry, index) => {
            const isActive = index === menuCursor;
            return (
              <box
                key={`${openMenu}-${entry.label}`}
                height={1}
                flexDirection="row"
                paddingLeft={1}
                paddingRight={1}
                backgroundColor={isActive ? "#45475a" : "#1e1e2e"}
                onMouseDown={(event) => {
                  event.stopPropagation();
                  activateMenuEntry(entry);
                }}
              >
                <text fg={isActive ? "#f5e0dc" : "#cdd6f4"}>
                  {entry.checked ? "✓ " : "  "}
                  {entry.label}
                </text>
                <box flexGrow={1} />
                <text fg={isActive ? "#bac2de" : "#7f849c"}>{entry.shortcut}</text>
              </box>
            );
          })}
        </box>
      )}

      {isHelpOpen && (
        <box
          position="absolute"
          left={Math.max(2, Math.floor((terminalWidth - 54) / 2))}
          top={3}
          width={Math.min(54, terminalWidth - 4)}
          flexDirection="column"
          zIndex={20}
          backgroundColor="#1e1e2e"
          border={["top", "right", "bottom", "left"]}
          borderColor="#89b4fa"
          padding={1}
        >
          <text fg="#89b4fa"><b>Keyboard Shortcuts</b></text>
          <text fg="#cdd6f4">↑/↓ or j/k  select section</text>
          <text fg="#cdd6f4">←/→ or h/l  move through the section tree</text>
          <text fg="#cdd6f4">Enter        fold or unfold selected section</text>
          <text fg="#cdd6f4">Ctrl+F or /   find in current page</text>
          <text fg="#cdd6f4">n / N        next / previous search match</text>
          <text fg="#cdd6f4">F10          open menu bar</text>
          <text fg="#cdd6f4">q            quit</text>
          <box height={1} />
          <text fg="#7f849c">Esc or ? closes this window</text>
        </box>
      )}
    </box>
  );
}

export async function runTui(result: QueryResult): Promise<void> {
  const renderer = await createCliRenderer({
    exitOnCtrlC: true,
    useMouse: true,
  });

  const quit = () => renderer.destroy();
  createRoot(renderer).render(<App result={result} onQuit={quit} />);
}
