/**
 * @file Manages the local tldr-pages Git cache and reads command quick references.
 */

import { access, mkdir, mkdtemp, rename, rm } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { runCommand as runProcessCommand } from "../core/process";
import { getTldrCacheDir, getTldrReadCacheDirs } from "./cache-paths";
import { parseTldrPage } from "./parser";
import type { TldrCacheUpdate, TldrPage } from "./types";

// ── Cache conventions and dependency contracts ──────────────

const DEFAULT_REPOSITORY = "https://github.com/tldr-pages/tldr.git";
const ALL_PLATFORMS = [
  "common",
  "linux",
  "osx",
  "macos",
  "windows",
  "android",
  "freebsd",
  "openbsd",
  "netbsd",
  "sunos",
  "cisco-ios",
  "dos",
];

export interface TldrCommandResult {
  stdout: string;
  stderr: string;
  exitCode: number;
}

type TldrCommandRunner = (command: string[]) => Promise<TldrCommandResult>;

export interface CachedTldrPageDependencies {
  cacheDir?: () => string;
  cacheDirs?: () => string[];
  env?: Record<string, string | undefined>;
  platform?: () => string;
  tldrPath?: () => string | null;
  pathExists?: (path: string) => Promise<boolean>;
  readFile?: (path: string) => Promise<string>;
}

export interface TldrCacheUpdateDependencies {
  cacheDir?: () => string;
  env?: Record<string, string | undefined>;
  repository?: string;
  tldrPath?: () => string | null;
  gitPath?: () => string | null;
  pathExists?: (path: string) => Promise<boolean>;
  createDirectory?: (path: string) => Promise<void>;
  makeTempDirectory?: (prefix: string) => Promise<string>;
  moveDirectory?: (from: string, to: string) => Promise<void>;
  removeDirectory?: (path: string) => Promise<void>;
  runCommand?: TldrCommandRunner;
}

// ── Default host adapters ───────────────────────────────────

