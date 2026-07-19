/**
 * @file Defines stable renderable identifiers shared by navigation, document,
 * and search code.
 */

/** The synthetic navigation entry rendered before a cached tldr page. */
export const TLDR_NAV_ID = "tldr-quick-reference";

export function navId(id: string): string {
  return `nav-${id}`;
}

export function contentId(id: string): string {
  return `content-${id}`;
}

/** Identifies a zero-width destination embedded in manual body content. */
export function contentAnchorId(id: string): string {
  return `content-anchor-${id}`;
}

/** Identifies the one body block currently targeted by a search result. */
export function contentBlockId(sectionId: string, blockIndex: number): string {
  return `${contentId(sectionId)}-block-${blockIndex}`;
}
