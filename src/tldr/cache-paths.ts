/**
 * @file Resolves Mant-owned and installed-client TLDR cache locations.
 *
 * The TLDR client specification standardises page archives and `--update`, but
 * deliberately leaves cache locations to each client.  These helpers encode
 * the conventions used by the official Python/Rust clients and Tealdeer while
 * preserving Mant's explicit directory override.
 */

import { homedir } from "node:os";
import { delimiter, join } from "node:path";

/** Returns the Git checkout used only when no system `tldr` client exists. */
export function getTldrCacheDir(
  env: Record<string, string | undefined> = process.env,
  platform: string = process.platform,
): string {
  if (env.MANT_TLDR_DIR) return env.MANT_TLDR_DIR;

  const home = env.HOME ?? homedir();
  if (platform === "darwin") {
    return join(home, "Library", "Caches", "mant", "tldr-pages");
  }
  if (platform === "win32") {
    return join(
      env.LOCALAPPDATA ?? env.APPDATA ?? home,
      "mant",
      "tldr-pages",
    );
  }
  return join(
    env.XDG_CACHE_HOME ?? join(home, ".cache"),
    "mant",
    "tldr-pages",
  );
}

/** Returns known cache roots used by installed TLDR clients, in priority order. */
export function getSystemTldrCacheDirs(
  env: Record<string, string | undefined> = process.env,
  platform: string = process.platform,
): string[] {
  const home = env.HOME ?? homedir();
  const portableCache = env.XDG_CACHE_HOME ?? join(home, ".cache");
  const nativeCache = platform === "darwin"
    ? join(home, "Library", "Caches")
    : platform === "win32"
      ? env.LOCALAPPDATA ?? env.APPDATA ?? portableCache
      : portableCache;

  const candidates = [
    // Official Python client.
    join(portableCache, "tldr"),
    // Official Rust client (tlrc).
    join(nativeCache, "tlrc"),
    join(portableCache, "tlrc"),
    // Tealdeer stores the extracted language archives one level deeper.
    join(nativeCache, "tealdeer", "tldr-pages"),
    join(portableCache, "tealdeer", "tldr-pages"),
    // Older Node clients commonly keep a repository-shaped cache here.
    join(home, ".tldr"),
  ];

  if (platform !== "win32") {
    const dataDirectories = env.XDG_DATA_DIRS
      ? env.XDG_DATA_DIRS.split(delimiter)
      : ["/usr/local/share", "/usr/share"];
    for (const directory of dataDirectories) {
      if (directory) candidates.push(join(directory, "tldr"));
    }
  }

  return [...new Set(candidates)];
}

/** Selects installed-client caches or Mant's private fallback cache. */
export function getTldrReadCacheDirs(
  env: Record<string, string | undefined> = process.env,
  platform: string = process.platform,
  tldrPath: string | null = Bun.which("tldr"),
): string[] {
  if (env.MANT_TLDR_DIR) return [env.MANT_TLDR_DIR];
  return tldrPath
    ? getSystemTldrCacheDirs(env, platform)
    : [getTldrCacheDir(env, platform)];
}
