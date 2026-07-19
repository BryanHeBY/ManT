/**
 * @file Verifies native document and tldr rendering in terminal frames.
 */

import { describe, expect, test } from "bun:test";
import type { MantSection } from "../../../src/native";
import {
  mockLsResult,
  mockLsWithTldrResult,
  mockQuery,
} from "../../fixtures/mock-result";
import { installOpenTuiWarningFilter, navLines, renderApp } from "./test-support";

installOpenTuiWarningFilter();

const text = (value: string) => ({ type: "text" as const, value });
const paragraph = (value: string) => ({
  type: "paragraph" as const,
  children: [text(value)],
});

function gccResult() {
  return mockQuery("gcc", [
    { id: "name", title: "NAME", blocks: [paragraph("gcc - GNU C compiler")], children: [] },
    {
      id: "synopsis",
      title: "SYNOPSIS",
      blocks: [{
        type: "paragraph",
        children: [
          { type: "strong", children: [text("gcc")] },
          text(" [options] "),
          { type: "emphasis", children: [text("outfile")] },
        ],
      }],
      children: [],
    },
    { id: "description", title: "DESCRIPTION", blocks: [paragraph("Compile a standard program.")], children: [] },
    {
      id: "options",
      title: "OPTIONS",
      blocks: [],
      children: [
        { id: "summary", title: "Option Summary", blocks: [], children: [] },
        {
          id: "kind",
          title: "Options Controlling the Kind of Output",
          blocks: [paragraph("Choose an output kind.")],
          children: [],
        },
      ],
    },
  ]);
}

function clangDefinitionSections(): MantSection[] {
  return [{
    id: "options",
    title: "OPTIONS",
    blocks: [],
    children: [{
      id: "stage-selection",
      title: "Stage Selection Options",
      blocks: [{
        type: "definition-list",
        items: [
          {
            terms: [[text("-E")]],
            description: [paragraph("Run the preprocessor stage.")],
          },
          {
            terms: [[text("-fsyntax-only")]],
            description: [paragraph("Run parser and semantic analysis stages.")],
          },
        ],
        compact: false,
      }],
      children: [],
    }],
  }];
}

