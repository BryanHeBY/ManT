import { describe, expect, test } from "bun:test";
import { parseGroff, parseManHtml, parseMandoc } from "../../../src/core";
import type { InlineNode, SectionNode } from "../../../src/core";

const decoder = new TextDecoder();
const RENDERER_HTML_TAG = /<\/?(?:p|pre|div|section|span|br|b|i|dl|dt|dd|ul|ol|li|table|tr|td|th|thead|tbody|tfoot|em|strong|tt|u)\b[^>]*>/i;

function commandExists(command: string): boolean {
  return Bun.which(command) !== null;
}

function hasManPage(topic: string): boolean {
  return Bun.spawnSync(["man", "-w", topic], {
    stdout: "ignore",
    stderr: "ignore",
  }).exitCode === 0;
}

const canRenderLs =
  commandExists("man") &&
  commandExists("mandoc") &&
  commandExists("zcat") &&
  hasManPage("ls");

const canRenderGit = canRenderLs && hasManPage("git");

interface ProcessResult {
  stdout: Uint8Array;
  stderr: string;
}

async function run(command: string[], stdin?: Uint8Array): Promise<ProcessResult> {
  const process = Bun.spawn(command, {
    stdin: stdin ?? "ignore",
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(process.stdout).arrayBuffer(),
    new Response(process.stderr).text(),
    process.exited,
  ]);

  if (exitCode !== 0) {
    throw new Error(
      `${command.join(" ")} failed with code ${exitCode}: ${stderr.trim()}`,
    );
  }
  return { stdout: new Uint8Array(stdout), stderr };
}

async function loadManSource(topic: string): Promise<Uint8Array> {
  const located = await run(["man", "-w", topic]);
  const path = decoder.decode(located.stdout).trim().split(/\r?\n/, 1)[0];
  if (!path) throw new Error(`man -w ${topic} returned no source path`);

  if (path.endsWith(".gz")) return (await run(["zcat", path])).stdout;
  if (path.endsWith(".xz")) return (await run(["xzcat", path])).stdout;
  if (path.endsWith(".bz2")) return (await run(["bzcat", path])).stdout;
  if (path.endsWith(".zst")) return (await run(["zstdcat", path])).stdout;
  return new Uint8Array(await Bun.file(path).arrayBuffer());
}

async function renderBoth(topic: string): Promise<{
  groffHtml: string;
  mandocHtml: string;
}> {
  const [groff, source] = await Promise.all([
    run(["man", "-Thtml", topic]),
    loadManSource(topic),
  ]);
  const mandoc = await run(["mandoc", "-Wunsupp", "-Thtml"], source);
  return {
    groffHtml: decoder.decode(groff.stdout),
    mandocHtml: decoder.decode(mandoc.stdout),
  };
}

function flattenSections(nodes: SectionNode[]): SectionNode[] {
  return nodes.flatMap((node) => [node, ...flattenSections(node.children)]);
}

function flattenInline(nodes: InlineNode[]): string {
  return nodes
    .map((node) => {
      if (node.type === "text") return node.content;
      if (node.type === "break") return "\n";
      return flattenInline(node.children);
    })
    .join("");
}

function assertWellFormedTree(sections: SectionNode[]): void {
  const allSections = flattenSections(sections);
  expect(new Set(allSections.map((section) => section.id)).size).toBe(
    allSections.length,
  );

  for (const section of allSections) {
    expect(section.title.trim()).not.toBe("");
    expect(section.blocks.length + section.children.length).toBeGreaterThan(0);
    for (const block of section.blocks) {
      const content = block.type === "list"
        ? block.items.map(flattenInline).join("\n")
        : block.type === "spacer" ? "" : flattenInline(block.children);
      expect(content).not.toMatch(RENDERER_HTML_TAG);
    }
  }
}

const describeLsRenderers = canRenderLs ? describe : describe.skip;

describeLsRenderers("actual ls renderer output", () => {
  test("parses the current man-db and mandoc HTML into the same section hierarchy", async () => {
    const { groffHtml, mandocHtml } = await renderBoth("ls");
    const groffSections = parseManHtml(groffHtml);
    const mandocSections = parseManHtml(mandocHtml);

    expect(groffHtml).toContain("groff -Thtml");
    expect(mandocHtml).toContain("manual-text");
    expect(groffSections).toEqual(parseGroff(groffHtml));
    expect(mandocSections).toEqual(parseMandoc(mandocHtml));
    expect(groffSections.map((section) => section.title)).toEqual(
      mandocSections.map((section) => section.title),
    );
    expect(groffSections.map((section) => section.title)).toEqual([
      "NAME",
      "SYNOPSIS",
      "DESCRIPTION",
      "AUTHOR",
      "REPORTING BUGS",
      "COPYRIGHT",
      "SEE ALSO",
    ]);

    for (const sections of [groffSections, mandocSections]) {
      expect(sections.find((section) => section.title === "DESCRIPTION")?.children
        .map((section) => section.title)).toContain("Exit status:");
      expect(flattenSections(sections)
        .flatMap((section) => section.blocks)
        .some((block) => block.type !== "list" && block.type !== "spacer" && flattenInline(block.children).includes("list directory contents")))
        .toBe(true);
      assertWellFormedTree(sections);
    }
  });
});

const describeGitRenderers = canRenderGit ? describe : describe.skip;

describeGitRenderers("actual git renderer output", () => {
  test("keeps the large document's section topology and inline content intact", async () => {
    const { groffHtml, mandocHtml } = await renderBoth("git");
    const groffSections = parseManHtml(groffHtml);
    const mandocSections = parseManHtml(mandocHtml);

    expect(groffSections.map((section) => section.title)).toEqual(
      mandocSections.map((section) => section.title),
    );
    expect(groffSections).toHaveLength(24);
    expect(groffSections.map((section) => section.title)).toContain("OPTIONS");
    expect(groffSections.find((section) => section.title === "ENVIRONMENT VARIABLES")?.children
      .map((section) => section.title)).toContain("Git Diffs");

    for (const sections of [groffSections, mandocSections]) {
      expect(flattenSections(sections)
        .flatMap((section) => section.blocks)
        .some((block) => block.type !== "list" && block.type !== "spacer" && flattenInline(block.children).includes("Git is a fast")))
        .toBe(true);
      assertWellFormedTree(sections);
    }
  });
});
