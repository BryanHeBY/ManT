import { describe, expect, test } from "bun:test";
import {
  createManHtmlFetcher,
  type CommandResult,
} from "../../../src/core/fetcher";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

function result(
  stdout: string,
  exitCode = 0,
  stderr = "",
): CommandResult {
  return { stdout: encoder.encode(stdout), stderr, exitCode };
}

describe("fetchManHtml", () => {
  test("uses mandoc's HTML mode with unsupported-feature detection", async () => {
    const commands: string[][] = [];
    const fetchManHtml = createManHtmlFetcher({
      isMandocAvailable: () => true,
      readFile: async () => encoder.encode('.TH TOOL 1\n.SH NAME\ntool'),
      runCommand: async (command, options) => {
        commands.push(command);
        if (command[0] === "man" && command[1] === "-w") {
          return result("/fixtures/tool.1\n");
        }
        if (command[0] === "mandoc") {
          expect(command).toEqual(["mandoc", "-Wunsupp", "-Thtml"]);
          expect(decoder.decode(options?.stdin)).toContain(".SH NAME");
          return result("<!DOCTYPE html><body><div class=\"manual-text\"></div></body>");
        }
        throw new Error(`unexpected command: ${command.join(" ")}`);
      },
    });

    await expect(fetchManHtml("tool")).resolves.toContain("manual-text");
    expect(commands).toEqual([
      ["man", "-w", "tool"],
      ["mandoc", "-Wunsupp", "-Thtml"],
    ]);
  });

  test("decompresses a located source page before passing it to mandoc", async () => {
    let readFileCalled = false;
    const fetchManHtml = createManHtmlFetcher({
      isMandocAvailable: () => true,
      readFile: async () => {
        readFileCalled = true;
        return encoder.encode("");
      },
      runCommand: async (command, options) => {
        if (command[0] === "man" && command[1] === "-w") {
          return result("/fixtures/tool.1.gz\n");
        }
        if (command[0] === "zcat") {
          expect(command).toEqual(["zcat", "/fixtures/tool.1.gz"]);
          return result('.TH TOOL 1\n.SH NAME\ntool');
        }
        if (command[0] === "mandoc") {
          expect(decoder.decode(options?.stdin)).toContain(".TH TOOL 1");
          return result("<html>mandoc output</html>");
        }
        throw new Error(`unexpected command: ${command.join(" ")}`);
      },
    });

    await expect(fetchManHtml("tool")).resolves.toBe("<html>mandoc output</html>");
    expect(readFileCalled).toBe(false);
  });

  test("falls back to groff HTML when mandoc reports an unsupported feature", async () => {
    const fallbackMessages: string[] = [];
    const fetchManHtml = createManHtmlFetcher({
      isMandocAvailable: () => true,
      readFile: async () => encoder.encode('.TH TOOL 1\n.SH NAME\ntool'),
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

  test("uses groff for sources with .so includes that stdin-fed mandoc cannot resolve", async () => {
    const commands: string[][] = [];
    const fetchManHtml = createManHtmlFetcher({
      isMandocAvailable: () => true,
      readFile: async () => encoder.encode(".so man1/target.1\n"),
      onMandocFallback: () => {},
      runCommand: async (command) => {
        commands.push(command);
        if (command[0] === "man" && command[1] === "-w") {
          return result("/fixtures/alias.1\n");
        }
        if (command[0] === "man" && command[1] === "-Thtml") {
          return result("<html>groff output</html>");
        }
        throw new Error(`unexpected command: ${command.join(" ")}`);
      },
    });

    await expect(fetchManHtml("alias")).resolves.toBe("<html>groff output</html>");
    expect(commands).toEqual([
      ["man", "-w", "alias"],
      ["man", "-Thtml", "alias"],
    ]);
  });

  test("uses groff directly when mandoc is unavailable", async () => {
    const fetchManHtml = createManHtmlFetcher({
      isMandocAvailable: () => false,
      readFile: async () => {
        throw new Error("readFile should not run without mandoc");
      },
      runCommand: async (command) => {
        if (command[0] === "man" && command[1] === "-Thtml") {
          return result("<html>groff output</html>");
        }
        throw new Error(`unexpected command: ${command.join(" ")}`);
      },
    });

    await expect(fetchManHtml("tool")).resolves.toBe("<html>groff output</html>");
  });

  test("uses groff when man cannot locate a source page", async () => {
    const fetchManHtml = createManHtmlFetcher({
      isMandocAvailable: () => true,
      readFile: async () => {
        throw new Error("readFile should not run without a source path");
      },
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
