/**
 * @file Coordinates manual-view state, keyboard input, scrolling, and layout.
 *
 * Stateless visual regions live beside this controller so interaction policy is
 * easy to trace here without making the main TUI module a rendering monolith.
 */

import {
  type BaseRenderable,
  createCliRenderer,
  type InputRenderable,
  type ScrollBoxRenderable,
  TextRenderable,
} from "@opentui/core";
import { createRoot, useKeyboard, useTerminalDimensions } from "@opentui/react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { MantQueryBundle, MantSection } from "../native";
import { TLDR_NAV_ID, contentAnchorId, contentId, navId } from "./ids";
import {
  KeyboardHelpDialog,
  MENU_BAR,
  MenuBar,
  MenuPopup,
  type MenuEntry,
  type MenuId,
} from "./menu-bar";
import { SectionContent, TldrQuickReference } from "./manual-content";
import { ManualSidebar } from "./manual-sidebar";
import {
  buildNavigationNodes,
  clamp,
  collectBranchIds,
  findNodeById,
  findNodePath,
  findParentById,
  flattenVisibleNodes,
} from "./navigation-tree";
import {
  buildPageSearchIndex,
  queryPageSearchIndex,
  type SearchMatch,
} from "./search";
import {
  applyActiveSearchHighlight,
  applySearchMatchHighlights,
  clearActiveSearchHighlight,
  clearSearchHighlights,
  toTextBufferRange,
} from "./search-highlight";
import { ManualStatusBar, SearchBar } from "./status-bar";
import { useDeferredNavigationSync } from "./use-deferred-navigation-sync";
import { useSidebarResize } from "./use-sidebar-resize";

interface AppProps {
  result: MantQueryBundle;
  onQuit: () => void;
}

interface AppliedSearch {
  query: string;
  matches: SearchMatch[];
  activeIndex: number;
}

const EMPTY_SECTIONS: MantSection[] = [];

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

