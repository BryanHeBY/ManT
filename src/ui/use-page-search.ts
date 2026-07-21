/**
 * @file Owns confirmed in-page search state and OpenTUI text decoration.
 *
 * The page index is built once per immutable native query. This hook keeps
 * search execution, highlights, and exact wrapped-row scrolling together so
 * the application shell only decides when search mode should be entered.
 */

import { type BaseRenderable, type InputRenderable, type ScrollBoxRenderable, TextRenderable } from "@opentui/core";
import { useEffect, useMemo, useRef, useState } from "react";
import type { MantSection, TldrDocument } from "../native";
import { buildPageSearchIndex, queryPageSearchIndex, type SearchMatch } from "./search";
import {
  applyActiveSearchHighlight,
  applySearchMatchHighlights,
  clearActiveSearchHighlight,
  clearSearchHighlights,
  toTextBufferRange,
} from "./search-highlight";

interface AppliedSearch {
  query: string;
  matches: SearchMatch[];
  activeIndex: number;
}

interface ScrollReference {
  current: ScrollBoxRenderable | null;
}

export interface PageSearchOptions {
  sections: MantSection[];
  tldr: TldrDocument | undefined;
  contentScrollRef: ScrollReference;
  onSelectSection: (sectionId: string) => void;
}

export interface PageSearchController {
  inputRef: { current: InputRenderable | null };
  isOpen: boolean;
  isEditing: boolean;
  query: string;
  matches: readonly SearchMatch[];
  activeIndex: number;
  open(): void;
  close(): void;
  markEditing(): void;
  submit(draft: string): void;
  select(index: number): void;
}

/** Resolve many stable IDs in one tree walk instead of one walk per match. */
function collectSearchTargets(
  root: BaseRenderable,
  targetIds: ReadonlySet<string>,
): Map<string, BaseRenderable> {
  const targets = new Map<string, BaseRenderable>();
  const pending = [root];
  while (pending.length > 0 && targets.size < targetIds.size) {
    const current = pending.pop()!;
    if (targetIds.has(current.id)) targets.set(current.id, current);
    pending.push(...current.getChildren());
  }
  return targets;
}

/** Resolve a source offset through OpenTUI's measured word-wrapping metadata. */
function matchVisualRow(renderable: TextRenderable, match: SearchMatch): number {
  const prefix = match.text.slice(0, match.range.start);
  const newlineCount = prefix.split("\n").length - 1;
  // OpenTUI reports each visual row's offset in flattened display columns;
  // explicit newline separators occupy one offset but no terminal width.
  const displayOffset = Bun.stringWidth(prefix) + newlineCount;
  const { lineStartCols } = renderable.lineInfo;
  let visualRow = 0;
  for (let index = 0; index < lineStartCols.length; index++) {
    if ((lineStartCols[index] ?? 0) > displayOffset) break;
    visualRow = index;
  }
  return visualRow;
}

/** Build, decorate, and navigate one immutable manual-page search index. */
export function usePageSearch({
  sections,
  tldr,
  contentScrollRef,
  onSelectSection,
}: PageSearchOptions): PageSearchController {
  const [isOpen, setIsOpen] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [search, setSearch] = useState<AppliedSearch>({
    query: "",
    matches: [],
    activeIndex: 0,
  });
  const inputRef = useRef<InputRenderable | null>(null);
  const highlightedTextsRef = useRef<Set<TextRenderable>>(new Set());
  const searchTargetTextsRef = useRef<Map<string, TextRenderable>>(new Map());
  const activeHighlightedTextRef = useRef<TextRenderable | null>(null);
  const pageSearchIndex = useMemo(
    () => buildPageSearchIndex(sections, tldr),
    [sections, tldr],
  );

  const clearAllDecorations = () => {
    clearSearchHighlights(highlightedTextsRef.current);
    highlightedTextsRef.current = new Set();
    searchTargetTextsRef.current = new Map();
    activeHighlightedTextRef.current = null;
  };

  /** Add the low-priority layer once per target TextBuffer for a new query. */
  const decorateMatches = (matches: readonly SearchMatch[]) => {
    clearAllDecorations();
    const content = contentScrollRef.current?.content;
    if (!content || matches.length === 0) return;

    const grouped = new Map<string, SearchMatch[]>();
    for (const match of matches) {
      const group = grouped.get(match.targetId);
      if (group) group.push(match);
      else grouped.set(match.targetId, [match]);
    }
    const targets = collectSearchTargets(content, new Set(grouped.keys()));
    for (const [targetId, group] of grouped) {
      const target = targets.get(targetId);
      if (!target) continue;
      const text = applySearchMatchHighlights(
        target,
        group.map((match) => toTextBufferRange(match.text, match.range)),
      );
      if (!text) continue;
      highlightedTextsRef.current.add(text);
      searchTargetTextsRef.current.set(targetId, text);
    }
  };

  /** Move only the high-priority active layer, retaining every other match. */
  const scrollToMatch = (match: SearchMatch) => {
    const scrollbox = contentScrollRef.current;
    if (!scrollbox) return;
    clearActiveSearchHighlight(activeHighlightedTextRef.current);
    let text = searchTargetTextsRef.current.get(match.targetId);
    if (!text) {
      const target = scrollbox.content.findDescendantById(match.targetId);
      if (!target) return;
      text = applySearchMatchHighlights(target, [
        toTextBufferRange(match.text, match.range),
      ]);
      if (!text) return;
      highlightedTextsRef.current.add(text);
      searchTargetTextsRef.current.set(match.targetId, text);
    }
    applyActiveSearchHighlight(text, toTextBufferRange(match.text, match.range));
    activeHighlightedTextRef.current = text;
    const targetY = text.y + matchVisualRow(text, match);
    scrollbox.scrollTo({
      x: scrollbox.scrollLeft,
      y: Math.max(0, scrollbox.scrollTop + targetY - scrollbox.viewport.y),
    });
  };

  const select = (index: number) => {
    if (search.matches.length === 0) return;
    const nextIndex = ((index % search.matches.length) + search.matches.length) % search.matches.length;
    const match = search.matches[nextIndex]!;
    setSearch((current) => ({ ...current, activeIndex: nextIndex }));
    onSelectSection(match.sectionId);
    scrollToMatch(match);
  };

  const open = () => {
    setIsOpen(true);
    setIsEditing(false);
  };

  const close = () => {
    clearAllDecorations();
    setSearch({ query: "", matches: [], activeIndex: 0 });
    setIsOpen(false);
    setIsEditing(false);
  };

  const submit = (draft: string) => {
    setIsEditing(false);
    if (draft === search.query) {
      select(search.activeIndex + 1);
      return;
    }

    const matches = queryPageSearchIndex(pageSearchIndex, draft);
    setSearch({ query: draft, matches, activeIndex: 0 });
    decorateMatches(matches);
    if (matches[0]) {
      onSelectSection(matches[0].sectionId);
      scrollToMatch(matches[0]);
    }
  };

  useEffect(() => {
    if (isOpen) inputRef.current?.focus();
  }, [isOpen]);

  return {
    inputRef,
    isOpen,
    isEditing,
    query: search.query,
    matches: search.matches,
    activeIndex: search.activeIndex,
    open,
    close,
    markEditing: () => setIsEditing(true),
    submit,
    select,
  };
}
