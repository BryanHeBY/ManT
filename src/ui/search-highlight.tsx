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
export const SEARCH_MATCH_BACKGROUND = "#45475a";

const SEARCH_MATCH_NAME = "mant.search.match";
const SEARCH_ACTIVE_NAME = "mant.search.active";
// OpenTUI's native Highlight struct stores references as u16. Values outside
// that range are truncated on insertion and can no longer be removed by the
// original JS number.
const SEARCH_MATCH_REF = 0x4d01;
const SEARCH_ACTIVE_REF = 0x4d02;

/**
 * Convert a source-string range to OpenTUI's flattened TextBuffer positions.
 *
 * Search records retain `\n` so matching and row selection see the same lines
 * as the AST. OpenTUI's highlight API addresses visible characters across
 * those lines and excludes the newline separators themselves. Without this
 * conversion, a match on line N shifts right by the preceding N - 1 breaks.
 */
export function toTextBufferRange(text: string, range: SearchRange): SearchRange {
  let breaksBeforeStart = 0;
  let breaksBeforeEnd = 0;
  for (let index = 0; index < range.end; index++) {
    if (text[index] !== "\n") continue;
    if (index < range.start) breaksBeforeStart++;
    breaksBeforeEnd++;
  }
  return {
    start: range.start - breaksBeforeStart,
    end: range.end - breaksBeforeEnd,
  };
}

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
  registerStyle(name: string, style: { fg?: string; bg?: string }): number;
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

function resolveStyle(
  renderable: TextRenderable,
  name: string,
  definition: { fg?: string; bg?: string },
): number {
  const syntaxStyle = highlightInternals(renderable)._textBufferSyntaxStyle;
  return syntaxStyle.resolveStyleId(name)
    ?? syntaxStyle.registerStyle(name, definition);
}

/** Remove only the high-priority current-result decoration. */
export function clearActiveSearchHighlight(renderable: TextRenderable | null): void {
  if (!renderable) return;
  highlightInternals(renderable).textBuffer.removeHighlightsByRef(SEARCH_ACTIVE_REF);
  renderable.requestRender();
}

/** Remove every search decoration from the buffers touched by the last query. */
export function clearSearchHighlights(renderables: Iterable<TextRenderable>): void {
  for (const renderable of renderables) {
    const { textBuffer } = highlightInternals(renderable);
    textBuffer.removeHighlightsByRef(SEARCH_MATCH_REF);
    textBuffer.removeHighlightsByRef(SEARCH_ACTIVE_REF);
    renderable.requestRender();
  }
}

/** Apply all ordinary matches in one TextBuffer without invalidating layout. */
export function applySearchMatchHighlights(
  target: BaseRenderable,
  ranges: readonly SearchRange[],
): TextRenderable | undefined {
  const renderable = firstTextRenderable(target);
  if (!renderable) return undefined;

  const { textBuffer } = highlightInternals(renderable);
  const styleId = resolveStyle(renderable, SEARCH_MATCH_NAME, {
    bg: SEARCH_MATCH_BACKGROUND,
  });
  textBuffer.removeHighlightsByRef(SEARCH_MATCH_REF);
  for (const range of ranges) {
    textBuffer.addHighlightByCharRange({
      start: range.start,
      end: range.end,
      styleId,
      // OpenTUI stores this field as an unsigned byte. Keep both search
      // priorities within 0..255 so the active layer reliably sorts last.
      priority: 100,
      hlRef: SEARCH_MATCH_REF,
    });
  }
  renderable.requestRender();
  return renderable;
}

/** Overlay the currently selected result above the ordinary match layer. */
export function applyActiveSearchHighlight(
  renderable: TextRenderable,
  range: SearchRange,
): void {
  const { textBuffer } = highlightInternals(renderable);
  const styleId = resolveStyle(renderable, SEARCH_ACTIVE_NAME, {
    fg: SEARCH_HIGHLIGHT_FOREGROUND,
    bg: SEARCH_HIGHLIGHT_BACKGROUND,
  });
  textBuffer.removeHighlightsByRef(SEARCH_ACTIVE_REF);
  textBuffer.addHighlightByCharRange({
    start: range.start,
    end: range.end,
    styleId,
    priority: 200,
    hlRef: SEARCH_ACTIVE_REF,
  });
  renderable.requestRender();
}
