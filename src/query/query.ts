import { fetchManHtml, parseManHtml } from "../core";
import type { QueryOptions, QueryResult } from "./schema";

export async function query(options: QueryOptions): Promise<QueryResult> {
  const html = await fetchManHtml(options.topic);
  const sections = parseManHtml(html);

  return {
    topic: options.topic,
    section: options.section,
    sections,
  };
}