async function pathExists(path: string): Promise<boolean> {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

async function readFile(path: string): Promise<string> {
  return Bun.file(path).text();
}

async function runCommand(command: string[]): Promise<TldrCommandResult> {
  const result = await runProcessCommand(command);
  return {
    stdout: new TextDecoder().decode(result.stdout),
    stderr: result.stderr,
    exitCode: result.exitCode,
  };
}

function commandError(command: string[], result: TldrCommandResult): Error {
  return new Error(result.stderr.trim() || `${command.join(" ")} failed with code ${result.exitCode}`);
}

// ── Platform and locale resolution ──────────────────────────

function normalizeLocale(locale: string): string[] {
  const normalized = locale.split(".", 1)[0]!.replace("-", "_");
  if (!normalized || normalized === "C" || normalized === "POSIX") return [];
  const language = normalized.split("_", 1)[0]!;
  return normalized === language ? [language] : [normalized, language];
}

export function getTldrLanguages(
  env: Record<string, string | undefined> = process.env,
): string[] {
  const languages: string[] = [];
  if (env.LANG && env.LANG !== "C" && env.LANG !== "POSIX") {
    for (const locale of env.LANGUAGE?.split(":") ?? []) {
      languages.push(...normalizeLocale(locale));
    }
    languages.push(...normalizeLocale(env.LANG));
  }
  languages.push("en");
  return [...new Set(languages)];
}

export function getTldrPlatforms(platform: string = process.platform): string[] {
  const hostPlatforms: Record<string, string[]> = {
    darwin: ["osx", "macos"],
    win32: ["windows"],
    linux: ["linux"],
    android: ["android"],
    freebsd: ["freebsd"],
    openbsd: ["openbsd"],
    netbsd: ["netbsd"],
    sunos: ["sunos"],
  };
  return [...new Set([...(hostPlatforms[platform] ?? []), "common", ...ALL_PLATFORMS])];
}

export function normalizeTldrTopic(topic: string): string {
  return topic.trim().toLocaleLowerCase().replace(/\s+/g, "-");
}

// ── Cache reads ─────────────────────────────────────────────

/** Reads a cached tldr page only; it never initiates a network request. */
export function createCachedTldrPageFetcher(
  dependencies: CachedTldrPageDependencies = {},
): (topic: string) => Promise<TldrPage | null> {
  const env = dependencies.env ?? process.env;
  const getPlatform = dependencies.platform ?? (() => process.platform);
  const getTldrPath = dependencies.tldrPath ?? (() => Bun.which("tldr"));
  const explicitCacheDir = dependencies.cacheDir;
  const getCacheDirs = dependencies.cacheDirs
    ?? (explicitCacheDir
      ? () => [explicitCacheDir()]
      : () => getTldrReadCacheDirs(env, getPlatform(), getTldrPath()));
  const exists = dependencies.pathExists ?? pathExists;
  const read = dependencies.readFile ?? readFile;

  return async function fetchCachedTldrPage(topic: string): Promise<TldrPage | null> {
    const pageName = normalizeTldrTopic(topic);
    if (!pageName) return null;

    const cacheDirs = getCacheDirs();
    // The client specification gives host platform precedence over language.
    for (const platform of getTldrPlatforms(getPlatform())) {
      for (const language of getTldrLanguages(env)) {
        // Repository/Python caches use `pages` for English, whereas current
        // Rust clients consistently extract it as `pages.en`.
        const pagesDirectories = language === "en"
          ? ["pages", "pages.en"]
          : [`pages.${language}`];
        for (const cacheDir of cacheDirs) {
          for (const pagesDirectory of pagesDirectories) {
            const sourcePath = join(cacheDir, pagesDirectory, platform, `${pageName}.md`);
            if (!await exists(sourcePath)) continue;
            return parseTldrPage(await read(sourcePath), { language, platform, sourcePath });
          }
        }
      }
    }
    return null;
  };
}

// ── Transactional cache updates ─────────────────────────────

export function createTldrCacheUpdater(
  dependencies: TldrCacheUpdateDependencies = {},
): () => Promise<TldrCacheUpdate> {
  const env = dependencies.env ?? process.env;
  const cacheDir = dependencies.cacheDir ?? (() => getTldrCacheDir(env));
  const repository = dependencies.repository ?? DEFAULT_REPOSITORY;
  const tldrPath = dependencies.tldrPath ?? (() => Bun.which("tldr"));
  const gitPath = dependencies.gitPath ?? (() => Bun.which("git"));
  const exists = dependencies.pathExists ?? pathExists;
  const createDirectory = dependencies.createDirectory ?? (async (path) => { await mkdir(path, { recursive: true }); });
  const makeTempDirectory = dependencies.makeTempDirectory ?? mkdtemp;
  const moveDirectory = dependencies.moveDirectory ?? rename;
  const removeDirectory = dependencies.removeDirectory ?? (async (path) => { await rm(path, { recursive: true, force: true }); });
  const execute = dependencies.runCommand ?? runCommand;

  return async function updateTldrCache(): Promise<TldrCacheUpdate> {
    // An explicit Mant directory is intentionally independent from whichever
    // TLDR client is installed, so keep maintaining it as a Git checkout.
    const systemTldr = env.MANT_TLDR_DIR ? null : tldrPath();
    if (systemTldr) {
      const command = [systemTldr, "--update"];
      const result = await execute(command);
      if (result.exitCode !== 0) throw commandError(command, result);
      const output = [result.stdout.trim(), result.stderr.trim()]
        .filter(Boolean)
        .join("\n");
      return {
        action: "updated",
        client: systemTldr,
        ...(output ? { output } : {}),
      };
    }

    const git = gitPath();
    if (!git) {
      throw new Error(
        "cannot update tldr pages: install a `tldr` client or git",
      );
    }

    const target = cacheDir();
    let action: TldrCacheUpdate["action"];
    if (await exists(target)) {
      if (!await exists(join(target, ".git"))) {
        throw new Error(`${target} exists but is not a tldr git checkout`);
      }
      const command = [git, "-C", target, "pull", "--ff-only"];
      const result = await execute(command);
      if (result.exitCode !== 0) throw commandError(command, result);
      action = "updated";
    } else {
      const parent = dirname(target);
      await createDirectory(parent);
      const temporary = await makeTempDirectory(join(parent, `${basename(target)}.tmp-`));
      const command = [git, "clone", "--depth=1", "--single-branch", "--branch", "main", repository, temporary];
      try {
        const result = await execute(command);
        if (result.exitCode !== 0) throw commandError(command, result);
        await moveDirectory(temporary, target);
      } catch (error) {
        try {
          await removeDirectory(temporary);
        } catch (cleanupError) {
          // Preserve the clone/rename failure that explains why the update did
          // not complete; cleanup detail remains available in debug mode.
          if (process.env.MANT_DEBUG) {
            console.warn(
              `could not remove incomplete tldr cache ${temporary}: ${String(cleanupError)}`,
            );
          }
        }
        throw error;
      }
      action = "cloned";
    }

    const revisionCommand = [git, "-C", target, "rev-parse", "--short", "HEAD"];
    const revisionResult = await execute(revisionCommand);
    return {
      action,
      cacheDir: target,
      ...(revisionResult.exitCode === 0 ? { revision: revisionResult.stdout.trim() } : {}),
    };
  };
}

export const fetchCachedTldrPage = createCachedTldrPageFetcher();
export const updateTldrCache = createTldrCacheUpdater();

export { getTldrCacheDir } from "./cache-paths";
