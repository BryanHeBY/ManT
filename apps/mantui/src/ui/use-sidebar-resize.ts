/**
 * @file Owns sidebar width limits and drag-resize pointer handling.
 *
 * The hook keeps the resize hit area on the colour boundary while leaving the
 * sidebar component itself purely presentational.
 */

import type { MouseEvent as TuiMouseEvent } from "@opentui/core";
import { useEffect, useRef, useState } from "react";
import { clamp } from "./navigation-tree";

export const DEFAULT_NAVIGATION_WIDTH = 32;

const MIN_NAVIGATION_WIDTH = 24;
const MIN_CONTENT_WIDTH = 32;

export interface SidebarResizeOptions {
  isVisible: boolean;
  terminalWidth: number;
}

interface ResizeStart {
  startX: number;
  startWidth: number;
}

/** Returns width state and mouse handlers for the navigation/content boundary. */
export function useSidebarResize({ isVisible, terminalWidth }: SidebarResizeOptions) {
  const [navigationWidth, setNavigationWidth] = useState(DEFAULT_NAVIGATION_WIDTH);
  const resizeRef = useRef<ResizeStart | null>(null);
  const maxNavigationWidth = Math.max(
    MIN_NAVIGATION_WIDTH,
    terminalWidth - MIN_CONTENT_WIDTH - 1,
  );

  useEffect(() => {
    setNavigationWidth((current) =>
      clamp(current, MIN_NAVIGATION_WIDTH, maxNavigationWidth),
    );
  }, [maxNavigationWidth]);

  const startResize = (event: TuiMouseEvent) => {
    // One terminal column on either side of the boundary balances discoverable
    // dragging with avoiding accidental drags from normal sidebar text.
    if (!isVisible || Math.abs(event.x - navigationWidth) > 1) return;

    event.stopPropagation();
    event.preventDefault();
    resizeRef.current = { startX: event.x, startWidth: navigationWidth };
  };

  const resize = (event: TuiMouseEvent) => {
    const start = resizeRef.current;
    if (!start) return;

    event.stopPropagation();
    event.preventDefault();
    setNavigationWidth(
      clamp(start.startWidth + event.x - start.startX, MIN_NAVIGATION_WIDTH, maxNavigationWidth),
    );
  };

  const finishResize = (event: TuiMouseEvent) => {
    if (!resizeRef.current) return;

    event.stopPropagation();
    event.preventDefault();
    resizeRef.current = null;
  };

  return {
    navigationWidth,
    resetNavigationWidth: () => setNavigationWidth(DEFAULT_NAVIGATION_WIDTH),
    startResize,
    resize,
    finishResize,
  };
}
