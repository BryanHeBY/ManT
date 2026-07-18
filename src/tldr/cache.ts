import { access, mkdir, mkdtemp, rename, rm } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { parseTldrPage } from "./parser";
import type { TldrCacheUpdate, TldrPage } from "./types";

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
  env?: Record<string, string | undefined>;
  platform?: () => string;
  pathExists?: (path: string) => Promise<boolean>;
  readFile?: (path: string) => Promise<string>;
}

export interface TldrCacheUpdateDependencies {
  cacheDir?: () => string;
  repository?: string;
  gitPath?: () => string | null;
  pathExists?: (path: string) => Promise<boolean>;
  createDirectory?: (path: string) => Promise<void>;
  makeTempDirectory?: (prefix: string) => Promise<string>;
  moveDirectory?: (from: string, to: string) => Promise<void>;
  removeDirectory?: (path: string) => Promise<void>;
  runCommand?: TldrCommandRunner;
}

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
  const process = Bun.spawn(command, { stdout: "pipe", stderr: "pipe" });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(process.stdout).text(),
    new Response(process.stderr).text(),
    process.exited,
  ]);
  return { stdout, stderr, exitCode };
}

function commandError(command: string[], result: TldrCommandResult): Error {
  return new Error(result.stderr.trim() || `${command.join(" ")} failed with code ${result.exitCode}`);
}

export function getTldrCacheDir(
  env: Record<string, string | undefined> = process.env,
  platform: string = process.platform,
): string {
  if (env.MANT_TLDR_DIR) return env.MANT_TLDR_DIR;

  if (platform === "darwin") {
    return join(env.HOME ?? ".", "Library", "Caches", "mant", "tldr-pages");
  }
  if (platform === "win32") {
    return join(env.LOCALAPPDATA ?? env.APPDATA ?? ".", "mant", "tldr-pages");
  }
  return join(env.XDG_CACHE_HOME ?? join(env.HOME ?? ".", ".cache"), "mant", "tldr-pages");
}

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

/** Reads a cached tldr page only; it never initiates a network request. */
export function createCachedTldrPageFetcher(
  dependencies: CachedTldrPageDependencies = {},
): (topic: string) => Promise<TldrPage | null> {
  const env = dependencies.env ?? process.env;
  const cacheDir = dependencies.cacheDir ?? (() => getTldrCacheDir(env, dependencies.platform?.()));
  const getPlatform = dependencies.platform ?? (() => process.platform);
  const exists = dependencies.pathExists ?? pathExists;
  const read = dependencies.readFile ?? readFile;

  return async function fetchCachedTldrPage(topic: string): Promise<TldrPage | null> {
    const pageName = normalizeTldrTopic(topic);
    if (!pageName) return null;

    for (const language of getTldrLanguages(env)) {
      const pagesDirectory = language === "en" ? "pages" : `pages.${language}`;
      for (const platform of getTldrPlatforms(getPlatform())) {
        const sourcePath = join(cacheDir(), pagesDirectory, platform, `${pageName}.md`);
        if (!await exists(sourcePath)) continue;
        return parseTldrPage(await read(sourcePath), { language, platform, sourcePath });
      }
    }
    return null;
  };
}

export function createTldrCacheUpdater(
  dependencies: TldrCacheUpdateDependencies = {},
): () => Promise<TldrCacheUpdate> {
  const cacheDir = dependencies.cacheDir ?? (() => getTldrCacheDir());
  const repository = dependencies.repository ?? DEFAULT_REPOSITORY;
  const gitPath = dependencies.gitPath ?? (() => Bun.which("git"));
  const exists = dependencies.pathExists ?? pathExists;
  const createDirectory = dependencies.createDirectory ?? (async (path) => { await mkdir(path, { recursive: true }); });
  const makeTempDirectory = dependencies.makeTempDirectory ?? mkdtemp;
  const moveDirectory = dependencies.moveDirectory ?? rename;
  const removeDirectory = dependencies.removeDirectory ?? (async (path) => { await rm(path, { recursive: true, force: true }); });
  const execute = dependencies.runCommand ?? runCommand;

  return async function updateTldrCache(): Promise<TldrCacheUpdate> {
    const git = gitPath();
    if (!git) throw new Error("git is required for `mant --update-tldr`; install git or provide MANT_TLDR_DIR");

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
        await removeDirectory(temporary);
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
