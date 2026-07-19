/**
 * @file Applies search decoration directly to OpenTUI text buffers.
 *
 * Search highlighting changes colors only. Using the buffer highlight layer
 * avoids replacing React text nodes and triggering a full-document layout.
 */

import { type BaseRenderable, TextRenderable } from "@opentui/core";
import type { SearchRange } from "./search";

export const SEARCH_HIGHLIGHT_BACKGROUND = "#f9e2af";
export const SEARCH_HIGHLIGHT_FOREGROUND = "#1e1e2e";

const SEARCH_HIGHLIGHT_NAME = "mant.search.match";
const SEARCH_HIGHLIGHT_REF = 0x4d414e54;

interface TextBufferHighlightApi {
  addHighlightByCharRange(highlight: {
    start: number;
    end: number;
    styleId: number;
    priority: number;
    hlRef: number;
  }): void;
  removeHighlightsByRef(reference: number): void;
}

interface SyntaxStyleHighlightApi {
  resolveStyleId(name: string): number | null;
  registerStyle(name: string, style: { fg: string; bg: string }): number;
}

interface TextHighlightInternals {
  textBuffer: TextBufferHighlightApi;
  _textBufferSyntaxStyle: SyntaxStyleHighlightApi;
}

function highlightInternals(renderable: TextRenderable): TextHighlightInternals {
  // OpenTUI exposes these as protected extension points rather than public
  // TextRenderable methods. The framework version is pinned and this adapter
  // is the sole place coupled to that boundary.
  return renderable as unknown as TextHighlightInternals;
}

/** Find the TextBuffer owned by a stable search target container. */
export function firstTextRenderable(renderable: BaseRenderable): TextRenderable | undefined {
  if (renderable instanceof TextRenderable) return renderable;
  for (const child of renderable.getChildren()) {
    const text = firstTextRenderable(child);
    if (text) return text;
  }
  return undefined;
}

/** Remove Mant's decoration without touching syntax or inline styles. */
export function clearSearchHighlight(renderable: TextRenderable | null): void {
  if (!renderable) return;
  highlightInternals(renderable).textBuffer.removeHighlightsByRef(SEARCH_HIGHLIGHT_REF);
  renderable.requestRender();
}

/** Apply a high-priority color overlay without invalidating text layout. */
export function applySearchHighlight(
  target: BaseRenderable,
  range: SearchRange,
): TextRenderable | undefined {
  const renderable = firstTextRenderable(target);
  if (!renderable) return undefined;

  const { textBuffer, _textBufferSyntaxStyle: syntaxStyle } = highlightInternals(renderable);
  const styleId = syntaxStyle.resolveStyleId(SEARCH_HIGHLIGHT_NAME)
    ?? syntaxStyle.registerStyle(SEARCH_HIGHLIGHT_NAME, {
      fg: SEARCH_HIGHLIGHT_FOREGROUND,
      bg: SEARCH_HIGHLIGHT_BACKGROUND,
    });
  textBuffer.removeHighlightsByRef(SEARCH_HIGHLIGHT_REF);
  textBuffer.addHighlightByCharRange({
    start: range.start,
    end: range.end,
    styleId,
    priority: 10_000,
    hlRef: SEARCH_HIGHLIGHT_REF,
  });
  renderable.requestRender();
  return renderable;
}
