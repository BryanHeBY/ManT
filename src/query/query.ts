/**
 * @file Combines local manual parsing with optional cached tldr quick references.
 */

import { fetchManHtml, parseManHtml } from "../core";
import { fetchCachedTldrPage, type TldrPage } from "../tldr";
import type { QueryOptions, QueryResult } from "./schema";

export interface QueryDependencies {
  fetchManHtml?: (topic: string) => Promise<string>;
  parseManHtml?: (html: string) => QueryResult["sections"];
  fetchTldrPage?: (topic: string) => Promise<TldrPage | null>;
}

/**
 * Queries the authoritative local man page and, when available, augments it
 * with an offline tldr quick reference.  The tldr cache is never updated as
 * part of a read query.
 */
export function createQuery(dependencies: QueryDependencies = {}) {
  const getManHtml = dependencies.fetchManHtml ?? fetchManHtml;
  const parse = dependencies.parseManHtml ?? parseManHtml;
  const getTldrPage = dependencies.fetchTldrPage ?? fetchCachedTldrPage;

  return async function query(options: QueryOptions): Promise<QueryResult> {
    const topic = options.topic.trim();
    if (!topic) throw new Error("manual topic must not be empty");

    const tldrPagePromise = getTldrPage(topic).catch(() => null);
    try {
      const html = await getManHtml(topic);
      const sections = parse(html);
      const tldr = await tldrPagePromise;

      if (sections.length === 0 && !tldr) {
        throw new Error(`no readable manual content was found for '${topic}'`);
      }

      return {
        topic,
        section: options.section,
        sections,
        ...(tldr ? { tldr } : {}),
      };
    } catch (manError) {
      const tldr = await tldrPagePromise;
      if (!tldr) throw manError;

      return {
        topic,
        section: options.section,
        sections: [],
        tldr,
      };
    }
  };
}

export const query = createQuery();
