import type { SectionNode } from "../core";

export interface QueryOptions {
  topic: string;
  section?: number;
  format?: "structured" | "markdown" | "text";
}

export interface QueryResult {
  topic: string;
  section?: number | undefined;
  sections: SectionNode[];
}
