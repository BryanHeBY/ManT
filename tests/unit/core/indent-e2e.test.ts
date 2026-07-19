/**
 * @file Checks cross-renderer indentation consistency from parsed output.
 */

import { describe, expect, test } from "bun:test";
import { parseManHtml } from "../../../src/core/parser";
import { parseGroff } from "../../../src/core/groff-parser";
import { loadManPageFixture } from "../../fixtures/man-pages";

// ── End-to-end indent verification ────────────────────────
//
// These tests verify that indent values survive the full pipeline:
// HTML → parseManHtml (format detection) → parseGroff/parseMandoc → BlockNode.indent
//
// The rendering layer (app.tsx) uses `<box paddingLeft={baseIndent + block.indent}>`
// to apply indentation. We test that the parser produces correct indent values
// that the rendering layer can consume.

describe("E2E indent - format detection dispatches correctly", () => {
  test("groff HTML is detected and parsed by parseGroff", () => {
    const groffHtml = `<body>
      <h1>TITLE</h1>
      <h2>NAME<a name="NAME"></a></h2>
      <p style="margin-left:9%;">content</p>
    </body>`;

    const sections = parseManHtml(groffHtml);

    expect(sections).toHaveLength(1);
    expect(sections[0]?.title).toBe("NAME");
    const block = sections[0]?.blocks?.[0];
    expect(block?.type === "paragraph" && block.indent).toBe(7);
  });

  test("mandoc HTML is detected and parsed by parseMandoc", () => {
    const mandocHtml = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="NAME">NAME</h1>
          <p class="Pp">content</p>
        </section>
      </div>
    </body>`;

    const sections = parseManHtml(mandocHtml);

    expect(sections).toHaveLength(1);
    expect(sections[0]?.title).toBe("NAME");
  });
});

// ── Groff indent end-to-end ───────────────────────────────

describe("E2E indent - groff margin-left", () => {
  test("all margin-left values produce correct indent columns", () => {
    const html = `<body>
      <h2>TEST</h2>
      <p style="margin-left:0%;">zero</p>
      <p style="margin-left:5%;">five</p>
      <p style="margin-left:9%;">nine</p>
      <p style="margin-left:10%;">ten</p>
      <p style="margin-left:14%;">fourteen</p>
      <p style="margin-left:18%;">eighteen</p>
      <p style="margin-left:19%;">nineteen</p>
      <p style="margin-left:25%;">twentyfive</p>
    </body>`;

    const sections = parseManHtml(html);
    const blocks = sections[0]?.blocks ?? [];

    // margin-left:X% → Math.round(X / 100 * 80)
    const expected = [0, 4, 7, 8, 11, 14, 15, 20];
    for (let i = 0; i < expected.length; i++) {
      const block = blocks[i];
      expect(block?.type).toBe("paragraph");
      expect(block?.type === "paragraph" && block.indent).toBe(expected[i]!);
    }
  });

  test("indent values are consistent across sections in ls.html", () => {
    const html = loadManPageFixture("ls");
    const sections = parseManHtml(html);

    // Check every section: no paragraph should have indent=0 unless it's
    // the first paragraph in the section (which may legitimately be indent=0).
    for (const section of sections) {
      for (const block of section.blocks) {
        if (block.type === "paragraph") {
          // In ls.html, all content uses margin-left, so indent should be > 0
          // or the block should be empty
          if (block.indent === 0 && block.children.length > 0) {
            const text = block.children
              .map((n) => (n.type === "text" ? n.content : n.type))
              .join("");
            // Allow section headings to have indent 0 (they're headings, not paragraphs)
            // but flag any real content with indent 0
            expect(text.length).toBeLessThanOrEqual(0);
          }
        }
      }
    }
  });
});

describe("E2E indent - groff table parsing", () => {
  test("table content never has indent=0 in ls.html", () => {
    const html = loadManPageFixture("ls");
    const sections = parseGroff(html);

    const desc = sections.find((s) => s.title === "DESCRIPTION");
    const blocks = desc?.blocks ?? [];

    // All blocks in DESCRIPTION should have indent > 0
    // (ls uses margin-left:9% for options, margin-left:18% for descriptions,
    // and tables with td width=9% spacer)
    for (const block of blocks) {
      if (
        block.type === "paragraph"
        || block.type === "list"
        || block.type === "definition-list"
      ) {
        expect(block.indent).toBeGreaterThan(0);
      }
    }
  });

  test("table td indent matches equivalent margin-left indent", () => {
    // In ls.html, -c is in a table with:
    //   <td width="9%"></td><td width="3%"><p>-c</p></td>
    // The cumulative width before the content td is 9%, so indent = round(9/100*80) = 7
    // This should match a <p style="margin-left:9%"> which also gives indent=7.
    const html = loadManPageFixture("ls");
    const sections = parseGroff(html);

    const desc = sections.find((s) => s.title === "DESCRIPTION");
    const blocks = desc?.blocks ?? [];

    // Find blocks that are option flags (contain bold text starting with -)
    const optionBlocks = blocks.filter(
      (b) =>
        b.type === "paragraph" &&
        b.indent === 7 &&
        b.children.some(
          (n) =>
            n.type === "bold" &&
            n.children.some((c) => c.type === "text" && c.content.startsWith("\u2212"))
        )
    );

    // Should have options from both <p> and <table> sources, all with indent=7
    expect(optionBlocks.length).toBeGreaterThan(5);
  });
});

// ── Mandoc indent end-to-end ──────────────────────────────

describe("E2E indent - mandoc", () => {
  test("Bd-indent produces indent=4", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="DESC">DESCRIPTION</h1>
          <p class="Pp">normal (indent=0)</p>
          <div class="Bd-indent">
            <p class="Pp">indented (indent=4)</p>
          </div>
        </section>
      </div>
    </body>`;

    const sections = parseManHtml(html);
    const blocks = sections[0]?.blocks ?? [];

    expect(blocks).toHaveLength(2);
    expect(blocks[0]?.type === "paragraph" && blocks[0].indent).toBe(0);
    expect(blocks[1]?.type === "paragraph" && blocks[1].indent).toBe(4);
  });

  test("dd in Bl-tag with block-level content produces indent=4", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="OPT">OPTIONS</h1>
          <dl class="Bl-tag">
            <dt><b>-flag</b></dt>
            <dd>
              Description text.
              <pre>code example</pre>
            </dd>
          </dl>
        </section>
      </div>
    </body>`;

    const sections = parseManHtml(html);
    const blocks = sections[0]?.blocks ?? [];

    // dt: paragraph indent=0
    // dd inline text: paragraph indent=4
    // pre: pre block (no indent field, but rendered separately)
    // trailing whitespace after </pre> may produce an empty paragraph
    expect(blocks.length).toBeGreaterThanOrEqual(3);

    expect(blocks[0]?.type === "paragraph" && blocks[0].indent).toBe(0);
    expect(blocks[1]?.type === "paragraph" && blocks[1].indent).toBe(4);
    expect(blocks[2]?.type).toBe("pre");
  });

  test("mandoc-ls.html has correct indent structure", () => {
    const html = loadManPageFixture("mandoc-ls");
    const sections = parseManHtml(html);

    const desc = sections.find((s) => s.title === "DESCRIPTION");
    expect(desc).toBeDefined();

    // First paragraph should have indent=0 (no Bd-indent)
    const firstBlock = desc?.blocks?.[0];
    expect(firstBlock?.type === "paragraph" && firstBlock.indent).toBe(0);

    // The list (Bl-tag with inline dd) should have indent=0
    const list = desc?.blocks.find((b) => b.type === "definition-list");
    expect(list?.type === "definition-list" && list.indent).toBe(0);
  });
});

// ── Cross-format consistency ───────────────────────────────

describe("E2E indent - cross-format consistency", () => {
  test("groff and mandoc produce same section structure for ls", () => {
    const groffHtml = loadManPageFixture("ls");
    const mandocHtml = loadManPageFixture("mandoc-ls");

    const groffSections = parseManHtml(groffHtml);
    const mandocSections = parseManHtml(mandocHtml);

    const groffTitles = groffSections.map((s) => s.title);
    const mandocTitles = mandocSections.map((s) => s.title);

    // Both should have these core sections
    const coreSections = ["NAME", "SYNOPSIS", "DESCRIPTION", "SEE ALSO"];
    for (const title of coreSections) {
      expect(groffTitles).toContain(title);
      expect(mandocTitles).toContain(title);
    }
  });

  test("both formats preserve bold option flags in DESCRIPTION", () => {
    const groffHtml = loadManPageFixture("ls");
    const mandocHtml = loadManPageFixture("mandoc-ls");

    const groffSections = parseManHtml(groffHtml);
    const mandocSections = parseManHtml(mandocHtml);

    const groffDesc = groffSections.find((s) => s.title === "DESCRIPTION");
    const mandocDesc = mandocSections.find((s) => s.title === "DESCRIPTION");

    const groffHasBoldFlag = groffDesc?.blocks.some(
      (b) =>
        b.type === "paragraph" &&
        b.children.some(
          (n) => n.type === "bold" && n.children.some((c) => c.type === "text")
        )
    );

    const mandocHasBoldFlag = mandocDesc?.blocks.some(
      (b) =>
        b.type === "definition-list" &&
        b.items.some(
          (item) =>
            item.terms.some((term) => term.some(
              (n) => n.type === "bold" && n.children.some((c) => c.type === "text")
            ))
        )
    );

    expect(groffHasBoldFlag).toBe(true);
    expect(mandocHasBoldFlag).toBe(true);
  });
});

// ── Regression tests ──────────────────────────────────────

describe("E2E indent - regression tests", () => {
  test("regression: ls.html table content no longer has indent=0", () => {
    // Before the table parsing fix, all table content was flattened into
    // a single paragraph with indent=0. This test ensures that never happens again.
    const html = loadManPageFixture("ls");
    const sections = parseManHtml(html);

    const desc = sections.find((s) => s.title === "DESCRIPTION");
    const blocks = desc?.blocks ?? [];

    // Check that no block has indent=0 with table-like content
    // (multiple bold elements mixed with text, which was the flattened table symptom)
    for (const block of blocks) {
      if (block.type !== "paragraph" || block.indent !== 0) continue;
      if (block.children.length === 0) continue;

      // Count bold elements — a flattened table would have many
      const boldCount = block.children.filter((n) => n.type === "bold").length;
      expect(boldCount).toBeLessThan(3);
    }
  });

  test("regression: groff-git.html OPTIONS has multi-level indent", () => {
    // The git man page has option names at 9%, descriptions at 14%, examples at 19%
    const html = loadManPageFixture("groff-git");
    const sections = parseManHtml(html);

    const options = sections.find((s) => s.title === "OPTIONS");
    const blocks = options?.blocks ?? [];

    const indents = new Set(
      blocks.filter((b) => b.type === "paragraph").map((b) => (b.type === "paragraph" ? b.indent : -1))
    );

    // Should have at least 2 different indent levels (option names + descriptions)
    expect(indents.size).toBeGreaterThanOrEqual(2);
    // Should contain indent=7 (9% margin-left) and indent=11 (14% margin-left)
    expect(indents.has(7)).toBe(true);
    expect(indents.has(11)).toBe(true);
  });

  test("regression: index.ts barrel export has no duplicates", () => {
    // This was a SyntaxError that wasn't caught by tests because tests
    // imported directly from parser.ts, not from index.ts.
    // This test ensures the barrel export works correctly.
    const core = require("../../../src/core");
    expect(typeof core.parseManHtml).toBe("function");
    expect(typeof core.parseGroff).toBe("function");
    expect(typeof core.parseMandoc).toBe("function");
    expect(typeof core.parseInline).toBe("function");
    expect(typeof core.fetchManHtml).toBe("function");
  });
});
