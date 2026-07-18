/**
 * @file Tests groff/man-db HTML parsing, including indentation and tables.
 */

import { describe, expect, test } from "bun:test";
import { parseGroff } from "../../../src/core/groff-parser";
import { loadManPageFixture } from "../../fixtures/man-pages";

// ── Inline HTML tests ──────────────────────────────────────

describe("parseGroff - basic structure", () => {
  test("extracts h2 sections", () => {
    const html = `<body>
      <h2>NAME</h2>
      <p>ls - list directory contents</p>
      <h2>SYNOPSIS</h2>
      <p><b>ls</b> [OPTION]...</p>
    </body>`;

    const sections = parseGroff(html);

    expect(sections).toHaveLength(2);
    expect(sections[0]?.title).toBe("NAME");
    expect(sections[1]?.title).toBe("SYNOPSIS");
  });

  test("nests h3 subsections under h2", () => {
    const html = `<body>
      <h2>OPTIONS</h2>
      <p>overview</p>
      <h3>Output Options</h3>
      <p>output details</p>
      <h2>ENVIRONMENT</h2>
      <p>env section</p>
    </body>`;

    const sections = parseGroff(html);

    expect(sections).toHaveLength(2);
    expect(sections[0]?.title).toBe("OPTIONS");
    expect(sections[0]?.children).toHaveLength(1);
    expect(sections[0]?.children[0]?.title).toBe("Output Options");
    expect(sections[1]?.title).toBe("ENVIRONMENT");
  });

  test("skips h1 title, hr, TOC links, and br", () => {
    const html = `<body>
      <h1 align="center">GIT</h1>
      <a href="#NAME">NAME</a><br>
      <a href="#SYNOPSIS">SYNOPSIS</a><br>
      <hr>
      <h2>NAME<a name="NAME"></a></h2>
      <p>real content</p>
    </body>`;

    const sections = parseGroff(html);

    expect(sections).toHaveLength(1);
    expect(sections[0]?.title).toBe("NAME");
    expect(sections[0]?.blocks).toHaveLength(1);
  });

  test("ignores content before first heading", () => {
    const html = `<body>
      <p>orphan content</p>
      <h2>NAME</h2>
      <p>real content</p>
    </body>`;

    const sections = parseGroff(html);

    expect(sections).toHaveLength(1);
    expect(sections[0]?.blocks).toHaveLength(1);
  });
});

// ── Indent tests ───────────────────────────────────────────

describe("parseGroff - indent parsing", () => {
  test("parses margin-left percentages into column counts", () => {
    const html = `<body>
      <h2>TEST</h2>
      <p style="margin-left:9%; margin-top: 1em">indent 9%</p>
      <p style="margin-left:14%;">indent 14%</p>
      <p style="margin-left:18%;">indent 18%</p>
      <p style="margin-left:19%; margin-top: 1em">indent 19%</p>
      <p>no indent</p>
    </body>`;

    const sections = parseGroff(html);
    const blocks = sections[0]?.blocks ?? [];

    expect(blocks[0]?.type).toBe("paragraph");
    expect(blocks[0]?.type === "paragraph" && blocks[0].indent).toBe(7);

    expect(blocks[1]?.type === "paragraph" && blocks[1].indent).toBe(11);

    expect(blocks[2]?.type === "paragraph" && blocks[2].indent).toBe(14);

    expect(blocks[3]?.type === "paragraph" && blocks[3].indent).toBe(15);

    expect(blocks[4]?.type === "paragraph" && blocks[4].indent).toBe(0);
  });

  test("preserves different indents as separate blocks", () => {
    const html = `<body>
      <h2>OPTIONS</h2>
      <p style="margin-left:9%;"><b>-a</b></p>
      <p style="margin-left:18%;">do not ignore entries</p>
      <p style="margin-left:9%;"><b>-A</b></p>
      <p style="margin-left:18%;">do not list implied</p>
    </body>`;

    const sections = parseGroff(html);
    const blocks = sections[0]?.blocks ?? [];

    expect(blocks).toHaveLength(4);
    expect(blocks[0]?.type === "paragraph" && blocks[0].indent).toBe(7);
    expect(blocks[1]?.type === "paragraph" && blocks[1].indent).toBe(14);
    expect(blocks[2]?.type === "paragraph" && blocks[2].indent).toBe(7);
    expect(blocks[3]?.type === "paragraph" && blocks[3].indent).toBe(14);
  });
});

