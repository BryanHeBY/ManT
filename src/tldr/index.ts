/**
 * @file Exposes tldr cache, parsing, and data-model APIs.
 */

export {
  createCachedTldrPageFetcher,
  createTldrCacheUpdater,
  fetchCachedTldrPage,
  getTldrCacheDir,
  getTldrLanguages,
  getTldrPlatforms,
  normalizeTldrTopic,
  updateTldrCache,
  type CachedTldrPageDependencies,
  type TldrCacheUpdateDependencies,
} from "./cache";
export { getSystemTldrCacheDirs, getTldrReadCacheDirs } from "./cache-paths";
export { parseTldrCommand, parseTldrPage, tldrPageText, type TldrPageLocation } from "./parser";
export type { TldrCacheUpdate, TldrCommandPart, TldrExample, TldrPage } from "./types";
