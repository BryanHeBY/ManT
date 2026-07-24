/**
 * @file Synchronizes sidebar selection after content scrolling becomes idle.
 *
 * Deferring the document scan prevents wheel and page-scroll input from paying
 * for sidebar state updates on every movement while preserving an accurate
 * selected section once the viewport settles.
 */

import type { ScrollBoxRenderable } from "@opentui/core";
import { useCallback, useEffect, useMemo, useRef, type Dispatch, type SetStateAction } from "react";
import type { MantSection } from "../native";
import { DOCUMENT_ROOT_ID, TLDR_NAV_ID, contentId } from "./ids";
import { findNodePath, sectionIdsInDocumentOrder } from "./navigation-tree";

const NAVIGATION_SYNC_DELAY_MS = 180;

export interface DeferredNavigationSyncOptions {
  sections: MantSection[];
  hasTldr: boolean;
  hasRoot: boolean;
  contentScrollRef: { current: ScrollBoxRenderable | null };
  setSelectedId: Dispatch<SetStateAction<string>>;
  setExpanded: Dispatch<SetStateAction<Set<string>>>;
}

/** Returns an event handler that schedules one post-scroll navigation update. */
export function useDeferredNavigationSync({
  sections,
  hasTldr,
  hasRoot,
  contentScrollRef,
  setSelectedId,
  setExpanded,
}: DeferredNavigationSyncOptions): () => void {
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const sectionIds = useMemo(
    () => [
      ...(hasTldr ? [TLDR_NAV_ID] : []),
      ...(hasRoot ? [DOCUMENT_ROOT_ID] : []),
      ...sectionIdsInDocumentOrder(sections),
    ],
    [hasRoot, hasTldr, sections],
  );

  const sync = useCallback(() => {
    const scrollbox = contentScrollRef.current;
    if (!scrollbox || sectionIds.length === 0) return;

    // The active item is the final heading at or above the first visible
    // content row, rather than the nearest heading in either direction.
    let activeId = sectionIds[0]!;
    for (const id of sectionIds) {
      const heading = scrollbox.content.findDescendantById(contentId(id));
      if (!heading) continue;
      if (heading.y > scrollbox.viewport.y) break;
      activeId = id;
    }

    setSelectedId((current) => (current === activeId ? current : activeId));

    // Scrolling to a folded child must reveal its complete ancestry.
    if (activeId === TLDR_NAV_ID || activeId === DOCUMENT_ROOT_ID) return;
    const path = findNodePath(sections, activeId);
    if (!path) return;
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
  }, [contentScrollRef, sectionIds, sections, setExpanded, setSelectedId]);

  const schedule = useCallback(() => {
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      sync();
    }, NAVIGATION_SYNC_DELAY_MS);
  }, [sync]);

  useEffect(
    () => () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    },
    [],
  );

  return schedule;
}
