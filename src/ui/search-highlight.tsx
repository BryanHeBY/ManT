/**
 * @file Splits rendered inline text into case-insensitive search highlights.
 */

import type { ReactNode } from "react";

export const SEARCH_HIGHLIGHT_BACKGROUND = "#f9e2af";
export const SEARCH_HIGHLIGHT_FOREGROUND = "#1e1e2e";

export interface SearchHighlightRange {
  start: number;
  end: number;
}

export interface SearchTextFragment {
  text: string;
  highlighted: boolean;
}

export function getSearchHighlightRanges(text: string, query: string): SearchHighlightRange[] {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return [];

  const haystack = text.toLocaleLowerCase();
  const ranges: SearchHighlightRange[] = [];
  let cursor = 0;
  while (cursor < haystack.length) {
    const start = haystack.indexOf(needle, cursor);
    if (start < 0) break;
    ranges.push({ start, end: start + needle.length });
    cursor = start + needle.length;
  }
  return ranges;
}

/** Splits one rendered token against match ranges from its original source. */
export function splitTextByHighlightRanges(
  text: string,
  ranges: SearchHighlightRange[],
  offset = 0,
): SearchTextFragment[] {
  if (!text || ranges.length === 0) return text ? [{ text, highlighted: false }] : [];

  const tokenEnd = offset + text.length;
  const fragments: SearchTextFragment[] = [];
  let cursor = offset;
  for (const range of ranges) {
    if (range.end <= cursor || range.start >= tokenEnd) continue;
    const start = Math.max(range.start, cursor);
    const end = Math.min(range.end, tokenEnd);
    if (start > cursor) {
      fragments.push({ text: text.slice(cursor - offset, start - offset), highlighted: false });
    }
    fragments.push({ text: text.slice(start - offset, end - offset), highlighted: true });
    cursor = end;
  }
  if (cursor < tokenEnd) {
    fragments.push({ text: text.slice(cursor - offset), highlighted: false });
  }
  return fragments;
}

export function renderSearchHighlights(
  text: string,
  query: string,
  keyPrefix: string | number,
  foreground = SEARCH_HIGHLIGHT_FOREGROUND,
): ReactNode[] {
  return splitTextByHighlightRanges(text, getSearchHighlightRanges(text, query)).map(
    (fragment, index) => fragment.highlighted ? (
      <span key={`${keyPrefix}-${index}`} fg={foreground} bg={SEARCH_HIGHLIGHT_BACKGROUND}>
        {fragment.text}
      </span>
    ) : fragment.text,
  );
}