describe("App rendering (e2e)", () => {
  test("renders topic, section titles, and all manual content", async () => {
    const setup = await renderApp(mockLsResult);
    const frame = setup.captureCharFrame();

    expect(frame).toContain("MANUAL");
    expect(frame).toContain("ls");
    expect(frame).toContain("NAME");
    expect(frame).toContain("SYNOPSIS");
    expect(frame).toContain("DESCRIPTION");
    expect(frame).toContain("list directory contents");
    expect(frame).toContain("[OPTION]");
    expect(frame).toContain("List information about files.");
    setup.renderer.destroy();
  });

  test("renders native strong, emphasis, and hierarchical sections", async () => {
    const setup = await renderApp(gccResult(), { width: 100, height: 40 });
    const frame = setup.captureCharFrame();

    for (const value of [
      "gcc",
      "NAME",
      "SYNOPSIS",
      "DESCRIPTION",
      "OPTIONS",
      "standard",
      "outfile",
      "Option Summary",
    ]) expect(frame).toContain(value);
    expect(
      navLines(frame).some((line) => line.includes("Options") && line.includes("Kind")),
    ).toBe(true);
    setup.renderer.destroy();
  });

  test("renders inline code and preformatted blocks", async () => {
    const result = mockQuery("smoke", [{
      id: "code",
      title: "CODE",
      blocks: [
        {
          type: "paragraph",
          children: [
            text("Run "),
            { type: "code", value: "ls -la" },
            text(" to list files."),
          ],
        },
        {
          type: "preformatted",
          children: [
            text("int main() {"),
            { type: "line-break" },
            text("    return 0;"),
            { type: "line-break" },
            text("}"),
          ],
        },
      ],
      children: [],
    }]);
    const setup = await renderApp(result);
    const frame = setup.captureCharFrame();

    expect(frame).toContain("ls -la");
    expect(frame).toContain("int main()");
    expect(frame).toContain("return 0;");
    setup.renderer.destroy();
  });

  test("renders nested native definition blocks without bullet corruption", async () => {
    const result = mockQuery("gcc", [{
      id: "options",
      title: "OPTIONS",
      blocks: [{
        type: "definition-list",
        items: [{
          terms: [[text("-fcond-mismatch")]],
          description: [
            paragraph("Allow conditional expressions"),
            {
              type: "preformatted",
              children: [text("#define abs(n) __builtin_strcpy")],
            },
          ],
        }],
      }],
      children: [],
    }]);
    const setup = await renderApp(result, { width: 100, height: 40 });
    const frame = setup.captureCharFrame();

    expect(frame).toContain("-fcond-mismatch");
    expect(frame).toContain("Allow conditional expressions");
    expect(frame).toContain("#define abs(n)");
    const codeLine = frame.split("\n").find((line) => line.includes("#define abs(n)"));
    expect(codeLine?.includes("•")).toBe(false);
    setup.renderer.destroy();
  });

  test("indents and spaces non-compact native definitions", async () => {
    const setup = await renderApp(
      mockQuery("clang", clangDefinitionSections()),
      { width: 100, height: 28 },
    );
    const lines = setup.captureCharFrame().split("\n").map((line) => line.slice(32));
    const lineIndex = (value: string) => lines.findIndex((line) => line.includes(value));
    const optionsLine = lines[lineIndex("OPTIONS")]!;
    const subsectionLine = lines[lineIndex("Stage Selection Options")]!;
    const termLine = lines[lineIndex("-E")]!;
    const descriptionIndex = lineIndex("Run the preprocessor stage.");
    const nextTermIndex = lineIndex("-fsyntax-only");

    expect(termLine).not.toContain("•");
    expect(subsectionLine.indexOf("Stage Selection Options"))
      .toBeGreaterThan(optionsLine.indexOf("OPTIONS"));
    expect(lines[descriptionIndex]!.indexOf("Run the preprocessor stage."))
      .toBeGreaterThan(termLine.indexOf("-E"));
    expect(nextTermIndex).toBe(descriptionIndex + 2);
    expect(lines[descriptionIndex + 1]?.trim()).toBe("");
    setup.renderer.destroy();
  });

  test("keeps compact native definitions adjacent", async () => {
    const sections = clangDefinitionSections();
    const definitionList = sections[0]!.children[0]!.blocks[0]!;
    if (definitionList.type !== "definition-list") {
      throw new Error("expected definition-list fixture");
    }
    definitionList.compact = true;

    const setup = await renderApp(mockQuery("clang", sections), { width: 100, height: 28 });
    const lines = setup.captureCharFrame().split("\n").map((line) => line.slice(32));
    const descriptionIndex = lines.findIndex((line) => line.includes("Run the preprocessor stage."));
    const nextTermIndex = lines.findIndex((line) => line.includes("-fsyntax-only"));

    expect(nextTermIndex).toBe(descriptionIndex + 1);
    setup.renderer.destroy();
  });

  test("honours per-item man paragraph spacing inside a compact definition list", async () => {
    const sections = clangDefinitionSections();
    const definitionList = sections[0]!.children[0]!.blocks[0]!;
    if (definitionList.type !== "definition-list") {
      throw new Error("expected definition-list fixture");
    }
    definitionList.compact = true;
    definitionList.items[1]!.spacingBeforeLines = 1;

    const setup = await renderApp(mockQuery("clang", sections), { width: 100, height: 28 });
    const lines = setup.captureCharFrame().split("\n").map((line) => line.slice(32));
    const descriptionIndex = lines.findIndex((line) => line.includes("Run the preprocessor stage."));
    const nextTermIndex = lines.findIndex((line) => line.includes("-fsyntax-only"));

    expect(nextTermIndex).toBe(descriptionIndex + 2);
    expect(lines[descriptionIndex + 1]?.trim()).toBe("");
    setup.renderer.destroy();
  });

  test("renders native lists, tables, equations, and unsupported nodes", async () => {
    const result = mockQuery("structures", [{
      id: "structures",
      title: "STRUCTURES",
      blocks: [
        {
          type: "list",
          kind: "bullet",
          items: [{ blocks: [paragraph("first item")] }],
        },
        {
          type: "table",
          rows: [{ cells: [
            { blocks: [paragraph("left cell")] },
            { blocks: [paragraph("right cell")] },
          ] }],
        },
        { type: "equation", value: "x = y + 1", display: true },
        { type: "unsupported", name: "custom", text: "unrendered custom macro" },
      ],
      children: [],
    }]);
    const setup = await renderApp(result, { width: 100, height: 28 });
    const frame = setup.captureCharFrame();

    expect(frame).toContain("• first item");
    expect(frame).toContain("left cell");
    expect(frame).toContain("right cell");
    expect(frame).toContain("x = y + 1");
    expect(frame).toContain("unrendered custom macro");
    setup.renderer.destroy();
  });

  test("produces a non-empty frame with menu and compact status bars", async () => {
    const setup = await renderApp(mockLsResult);
    const frame = setup.captureCharFrame();

    expect(frame.trim().length).toBeGreaterThan(0);
    for (const label of ["File", "View", "Navigate", "Search", "Help"]) {
      expect(frame.split("\n")[0]).toContain(label);
    }
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
    const { manual: _, ...tldrOnly } = mockLsWithTldrResult;
    const setup = await renderApp(tldrOnly, { width: 100, height: 28 });
    const frame = setup.captureCharFrame();

    expect(frame).toContain("TLDR QUICK REFERENCE · ls");
    expect(frame).toContain("No local man page was found");
    expect(navLines(frame).some((line) => line.includes("◆ TLDR QUICK REFERENCE"))).toBe(true);
    setup.renderer.destroy();
  });

  test("indents preformatted blocks to the native section body level", async () => {
    const result = mockQuery("git", [
      {
        id: "synopsis",
        title: "SYNOPSIS",
        blocks: [{
          type: "preformatted",
          children: [text("git [-v | --version]")],
        }],
        children: [],
      },
      {
        id: "description",
        title: "DESCRIPTION",
        blocks: [paragraph("Git is a fast version control system.")],
        children: [],
      },
    ]);
    const setup = await renderApp(result, { width: 100, height: 40 });
    const frame = setup.captureCharFrame();
    const columnOf = (needle: string): number => frame.split("\n")
      .map((line) => line.indexOf(needle)).find((column) => column >= 0) ?? -1;

    expect(columnOf("git [-v | --version]")).toBe(columnOf("Git is a fast"));
    setup.renderer.destroy();
  });

  test("honours explicit vertical space around a preformatted block", async () => {
    const result = mockQuery("spacing", [{
      id: "options",
      title: "OPTIONS",
      blocks: [
        paragraph("Equivalent commands:"),
        { type: "vertical-space", lines: 1 },
        {
          type: "preformatted",
          children: [
            text("command one"),
            { type: "line-break" },
            text("command two"),
          ],
        },
        { type: "vertical-space", lines: 1 },
        paragraph("-c <name>=<value>"),
      ],
      children: [],
    }]);
    const setup = await renderApp(result);
    const lines = setup.captureCharFrame().split("\n");
    const introLine = lines.findIndex((line) => line.includes("Equivalent commands:"));
    const firstCodeLine = lines.findIndex((line) => line.includes("command one"));
    const lastCodeLine = lines.findIndex((line) => line.includes("command two"));

    expect(firstCodeLine).toBe(introLine + 2);
    expect(lines[lastCodeLine + 1]).not.toContain("-c <name>=<value>");
    expect(lines[lastCodeLine + 2]).toContain("-c <name>=<value>");
    setup.renderer.destroy();
  });

  test("does not invent blank rows around adjacent native display blocks", async () => {
    const result = mockQuery("spacing", [{
      id: "examples",
      title: "EXAMPLES",
      blocks: [
        paragraph("Before the display."),
        { type: "preformatted", children: [text("command output")] },
        paragraph("After the display."),
      ],
      children: [],
    }]);
    const setup = await renderApp(result, { width: 100, height: 24 });
    const lines = setup.captureCharFrame().split("\n");
    const beforeLine = lines.findIndex((line) => line.includes("Before the display."));
    const codeLine = lines.findIndex((line) => line.includes("command output"));
    const afterLine = lines.findIndex((line) => line.includes("After the display."));

    expect(codeLine).toBe(beforeLine + 1);
    expect(afterLine).toBe(codeLine + 1);
    setup.renderer.destroy();
  });
});
