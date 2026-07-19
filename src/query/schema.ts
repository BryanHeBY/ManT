/**
 * @file Defines input options and the combined result returned by a query.
 */

import type { SectionNode } from "../core";
import type { TldrPage } from "../tldr";

export interface QueryOptions {
  topic: string;
  section?: number;
}

export interface QueryResult {
  topic: string;
  section?: number | undefined;
  sections: SectionNode[];
  /** Cached community quick-reference content, independent from man sections. */
  tldr?: TldrPage;
}
