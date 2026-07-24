/**
 * @file Defines stable renderable identifiers shared by navigation, document,
 * and search code.
 */

/** The synthetic navigation entry rendered before a cached tldr page. */
export const TLDR_NAV_ID = "tldr-quick-reference";

/** Synthetic navigation target for content preceding the first heading. */
export const DOCUMENT_ROOT_ID = "document-overview";

export function navId(id: string): string {
  return `nav-${id}`;
}

export function contentId(id: string): string {
  return `content-${id}`;
}

/** Identifies one stable rendered target in the immutable page-search index. */
export function contentSearchId(sectionId: string, targetPath: string): string {
  return `${contentId(sectionId)}-search-${targetPath}`;
}

/** Identifies a zero-width destination embedded in manual body content. */
export function contentAnchorId(id: string): string {
  return `content-anchor-${id}`;
}
