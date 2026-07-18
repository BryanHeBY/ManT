/**
 * @file Declares the small structured model used to render cached tldr pages.
 */

export type TldrCommandPart =
  | { type: "text"; content: string }
  | { type: "placeholder"; content: string };

export interface TldrExample {
  description: string;
  command: string;
  commandParts: TldrCommandPart[];
}

export interface TldrPage {
  title: string;
  description: string[];
  moreInformation?: string;
  examples: TldrExample[];
  platform: string;
  language: string;
  sourcePath: string;
}

export interface TldrCacheUpdate {
  action: "cloned" | "updated";
  cacheDir: string;
  revision?: string;
}