// ── Table parsing tests ────────────────────────────────────

describe("parseGroff - table parsing", () => {
  test("parses layout tables with td width-based indent", () => {
    const html = `<body>
      <h2>DESCRIPTION</h2>
      <table width="100%" border="0" rules="none" frame="void" cellspacing="0" cellpadding="0">
      <tr valign="top" align="left">
      <td width="9%"></td>
      <td width="3%"><p><b>-c</b></p></td>
      <td width="6%"></td>
      <td width="82%"><p>sort by ctime</p></td>
      </tr>
      <tr valign="top" align="left">
      <td width="9%"></td>
      <td width="3%"><p><b>-C</b></p></td>
      <td width="6%"></td>
      <td width="82%"><p>list by columns</p></td>
      </tr>
      </table>
    </body>`;

    const sections = parseGroff(html);
    const blocks = sections[0]?.blocks ?? [];

    // Row 1: -c (indent 7), description (indent 14)
    // Row 2: -C (indent 7), description (indent 14)
    expect(blocks).toHaveLength(4);

    expect(blocks[0]?.type).toBe("paragraph");
    expect(blocks[0]?.type === "paragraph" && blocks[0].indent).toBe(7);

    expect(blocks[1]?.type).toBe("paragraph");
    expect(blocks[1]?.type === "paragraph" && blocks[1].indent).toBe(14);

    expect(blocks[2]?.type === "paragraph" && blocks[2].indent).toBe(7);
    expect(blocks[3]?.type === "paragraph" && blocks[3].indent).toBe(14);
  });

  test("handles tables with different width distributions", () => {
    const html = `<body>
      <h2>TEST</h2>
      <table>
      <tr>
      <td width="9%"></td>
      <td width="8%"><p><b>--si</b></p></td>
      <td width="4%"></td>
      <td width="79%"><p>description text</p></td>
      </tr>
      </table>
    </body>`;

    const sections = parseGroff(html);
    const blocks = sections[0]?.blocks ?? [];

    // --si: cumulative before = 9% → indent 7
    // description: cumulative before = 9+8+4 = 21% → indent 17
    expect(blocks).toHaveLength(2);
    expect(blocks[0]?.type === "paragraph" && blocks[0].indent).toBe(7);
    expect(blocks[1]?.type === "paragraph" && blocks[1].indent).toBe(17);
  });

  test("handles empty table rows gracefully", () => {
    const html = `<body>
      <h2>TEST</h2>
      <table>
      <tr><td width="9%"></td><td width="3%"></td><td width="88%"></td></tr>
      <tr><td width="9%"></td><td width="3%"><p>content</p></td><td width="88%"></td></tr>
      </table>
    </body>`;

    const sections = parseGroff(html);
    const blocks = sections[0]?.blocks ?? [];

    // First row has no content, should produce no blocks
    // Second row has one content cell
    expect(blocks).toHaveLength(1);
    expect(blocks[0]?.type === "paragraph" && blocks[0].indent).toBe(7);
  });
});

// ── Inline formatting tests ────────────────────────────────

describe("parseGroff - inline formatting", () => {
  test("preserves bold, italic, and font color tags", () => {
    const html = `<body>
      <h2>DESC</h2>
      <p style="margin-left:9%;">See <b>gittutorial</b>(7) and <i>path</i> and <font color="#0000FF">link</font></p>
    </body>`;

    const sections = parseGroff(html);
    const block = sections[0]?.blocks?.[0];

    expect(block?.type).toBe("paragraph");
    if (block?.type !== "paragraph") return;

    const hasBold = block.children.some(
      (n) => n.type === "bold" && n.children.some((c) => c.type === "text" && c.content.includes("gittutorial"))
    );
    const hasItalic = block.children.some(
      (n) => n.type === "italic" && n.children.some((c) => c.type === "text" && c.content.includes("path"))
    );
    // <font color> is transparent — its children pass through as plain text
    const hasLinkText = block.children.some(
      (n) => n.type === "text" && n.content.includes("link")
    );

    expect(hasBold).toBe(true);
    expect(hasItalic).toBe(true);
    expect(hasLinkText).toBe(true);
  });

  test("preserves br as break nodes", () => {
    const html = `<body>
      <h2>SYNOPSIS</h2>
      <p style="margin-left:9%;"><b>git</b> [<b>--version</b>] <br> [<b>--exec-path</b>]</p>
    </body>`;

    const sections = parseGroff(html);
    const block = sections[0]?.blocks?.[0];

    expect(block?.type).toBe("paragraph");
    if (block?.type !== "paragraph") return;

    const breakCount = block.children.filter((n) => n.type === "break").length;
    expect(breakCount).toBe(1);
  });

  test("preserves pre blocks", () => {
    const html = `<body>
      <h2>EXAMPLES</h2>
      <pre>git --git-dir=a.git --work-tree=b status
git --git-dir=c/a.git --work-tree=c/b status</pre>
    </body>`;

    const sections = parseGroff(html);
    const block = sections[0]?.blocks?.[0];

    expect(block?.type).toBe("pre");
    if (block?.type !== "pre") return;

    const text = block.children
      .map((n) => (n.type === "text" ? n.content : ""))
      .join("");
    expect(text).toContain("--git-dir=a.git");
    expect(text).toContain("--work-tree=c/b");
  });
});

