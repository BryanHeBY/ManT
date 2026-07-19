/**
 * @file Tests bundled/system renderer selection and HTML fallback policy.
 */

import { describe, expect, test } from "bun:test";
import {
  createManHtmlFetcher,
  type CommandResult,
  type FetchManHtmlDependencies,
} from "../../../src/core/fetcher";

const encoder = new TextEncoder();

function result(
  stdout: string,
  exitCode = 0,
  stderr = "",
): CommandResult {
  return { stdout: encoder.encode(stdout), stderr, exitCode };
}

/** Creates a deterministic fetcher without using the locally built sidecar. */
function createSystemFetcher(dependencies: FetchManHtmlDependencies) {
  return createManHtmlFetcher({
    getSidecarPath: () => "/fixtures/mant-mandoc-json",
    isSidecarAvailable: async () => false,
    isManHtmlAvailable: () => true,
    ...dependencies,
  });
}

describe("fetchManHtml", () => {
  test("uses mandoc's HTML mode with unsupported-feature detection", async () => {
    const commands: string[][] = [];
    const fetchManHtml = createSystemFetcher({
      isMandocAvailable: () => true,
      runCommand: async (command) => {
        commands.push(command);
        if (command[0] === "man" && command[1] === "-w") {
          return result("/fixtures/tool.1\n");
        }
        if (command[0] === "mandoc") {
          expect(command).toEqual([
            "mandoc",
            "-Wunsupp",
            "-Thtml",
            "/fixtures/tool.1",
          ]);
          return result("<!DOCTYPE html><body><div class=\"manual-text\"></div></body>");
        }
        throw new Error(`unexpected command: ${command.join(" ")}`);
      },
    });

    await expect(fetchManHtml("tool")).resolves.toContain("manual-text");
    expect(commands).toEqual([
      ["man", "-w", "tool"],
      ["mandoc", "-Wunsupp", "-Thtml", "/fixtures/tool.1"],
    ]);
  });

  test("passes the original compressed path to mandoc", async () => {
    const commands: string[][] = [];
    const fetchManHtml = createSystemFetcher({
      isMandocAvailable: () => true,
      runCommand: async (command) => {
        commands.push(command);
        if (command[0] === "man" && command[1] === "-w") {
          return result("/fixtures/tool.1.gz\n");
        }
        if (command[0] === "mandoc") {
          return result("<html>mandoc output</html>");
        }
        throw new Error(`unexpected command: ${command.join(" ")}`);
      },
    });

    await expect(fetchManHtml("tool")).resolves.toBe("<html>mandoc output</html>");
    expect(commands).toEqual([
      ["man", "-w", "tool"],
      ["mandoc", "-Wunsupp", "-Thtml", "/fixtures/tool.1.gz"],
    ]);
  });

  test("falls back to groff HTML when mandoc reports an unsupported feature", async () => {
    const fallbackMessages: string[] = [];
    const fetchManHtml = createSystemFetcher({
      isMandocAvailable: () => true,
      onMandocFallback: (_topic, error) => fallbackMessages.push(error.message),
      runCommand: async (command) => {
        if (command[0] === "man" && command[1] === "-w") {
          return result("/fixtures/tool.1\n");
        }
        if (command[0] === "mandoc") {
          return result("", 4, "unsupported roff request");
        }
        if (command[0] === "man" && command[1] === "-Thtml") {
          return result("<!-- Creator: groff -->\n<body>fallback</body>");
        }
        throw new Error(`unexpected command: ${command.join(" ")}`);
      },
    });

    await expect(fetchManHtml("tool")).resolves.toContain("groff");
    expect(fallbackMessages).toEqual(["unsupported roff request"]);
  });

  test("lets mandoc resolve source includes from the original path", async () => {
    const commands: string[][] = [];
    const fetchManHtml = createSystemFetcher({
      isMandocAvailable: () => true,
      runCommand: async (command, options) => {
        commands.push(command);
        if (command[0] === "man" && command[1] === "-w") {
          return result("/fixtures/man1/alias.1\n");
        }
        if (command[0] === "mandoc") {
          expect(options?.cwd).toBe("/fixtures");
          return result("<html>resolved include</html>");
        }
        throw new Error(`unexpected command: ${command.join(" ")}`);
      },
    });

    await expect(fetchManHtml("alias")).resolves.toBe("<html>resolved include</html>");
    expect(commands).toEqual([
      ["man", "-w", "alias"],
      ["mandoc", "-Wunsupp", "-Thtml", "/fixtures/man1/alias.1"],
    ]);
  });

  test("uses bundled best-effort HTML when BSD man has no HTML device", async () => {
    const commands: string[][] = [];
    const fetchManHtml = createManHtmlFetcher({
      getSidecarPath: () => "/bundled/mant-mandoc-json",
      isSidecarAvailable: async () => true,
      isMandocAvailable: () => false,
      isManHtmlAvailable: () => false,
      onMandocFallback: () => {},
      runCommand: async (command) => {
        commands.push(command);
        if (command[0] === "man" && command[1] === "-w") {
          return result("/fixtures/gcc.1.gz\n");
        }
        if (command[0] === "/bundled/mant-mandoc-json") {
          return result("<html>best effort gcc</html>", 4, "unsupported roff request");
        }
        throw new Error(`unexpected command: ${command.join(" ")}`);
      },
    });

    await expect(fetchManHtml("gcc")).resolves.toBe("<html>best effort gcc</html>");
    expect(commands).toEqual([
      ["man", "-w", "gcc"],
      ["/bundled/mant-mandoc-json", "--html", "/fixtures/gcc.1.gz"],
    ]);
  });

  test("uses groff directly when mandoc is unavailable", async () => {
    const fetchManHtml = createSystemFetcher({
      isMandocAvailable: () => false,
      runCommand: async (command) => {
        if (command[0] === "man" && command[1] === "-w") {
          return result("/fixtures/tool.1\n");
        }
        if (command[0] === "man" && command[1] === "-Thtml") {
          return result("<html>groff output</html>");
        }
        throw new Error(`unexpected command: ${command.join(" ")}`);
      },
    });

    await expect(fetchManHtml("tool")).resolves.toBe("<html>groff output</html>");
  });

  test("uses groff when man cannot locate a source page", async () => {
    const fetchManHtml = createSystemFetcher({
      isMandocAvailable: () => true,
      runCommand: async (command) => {
        if (command[0] === "man" && command[1] === "-w") {
          return result("", 1, "No manual entry");
        }
        if (command[0] === "man" && command[1] === "-Thtml") {
          return result("<html>groff output</html>");
        }
        throw new Error(`unexpected command: ${command.join(" ")}`);
      },
    });

    await expect(fetchManHtml("tool")).resolves.toBe("<html>groff output</html>");
  });
});
