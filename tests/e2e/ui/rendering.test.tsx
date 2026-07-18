/**
 * @file Verifies that representative man and tldr documents render intact in
 * the terminal interface, including roff formatting and spacing regressions.
 */

import { describe, expect, test } from "bun:test";
import { parseManHtml } from "../../../src/core/parser";
import type { QueryResult } from "../../../src/query";
import { mockLsResult, mockLsWithTldrResult } from "../../fixtures/mock-result";
import { loadManPageFixture } from "../../fixtures/man-pages";
import { mandocHtmlWithPreInDefinitionList } from "./manual-fixtures";
import { installOpenTuiWarningFilter, navLines, renderApp } from "./test-support";

installOpenTuiWarningFilter();

describe("App rendering (e2e)", () => {
  test("renders topic and section titles", async () => {
    const setup = await renderApp(mockLsResult);
    const frame = setup.captureCharFrame();

    expect(frame).toContain("MANUAL");
    expect(frame).toContain("ls");
    expect(frame).toContain("NAME");
    expect(frame).toContain("SYNOPSIS");
    expect(frame).toContain("DESCRIPTION");

    setup.renderer.destroy();
  });

  test("renders full manual content, not just selected section", async () => {
    const setup = await renderApp(mockLsResult);
    const frame = setup.captureCharFrame();

    expect(frame).toContain("list directory contents");
    expect(frame).toContain("[OPTION]");
    expect(frame).toContain("List information about files.");

    setup.renderer.destroy();
  });

  test("renders gcc full manual with bold and italic parameters", async () => {
    const setup = await renderApp(
      { topic: "gcc", sections: parseManHtml(loadManPageFixture("gcc")) },
      { width: 100, height: 40 },
    );
    const frame = setup.captureCharFrame();

    expect(frame).toContain("MANUAL");
    expect(frame).toContain("gcc");
    expect(frame).toContain("NAME");
    expect(frame).toContain("SYNOPSIS");
    expect(frame).toContain("DESCRIPTION");
    expect(frame).toContain("OPTIONS");
    expect(frame).toContain("standard");
    expect(frame).toContain("outfile");

    setup.renderer.destroy();
  });

  test("renders hierarchical subsections", async () => {
    const setup = await renderApp(
      { topic: "gcc", sections: parseManHtml(loadManPageFixture("gcc")) },
      { width: 100, height: 40 },
    );
    const frame = setup.captureCharFrame();

    expect(frame).toContain("Option Summary");
    expect(
      navLines(frame).some(
        (line) => line.includes("Options") && line.includes("Kind"),
      ),
    ).toBe(true);

    setup.renderer.destroy();
  });

  test("renders inline code and pre blocks", async () => {
    const result: QueryResult = {
      topic: "smoke",
      sections: [{
        id: "section-0",
        title: "CODE",
        level: 2,
        blocks: [
          {
            type: "paragraph",
            children: [
              { type: "text", content: "Run " },
              { type: "code", children: [{ type: "text", content: "ls -la" }] },
              { type: "text", content: " to list files." },
            ],
            indent: 0,
          },
          {
            type: "pre",
            children: [
              { type: "text", content: "int main() {" },
              { type: "break" },
              { type: "text", content: "    return 0;" },
              { type: "break" },
              { type: "text", content: "}" },
            ],
            indent: 0,
          },
        ],
        children: [],
      }],
    };
    const setup = await renderApp(result);
    const frame = setup.captureCharFrame();

    expect(frame).toContain("MANUAL");
    expect(frame).toContain("smoke");
    expect(frame).toContain("CODE");
    expect(frame).toContain("ls -la");
    expect(frame).toContain("int main()");
    expect(frame).toContain("return 0;");

    setup.renderer.destroy();
  });

  test("renders a mandoc pre block in a definition list as code", async () => {
    const setup = await renderApp(
      {
        topic: "gcc",
        sections: parseManHtml(mandocHtmlWithPreInDefinitionList()),
      },
      { width: 100, height: 40 },
    );
    const frame = setup.captureCharFrame();

    expect(frame).toContain("-fcond-mismatch");
    expect(frame).toContain("Allow conditional expressions");
    expect(frame).toContain("#define abs(n)");
    expect(frame).toContain("__builtin_strcpy");
    const codeLine = frame.split("\n").find((line) => line.includes("#define abs(n)"));
    expect(codeLine).toBeDefined();
    expect(codeLine?.includes("•")).toBe(false);

    setup.renderer.destroy();
  });

  test("produces a non-empty frame", async () => {
    const setup = await renderApp(mockLsResult);
    const frame = setup.captureCharFrame();

    expect(frame.trim().length).toBeGreaterThan(0);
    expect(frame).toContain("MANUAL");
    expect(frame).toContain("ls");

    setup.renderer.destroy();
  });

  test("exposes the classic menu bar and compact status bar", async () => {
    const setup = await renderApp(mockLsResult);
    const frame = setup.captureCharFrame();

    expect(frame.split("\n")[0]).toContain("File");
    expect(frame.split("\n")[0]).toContain("View");
    expect(frame.split("\n")[0]).toContain("Navigate");
    expect(frame.split("\n")[0]).toContain("Search");
    expect(frame.split("\n")[0]).toContain("Help");
    expect(frame).toContain("1/3 · NAME");
    expect(frame).toContain("3 visible manual sections");

    setup.renderer.destroy();
  });

  test("renders the tldr quick reference before the man page", async () => {
    const setup = await renderApp(mockLsWithTldrResult, { width: 100, height: 32 });
    const frame = setup.captureCharFrame();
    const tldrPosition = frame.indexOf("TLDR QUICK REFERENCE · ls");
    const manualPosition = frame.indexOf("list directory contents");

    expect(navLines(frame).some((line) => line.includes("◆ TLDR QUICK REFERENCE"))).toBe(true);
    expect(frame).toContain("tldr-pages · CC BY 4.0 · common · en");
    expect(frame).toContain("List files, including hidden entries");
    expect(tldrPosition).toBeGreaterThanOrEqual(0);
    expect(manualPosition).toBeGreaterThan(tldrPosition);
    expect(frame).toContain("› ◆ TLDR QUICK REFERENCE");

    setup.renderer.destroy();
  });

  test("keeps cached tldr content usable without a local man page", async () => {
    const setup = await renderApp(
      { ...mockLsWithTldrResult, sections: [] },
      { width: 100, height: 28 },
    );
    const frame = setup.captureCharFrame();

    expect(frame).toContain("TLDR QUICK REFERENCE · ls");
    expect(frame).toContain("No local man page was found");
    expect(navLines(frame).some((line) => line.includes("◆ TLDR QUICK REFERENCE"))).toBe(true);

    setup.renderer.destroy();
  });

  test("indents SYNOPSIS pre blocks to the section body", async () => {
    // Regression: OpenTUI ignores paddingLeft on <text>, so Pre applies
    // indentation to its wrapping box instead.
    const setup = await renderApp(
      { topic: "git", sections: parseManHtml(loadManPageFixture("mandoc-git")) },
      { width: 100, height: 40 },
    );
    const frame = setup.captureCharFrame();
    const columnOf = (needle: string): number => {
      for (const line of frame.split("\n")) {
        const column = line.indexOf(needle);
        if (column >= 0) return column;
      }
      return -1;
    };

    const synopsisPreColumn = columnOf("git [-v | --version]");
    const descriptionColumn = columnOf("Git is a fast");
    expect(synopsisPreColumn).toBeGreaterThan(0);
    expect(descriptionColumn).toBeGreaterThan(0);
    expect(synopsisPreColumn).toBe(descriptionColumn);

    setup.renderer.destroy();
  });

  test("keeps a blank row between a pre block and the following option", async () => {
    const result: QueryResult = {
      topic: "spacing",
      sections: [{
        id: "section-0",
        title: "OPTIONS",
        level: 2,
        blocks: [
          {
            type: "paragraph",
            children: [{ type: "text", content: "Equivalent commands:" }],
            indent: 0,
          },
          { type: "spacer", indent: 0 },
          {
            type: "pre",
            children: [
              { type: "text", content: "command one" },
              { type: "break" },
              { type: "text", content: "command two" },
            ],
            indent: 0,
          },
          {
            type: "paragraph",
            children: [{ type: "text", content: "-c <name>=<value>" }],
            indent: 0,
          },
        ],
        children: [],
      }],
    };
    const setup = await renderApp(result);
    const lines = setup.captureCharFrame().split("\n");
    const lastCodeLine = lines.findIndex((line) => line.includes("command two"));

    expect(lastCodeLine).toBeGreaterThanOrEqual(0);
    expect(lines[lastCodeLine + 1]).not.toContain("-c <name>=<value>");
    expect(lines[lastCodeLine + 2]).toContain("-c <name>=<value>");

    setup.renderer.destroy();
  });
});
