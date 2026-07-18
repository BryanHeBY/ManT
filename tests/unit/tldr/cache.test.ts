/**
 * @file Tests installed-client cache lookup and both TLDR update strategies.
 */

import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import {
  createCachedTldrPageFetcher,
  createTldrCacheUpdater,
  getSystemTldrCacheDirs,
  getTldrCacheDir,
  getTldrLanguages,
  getTldrPlatforms,
  getTldrReadCacheDirs,
  normalizeTldrTopic,
  parseTldrCommand,
  parseTldrPage,
} from "../../../src/tldr";

const page = `# tar

> Archiving utility.
> More information: <https://www.gnu.org/software/tar>.

- Create an archive:
  \`tar {{[-c|--create]}} {{path/to/archive.tar}} {{path/to/file}}\`

- Extract an archive: \`tar --extract --file {{path/to/archive.tar}}\`
`;

describe("tldr cache and parser", () => {
  test("parses examples and resolves long option placeholders", () => {
    const parsed = parseTldrPage(page, {
      platform: "linux",
      language: "en",
      sourcePath: "/cache/pages/linux/tar.md",
    });

    expect(parsed.title).toBe("tar");
    expect(parsed.description).toEqual(["Archiving utility."]);
    expect(parsed.moreInformation).toBe("https://www.gnu.org/software/tar.");
    expect(parsed.examples).toHaveLength(2);
    expect(parsed.examples[0]?.command).toBe("tar {{[-c|--create]}} {{path/to/archive.tar}} {{path/to/file}}");
    expect(parsed.examples[0]?.commandParts).toEqual([
      { type: "text", content: "tar " },
      { type: "placeholder", content: "--create" },
      { type: "text", content: " " },
      { type: "placeholder", content: "path/to/archive.tar" },
      { type: "text", content: " " },
      { type: "placeholder", content: "path/to/file" },
    ]);
  });

  test("keeps escaped placeholder braces literal", () => {
    expect(parseTldrCommand("echo \\{\\{not_a_placeholder\\}\\} {{value}}")).toEqual([
      { type: "text", content: "echo {{not_a_placeholder}} " },
      { type: "placeholder", content: "value" },
    ]);
  });

  test("uses locale and host-platform pages before common and other platforms", async () => {
    const cacheDir = "/cache/tldr-pages";
    const preferredPath = join(cacheDir, "pages.zh_TW", "linux", "tar.md");
    const fetchPage = createCachedTldrPageFetcher({
      cacheDir: () => cacheDir,
      env: { LANG: "zh_TW.UTF-8", LANGUAGE: "zh_TW:zh" },
      platform: () => "linux",
      pathExists: async (path) => path === preferredPath,
      readFile: async () => page,
    });

    const parsed = await fetchPage("tar");
    expect(parsed?.language).toBe("zh_TW");
    expect(parsed?.platform).toBe("linux");
    expect(parsed?.sourcePath).toBe(preferredPath);
  });

  test("falls back from the host platform to common English pages", async () => {
    const cacheDir = "/cache/tldr-pages";
    const commonPath = join(cacheDir, "pages", "common", "tar.md");
    const fetchPage = createCachedTldrPageFetcher({
      cacheDir: () => cacheDir,
      env: { LANG: "C" },
      platform: () => "linux",
      pathExists: async (path) => path === commonPath,
      readFile: async () => page,
    });

    const parsed = await fetchPage("tar");
    expect(parsed?.language).toBe("en");
    expect(parsed?.platform).toBe("common");
  });

  test("reads the pages.en layout used by Rust TLDR clients", async () => {
    const cacheDir = "/cache/tlrc";
    const sourcePath = join(cacheDir, "pages.en", "linux", "tar.md");
    const fetchPage = createCachedTldrPageFetcher({
      cacheDirs: () => [cacheDir],
      env: { LANG: "C" },
      platform: () => "linux",
      pathExists: async (path) => path === sourcePath,
      readFile: async () => page,
    });

    const parsed = await fetchPage("tar");
    expect(parsed?.sourcePath).toBe(sourcePath);
    expect(parsed?.language).toBe("en");
  });

  test("prefers the host platform over a translated common page", async () => {
    const translatedCommon = "/cache/pages.zh/common/tar.md";
    const englishLinux = "/cache/pages/linux/tar.md";
    const fetchPage = createCachedTldrPageFetcher({
      cacheDirs: () => ["/cache"],
      env: { LANG: "zh_CN.UTF-8" },
      platform: () => "linux",
      pathExists: async (path) => path === translatedCommon || path === englishLinux,
      readFile: async () => page,
    });

    const parsed = await fetchPage("tar");
    expect(parsed?.sourcePath).toBe(englishLinux);
    expect(parsed?.platform).toBe("linux");
    expect(parsed?.language).toBe("en");
  });

  test("normalises cache paths, topic names, language, and platform priority", () => {
    expect(getTldrCacheDir({ HOME: "/home/test" }, "linux")).toBe("/home/test/.cache/mant/tldr-pages");
    expect(getTldrCacheDir({ LOCALAPPDATA: "C:/Users/test/AppData/Local" }, "win32"))
      .toBe("C:/Users/test/AppData/Local/mant/tldr-pages");
    expect(getTldrLanguages({ LANG: "pt_BR.UTF-8", LANGUAGE: "zh_TW:pt_BR" }))
      .toEqual(["zh_TW", "zh", "pt_BR", "pt", "en"]);
    expect(getTldrPlatforms("linux").slice(0, 3)).toEqual(["linux", "common", "osx"]);
    expect(normalizeTldrTopic("Git Commit")).toBe("git-commit");
  });

  test("uses installed-client caches before Mant's private fallback", () => {
    const env = { HOME: "/home/test", XDG_CACHE_HOME: "/cache" };
    expect(getSystemTldrCacheDirs(env, "linux")).toEqual([
      "/cache/tldr",
      "/cache/tlrc",
      "/cache/tealdeer/tldr-pages",
      "/home/test/.tldr",
      "/usr/local/share/tldr",
      "/usr/share/tldr",
    ]);
    expect(getTldrReadCacheDirs(env, "linux", "/usr/bin/tldr"))
      .toEqual(getSystemTldrCacheDirs(env, "linux"));
    expect(getTldrReadCacheDirs(env, "linux", null))
      .toEqual(["/cache/mant/tldr-pages"]);
    expect(getTldrReadCacheDirs({ ...env, MANT_TLDR_DIR: "/custom" }, "linux", "/usr/bin/tldr"))
      .toEqual(["/custom"]);
  });

  test("updates an installed client by spawning tldr --update", async () => {
    const commands: string[][] = [];
    const update = createTldrCacheUpdater({
      env: {},
      tldrPath: () => "/usr/bin/tldr",
      gitPath: () => { throw new Error("git lookup should not run"); },
      runCommand: async (command) => {
        commands.push(command);
        return {
          stdout: "Updated cache for language en: 100 entries\n",
          stderr: "",
          exitCode: 0,
        };
      },
    });

    await expect(update()).resolves.toEqual({
      action: "updated",
      client: "/usr/bin/tldr",
      output: "Updated cache for language en: 100 entries",
    });
    expect(commands).toEqual([["/usr/bin/tldr", "--update"]]);
  });

  test("surfaces an installed client's update failure", async () => {
    const update = createTldrCacheUpdater({
      env: {},
      tldrPath: () => "tldr",
      runCommand: async () => ({
        stdout: "",
        stderr: "Unable to update cache",
        exitCode: 1,
      }),
    });

    await expect(update()).rejects.toThrow("Unable to update cache");
  });

  test("clones into a temporary cache directory and then moves it into place", async () => {
    const commands: string[][] = [];
    const created: string[] = [];
    const moved: Array<[string, string]> = [];
    const update = createTldrCacheUpdater({
      cacheDir: () => "/cache/mant/tldr-pages",
      repository: "https://example.test/tldr.git",
      tldrPath: () => null,
      gitPath: () => "git",
      pathExists: async () => false,
      createDirectory: async (path) => { created.push(path); },
      makeTempDirectory: async () => "/cache/mant/tldr-pages.tmp-1",
      moveDirectory: async (from, to) => { moved.push([from, to]); },
      removeDirectory: async () => {},
      runCommand: async (command) => {
        commands.push(command);
        return command.includes("rev-parse")
          ? { stdout: "abc123\n", stderr: "", exitCode: 0 }
          : { stdout: "", stderr: "", exitCode: 0 };
      },
    });

    await expect(update()).resolves.toEqual({
      action: "cloned",
      cacheDir: "/cache/mant/tldr-pages",
      revision: "abc123",
    });
    expect(created).toEqual(["/cache/mant"]);
    expect(commands[0]).toEqual([
      "git", "clone", "--depth=1", "--single-branch", "--branch", "main",
      "https://example.test/tldr.git", "/cache/mant/tldr-pages.tmp-1",
    ]);
    expect(moved).toEqual([["/cache/mant/tldr-pages.tmp-1", "/cache/mant/tldr-pages"]]);
  });

  test("updates an existing checkout without overwriting local changes", async () => {
    const commands: string[][] = [];
    const cacheDir = "/cache/mant/tldr-pages";
    const update = createTldrCacheUpdater({
      cacheDir: () => cacheDir,
      tldrPath: () => null,
      gitPath: () => "git",
      pathExists: async (path) => path === cacheDir || path === join(cacheDir, ".git"),
      runCommand: async (command) => {
        commands.push(command);
        return { stdout: command.includes("rev-parse") ? "def456\n" : "", stderr: "", exitCode: 0 };
      },
    });

    const result = await update();
    expect(result.action).toBe("updated");
    expect(commands[0]).toEqual(["git", "-C", cacheDir, "pull", "--ff-only"]);
  });

  test("preserves the clone failure when temporary cleanup also fails", async () => {
    const update = createTldrCacheUpdater({
      cacheDir: () => "/cache/mant/tldr-pages",
      tldrPath: () => null,
      gitPath: () => "git",
      pathExists: async () => false,
      createDirectory: async () => {},
      makeTempDirectory: async () => "/cache/mant/tldr-pages.tmp-1",
      moveDirectory: async () => {},
      removeDirectory: async () => { throw new Error("cleanup failed"); },
      runCommand: async () => ({
        stdout: "",
        stderr: "network unavailable",
        exitCode: 128,
      }),
    });

    await expect(update()).rejects.toThrow("network unavailable");
  });

  test("keeps an explicit MANT_TLDR_DIR independent from the installed client", async () => {
    const commands: string[][] = [];
    const target = "/custom/tldr";
    const update = createTldrCacheUpdater({
      env: { MANT_TLDR_DIR: target },
      cacheDir: () => target,
      tldrPath: () => "/usr/bin/tldr",
      gitPath: () => "git",
      pathExists: async (path) => path === target || path === join(target, ".git"),
      runCommand: async (command) => {
        commands.push(command);
        return { stdout: "", stderr: "", exitCode: 0 };
      },
    });

    await update();
    expect(commands[0]).toEqual(["git", "-C", target, "pull", "--ff-only"]);
  });
});