export function App({ result, onQuit }: AppProps) {
  const sections = result.manual?.sections ?? EMPTY_SECTIONS;
  // ── View state and render references ──────────────────────

  const [selectedId, setSelectedId] = useState<string>(
    result.tldr ? TLDR_NAV_ID : sections[0]?.id ?? ""
  );
  const [expanded, setExpanded] = useState<Set<string>>(() => {
    const initial = new Set<string>();
    for (const section of sections) {
      initial.add(section.id);
    }
    return initial;
  });
  const [isNavigationVisible, setIsNavigationVisible] = useState(true);
  const [openMenu, setOpenMenu] = useState<MenuId | null>(null);
  const [menuCursor, setMenuCursor] = useState(0);
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  // Keep the editing buffer separate from the indexed query so typing remains
  // free of search work and a new result set appears only after confirmation.
  const [searchDraft, setSearchDraft] = useState("");
  const [search, setSearch] = useState<AppliedSearch>({
    query: "",
    matches: [],
    activeIndex: 0,
  });
  const [isHelpOpen, setIsHelpOpen] = useState(false);
  const contentScrollRef = useRef<ScrollBoxRenderable | null>(null);
  const navScrollRef = useRef<ScrollBoxRenderable | null>(null);
  const searchInputRef = useRef<InputRenderable | null>(null);
  const highlightedTextsRef = useRef<Set<TextRenderable>>(new Set());
  const searchTargetTextsRef = useRef<Map<string, TextRenderable>>(new Map());
  const activeHighlightedTextRef = useRef<TextRenderable | null>(null);
  const { width: terminalWidth, height: terminalHeight } = useTerminalDimensions();
  const {
    navigationWidth,
    resetNavigationWidth,
    startResize,
    resize,
    finishResize,
  } = useSidebarResize({
    isVisible: isNavigationVisible,
    terminalWidth,
  });

  // ── Derived document model ─────────────────────────────────

  const navigationRoots = useMemo(
    () => buildNavigationNodes(sections),
    [sections]
  );
  const visibleNodes = useMemo(
    () => flattenVisibleNodes(navigationRoots, expanded),
    [navigationRoots, expanded]
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
  const visibleSectionCount = useMemo(
    () => visibleNodes.filter(({ node }) => node.kind === "section").length,
    [visibleNodes]
  );
  const pageSearchIndex = useMemo(
    () => buildPageSearchIndex(sections, result.tldr),
    [sections, result.tldr]
  );
  const branchIds = useMemo(
    () => collectBranchIds(navigationRoots),
    [navigationRoots]
  );
  const searchQuery = search.query;
  const searchMatches = search.matches;
  const searchIndex = search.activeIndex;
  // ── Section selection and scrolling ────────────────────────

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

  const clearAllSearchDecorations = () => {
    clearSearchHighlights(highlightedTextsRef.current);
    highlightedTextsRef.current = new Set();
    searchTargetTextsRef.current = new Map();
    activeHighlightedTextRef.current = null;
  };

  /** Add the low-priority layer once per target TextBuffer for a new query. */
  const decorateSearchMatches = (matches: readonly SearchMatch[]) => {
    clearAllSearchDecorations();
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
  const scrollToSearchMatch = (match: SearchMatch) => {
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

  const scrollToNode = (id: string) => scrollToContent(contentId(id));

  const scheduleNavigationSync = useDeferredNavigationSync({
    sections,
    hasTldr: Boolean(result.tldr),
    contentScrollRef,
    setSelectedId,
    setExpanded,
  });

  const selectSection = (id: string) => {
    setSelectedId(id);
    scrollToNode(id);
    navScrollRef.current?.scrollChildIntoView(navId(id));
  };

  const selectNavigationNode = (id: string) => {
    if (id === TLDR_NAV_ID) {
      selectSection(id);
      return;
    }
    const node = findNodeById(navigationRoots, id);
    if (!node) return;
    setSelectedId(id);
    scrollToContent(
      node.kind === "option"
        ? contentAnchorId(node.targetId)
        : contentId(node.targetId),
    );
    navScrollRef.current?.scrollChildIntoView(navId(id));
  };

  /** Follow a typed same-page reference without creating browser-like history. */
  const navigateWithinPage = (target: string) => {
    const path = findNodePath(sections, target);
    if (!path) {
      // Section references normally resolve to Section IDs. Keeping anchor
      // lookup here also makes explicit `.Tg` destinations usable by future
      // native reference forms without changing the view boundary.
      scrollToContent(contentAnchorId(target));
      return;
    }

    setExpanded((current) => {
      const next = new Set(current);
      for (const id of path.slice(0, -1)) next.add(id);
      return next;
    });
    setSelectedId(target);
    scrollToNode(target);
  };

  const selectSearchMatch = (index: number) => {
    if (searchMatches.length === 0) return;
    const nextIndex = ((index % searchMatches.length) + searchMatches.length) % searchMatches.length;
    const match = searchMatches[nextIndex]!;
    setSearch((current) => ({ ...current, activeIndex: nextIndex }));
    setSelectedId(match.sectionId);
    navScrollRef.current?.scrollChildIntoView(navId(match.sectionId));
    scrollToSearchMatch(match);
  };

  const selectRelativeSection = (offset: number) => {
    const currentIndex = navigationItems.findIndex((item) => item.id === selectedId);
    const nextIndex = clamp(
      currentIndex + offset,
      0,
      Math.max(navigationItems.length - 1, 0)
    );
    const next = navigationItems[nextIndex];
    if (next) selectNavigationNode(next.id);
  };

  // ── Search actions ─────────────────────────────────────────

  const openSearch = () => {
    setOpenMenu(null);
    setIsHelpOpen(false);
    setSearchDraft(searchQuery);
    setIsSearchOpen(true);
  };

  /** Leave search mode and remove every visual/result state owned by it. */
  const closeSearch = () => {
    clearAllSearchDecorations();
    setSearchDraft("");
    setSearch({ query: "", matches: [], activeIndex: 0 });
    setIsSearchOpen(false);
  };

  /** Apply the input's submitted value instead of a possibly stale render snapshot. */
  const submitSearch = (submittedDraft: string) => {
    if (submittedDraft !== searchDraft) setSearchDraft(submittedDraft);
    if (submittedDraft === searchQuery) {
      selectSearchMatch(searchIndex + 1);
      return;
    }

    const matches = queryPageSearchIndex(pageSearchIndex, submittedDraft);
    setSearch({ query: submittedDraft, matches, activeIndex: 0 });
    decorateSearchMatches(matches);
    if (matches[0]) {
      setSelectedId(matches[0].sectionId);
      navScrollRef.current?.scrollChildIntoView(navId(matches[0].sectionId));
      scrollToSearchMatch(matches[0]);
    }
  };

  // ── Tree expansion actions ─────────────────────────────────

  const expandAll = () => setExpanded(new Set(branchIds));
  const collapseAll = () => setExpanded(new Set());

  const navigateToParent = () => {
    const parent = findParentById(navigationRoots, selectedId);
    if (parent) selectNavigationNode(parent.id);
  };

  const navigateToFirstChild = () => {
    const node = findNodeById(navigationRoots, selectedId);
    if (node?.children[0]) selectNavigationNode(node.children[0].id);
  };

  const expandCurrentSection = () => {
    const node = findNodeById(navigationRoots, selectedId);
    if (!node?.children.length) return;
    setExpanded((current) => new Set(current).add(node.id));
  };

  const collapseCurrentSection = () => {
    const node = findNodeById(navigationRoots, selectedId);
    if (!node?.children.length) return;
    setExpanded((current) => {
      const next = new Set(current);
      next.delete(node.id);
      return next;
    });
  };

  // ── Menu actions ───────────────────────────────────────────

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
        action: resetNavigationWidth,
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
          closeSearch();
          setIsHelpOpen(true);
        },
      },
    ],
  };

  const activeMenuEntries = openMenu ? menuEntries[openMenu] : [];

  const openMenuById = (menu: MenuId) => {
    closeSearch();
    setIsHelpOpen(false);
    setOpenMenu((current) => (current === menu ? null : menu));
    setMenuCursor(0);
  };

  const activateMenuEntry = (entry: MenuEntry) => {
    entry.action();
    setOpenMenu(null);
    setMenuCursor(0);
  };

  // ── Layout synchronization effects ─────────────────────────

  useEffect(() => {
    if (isSearchOpen) searchInputRef.current?.focus();
  }, [isSearchOpen]);

  useEffect(() => {
    // A selected long title may grow from one row into several after React
    // commits.  Re-run the visibility adjustment after that layout change.
    if (selectedId) navScrollRef.current?.scrollChildIntoView(navId(selectedId));
  }, [selectedId, visibleNodes]);

  // ── Sidebar mouse interactions ─────────────────────────────

  const activateSidebarNode = (id: string, hasChildren: boolean) => {
    if (hasChildren && selectedId === id) {
      toggleExpanded(id);
      return;
    }

    selectNavigationNode(id);
    if (hasChildren) {
      setExpanded((current) => new Set(current).add(id));
    }
  };

  // ── Keyboard routing ───────────────────────────────────────

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
      const node = findNodeById(navigationRoots, selectedId);
      if (node && expanded.has(node.id) && node.children.length > 0) {
        collapseCurrentSection();
      } else {
        navigateToParent();
      }
    } else if (e.name === "l" || e.name === "right") {
      const node = findNodeById(navigationRoots, selectedId);
      if (node && node.children.length > 0) {
        if (!expanded.has(node.id)) {
          expandCurrentSection();
        } else {
          navigateToFirstChild();
        }
      }
    } else if (e.name === "return" || e.name === "enter" || e.name === "space") {
      const node = findNodeById(navigationRoots, selectedId);
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

  // ── TUI composition ────────────────────────────────────────

  return (
    <box flexDirection="column" shouldFill={true}>
      <MenuBar
        topic={result.topic}
        section={result.section}
        openMenu={openMenu}
        onToggleMenu={openMenuById}
      />
      <box
        flexDirection="row"
        shouldFill={true}
        flexGrow={1}
        onMouseDown={startResize}
        onMouseDrag={resize}
        onMouseUp={finishResize}
      >
        {isNavigationVisible && (
          <ManualSidebar
            result={result}
            visibleNodes={visibleNodes}
            selectedId={selectedId}
            expanded={expanded}
            width={navigationWidth}
            scrollRef={navScrollRef}
            onActivateNode={activateSidebarNode}
            onActivateTldr={() => selectSection(TLDR_NAV_ID)}
          />
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
              {result.tldr && <TldrQuickReference page={result.tldr} />}
              {result.tldr && sections.length > 0 && (
                <box height={1} border={["top"]} borderColor="#45475a" paddingLeft={1}>
                  <text fg="#6c7086">MANUAL</text>
                </box>
              )}
              {result.tldr && sections.length === 0 && (
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
              {sections.map((node) => (
                <SectionContent
                  key={node.id}
                  node={node}
                  onNavigateInternal={navigateWithinPage}
                />
              ))}
              <box height={terminalHeight} flexShrink={0} />
            </box>
          </scrollbox>
        </box>
      </box>

      {isSearchOpen ? (
        <SearchBar
          inputRef={searchInputRef}
          draft={searchDraft}
          appliedQuery={searchQuery}
          matchCount={searchMatches.length}
          matchIndex={searchIndex}
          onDraftChange={setSearchDraft}
          onSubmit={submitSearch}
        />
      ) : (
        <ManualStatusBar
          navigationItems={navigationItems}
          selectedId={selectedId}
          visibleSectionCount={visibleSectionCount}
          hasTldr={Boolean(result.tldr)}
          searchQuery={searchQuery}
          searchMatchCount={searchMatches.length}
        />
      )}

      {openMenu && (
        <MenuPopup
          menu={openMenu}
          entries={activeMenuEntries}
          cursor={menuCursor}
          onActivate={activateMenuEntry}
        />
      )}

      {isHelpOpen && <KeyboardHelpDialog terminalWidth={terminalWidth} />}
    </box>
  );
}

export async function runTui(result: MantQueryBundle): Promise<void> {
  let resolveDestroyed: () => void = () => {};
  const destroyed = new Promise<void>((resolve) => {
    resolveDestroyed = resolve;
  });
  const renderer = await createCliRenderer({
    exitOnCtrlC: true,
    useMouse: true,
    onDestroy: resolveDestroyed,
  });

  try {
    const quit = () => renderer.destroy();
    createRoot(renderer).render(<App result={result} onQuit={quit} />);
  } catch (error) {
    // A synchronous React/OpenTUI setup failure must not leave the terminal in
    // raw mode. The CLI boundary will turn the original error into a concise
    // user-facing diagnostic after the renderer has restored the terminal.
    renderer.destroy();
    throw error;
  }

  // Keep the CLI execution boundary alive for the complete interactive
  // session instead of considering startup alone to be a successful run.
  await destroyed;
}