// ── Fixture-based tests ────────────────────────────────────

describe("parseGroff - fixtures", () => {
  test("parses groff-git.html sections and hierarchy", () => {
    const html = loadManPageFixture("groff-git");
    const sections = parseGroff(html);

    const titles = sections.map((s) => s.title);
    expect(titles).toContain("NAME");
    expect(titles).toContain("SYNOPSIS");
    expect(titles).toContain("DESCRIPTION");
    expect(titles).toContain("OPTIONS");
    expect(titles).toContain("GIT COMMANDS");
    expect(titles).toContain("SEE ALSO");

    // GIT COMMANDS has a subsection
    const gitCmds = sections.find((s) => s.title === "GIT COMMANDS");
    expect(gitCmds?.children.map((c) => c.title)).toContain(
      "High\u2212Level Commands (Porcelain)"
    );
  });

  test("parses groff-git.html OPTIONS with correct indent levels", () => {
    const html = loadManPageFixture("groff-git");
    const sections = parseGroff(html);

    const options = sections.find((s) => s.title === "OPTIONS");
    const blocks = options?.blocks ?? [];

    // Option names at margin-left:9% → indent 7
    const optionNameBlocks = blocks.filter(
      (b) => b.type === "paragraph" && b.indent === 7
    );
    expect(optionNameBlocks.length).toBeGreaterThan(0);

    // Descriptions at margin-left:14% → indent 11
    const descBlocks = blocks.filter(
      (b) => b.type === "paragraph" && b.indent === 11
    );
    expect(descBlocks.length).toBeGreaterThan(0);

    // Examples at margin-left:19% → indent 15
    const exampleBlocks = blocks.filter(
      (b) => b.type === "paragraph" && b.indent === 15
    );
    expect(exampleBlocks.length).toBeGreaterThan(0);
  });

  test("parses ls.html table content with correct indent", () => {
    const html = loadManPageFixture("ls");
    const sections = parseGroff(html);

    const desc = sections.find((s) => s.title === "DESCRIPTION");
    expect(desc).toBeDefined();
    const blocks = desc?.blocks ?? [];

    // All table-sourced blocks should have indent > 0 (not 0)
    // Tables use td width: 9% spacer + 3% content → indent 7
    const tableBlocks = blocks.filter((b) => b.type === "paragraph" && b.indent === 7);
    expect(tableBlocks.length).toBeGreaterThan(5);

    // Description blocks at 18% → indent 14
    const descBlocks = blocks.filter((b) => b.type === "paragraph" && b.indent === 14);
    expect(descBlocks.length).toBeGreaterThan(5);

    // No block should have indent 0 (orphaned table content)
    const zeroIndentBlocks = blocks.filter(
      (b) => b.type === "paragraph" && b.indent === 0 && b.children.length > 0
    );
    expect(zeroIndentBlocks.length).toBe(0);
  });

  test("parses ls.html Exit status subsection", () => {
    const html = loadManPageFixture("ls");
    const sections = parseGroff(html);

    const desc = sections.find((s) => s.title === "DESCRIPTION");
    const exitStatus = desc?.children.find((c) => c.title.includes("Exit status"));
    expect(exitStatus).toBeDefined();
    expect(exitStatus?.blocks.length).toBeGreaterThan(0);
  });
});
