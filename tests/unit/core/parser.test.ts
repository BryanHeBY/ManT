/**
 * @file Tests renderer detection and the unified man HTML parser API.
 */

import { describe, expect, test } from "bun:test";
import { parseManHtml } from "../../../src/core/parser";
import { loadManPageFixture } from "../../fixtures/man-pages";

describe("parseManHtml", () => {
  test("extracts sections from man HTML output", () => {
    const html = `
      <html>
        <body>
          <h1>LS</h1>
          <h2>NAME</h2>
          <p>ls − list directory contents</p>
          <h2>SYNOPSIS</h2>
          <p><b>ls</b> [OPTION]... [FILE]...</p>
          <h2>DESCRIPTION</h2>
          <p>List information about files.</p>
        </body>
      </html>
    `;

    const sections = parseManHtml(html);

    expect(sections).toHaveLength(3);
    expect(sections[0]?.title).toBe("NAME");
    expect(sections[0]?.blocks).toHaveLength(1);
    expect(sections[1]?.title).toBe("SYNOPSIS");
    expect(sections[2]?.title).toBe("DESCRIPTION");
  });

  test("skips content before the first section heading", () => {
    const html = `
      <body>
        <p>Preamble content should be ignored.</p>
        <h2>NAME</h2>
        <p>real content</p>
      </body>
    `;

    const sections = parseManHtml(html);

    expect(sections).toHaveLength(1);
    expect(sections[0]?.title).toBe("NAME");
    expect(sections[0]?.blocks).toHaveLength(1);
  });

  test("preserves inline formatting", () => {
    const html = `
      <body>
        <h2>NAME</h2>
        <p><b>bold</b> and <i>italic</i> text</p>
      </body>
    `;

    const sections = parseManHtml(html);
    const block = sections[0]?.blocks?.[0];

    expect(block?.type).toBe("paragraph");
    expect(block?.type === "paragraph" && block.children).toHaveLength(4);
  });

  test("nests subsections under their parent heading", () => {
    const html = `
      <body>
        <h2>DESCRIPTION</h2>
        <p>overview</p>
        <h3>Details</h3>
        <p>details content</p>
      </body>
    `;

    const sections = parseManHtml(html);

    expect(sections).toHaveLength(1);
    expect(sections[0]?.children).toHaveLength(1);
    expect(sections[0]?.children?.[0]?.title).toBe("Details");
  });

  test("preserves mandoc Bd-indent and Li spans", () => {
    const html = `
      <body>
        <div class="manual-text">
          <section class="Sh">
            <h1 class="Sh" id="NAME">NAME</h1>
            <p class="Pp">tool - example</p>
            <div class="Bd-indent">
              <p class="Pp">indented paragraph</p>
              <dl class="Bl-tag">
                <dt><b>-f</b></dt>
                <dd>option with <span class="Li">literal</span></dd>
              </dl>
            </div>
          </section>
        </div>
      </body>
    `;

    const sections = parseManHtml(html);

    expect(sections).toHaveLength(1);
    expect(sections[0]?.title).toBe("NAME");
    expect(sections[0]?.blocks).toHaveLength(3);

    const normalParagraph = sections[0]?.blocks?.[0];
    expect(normalParagraph?.type).toBe("paragraph");
    expect(normalParagraph?.type === "paragraph" && normalParagraph.indent).toBe(0);

    const indentedParagraph = sections[0]?.blocks?.[1];
    expect(indentedParagraph?.type).toBe("paragraph");
    expect(indentedParagraph?.type === "paragraph" && indentedParagraph.indent).toBe(4);

    const list = sections[0]?.blocks?.[2];
    expect(list?.type).toBe("list");
    expect(list?.type === "list" && list.indent).toBe(4);
  });

  test("expands mandoc definition lists with pre blocks into separate blocks", () => {
    const html = `
      <body>
        <div class="manual-text">
          <section class="Sh">
            <h1 class="Sh" id="OPTIONS">OPTIONS</h1>
            <dl class="Bl-tag">
              <dt><b>-fcond-mismatch</b></dt>
              <dd>
                Allow conditional expressions with mismatched types.
                <pre>        #define abs(n)          __builtin_abs ((n))
        #define strcpy(d, s)    __builtin_strcpy ((d), (s))</pre>
                More text after the example.
              </dd>
            </dl>
          </section>
        </div>
      </body>
    `;

    const sections = parseManHtml(html);

    expect(sections).toHaveLength(1);
    expect(sections[0]?.blocks).toHaveLength(4);

    const termParagraph = sections[0]?.blocks?.[0];
    expect(termParagraph?.type).toBe("paragraph");
    expect(termParagraph?.type === "paragraph" && termParagraph.indent).toBe(0);

    const descriptionParagraph = sections[0]?.blocks?.[1];
    expect(descriptionParagraph?.type).toBe("paragraph");

    const preBlock = sections[0]?.blocks?.[2];
    expect(preBlock?.type).toBe("pre");
    if (preBlock?.type === "pre") {
      const text = preBlock.children
        .map((n) => (n.type === "text" ? n.content : ""))
        .join("");
      expect(text).toContain("#define abs(n)");
      expect(text).toContain("__builtin_strcpy");
    }

    const afterParagraph = sections[0]?.blocks?.[3];
    expect(afterParagraph?.type).toBe("paragraph");
  });
});

describe("parseManHtml with fixtures", () => {
  test("parses ls man page sections", () => {
    const html = loadManPageFixture("ls");
    const sections = parseManHtml(html);

    const titles = sections.map((s) => s.title);
    expect(titles).toContain("NAME");
    expect(titles).toContain("SYNOPSIS");
    expect(titles).toContain("DESCRIPTION");
    expect(titles).toContain("SEE ALSO");

    const description = sections.find((s) => s.title === "DESCRIPTION");
    expect(description?.children.map((c) => c.title)).toContain("Exit status:");
  });

  test("parses gcc man page hierarchy", () => {
    const html = loadManPageFixture("gcc");
    const sections = parseManHtml(html);

    expect(sections.map((s) => s.title)).toEqual([
      "NAME",
      "SYNOPSIS",
      "DESCRIPTION",
      "OPTIONS",
      "ENVIRONMENT",
      "SEE ALSO",
    ]);

    const options = sections.find((s) => s.title === "OPTIONS");
    expect(options?.children.map((c) => c.title)).toEqual([
      "Option Summary",
      "Options Controlling the Kind of Output",
    ]);
  });

  test("preserves bold and italic parameters in gcc SYNOPSIS", () => {
    const html = loadManPageFixture("gcc");
    const sections = parseManHtml(html);
    const synopsis = sections.find((s) => s.title === "SYNOPSIS");
    const block = synopsis?.blocks?.[0];

    expect(block?.type).toBe("paragraph");
    if (block?.type !== "paragraph") return;

    const hasBoldOption = block.children.some(
      (n) =>
        n.type === "bold" &&
        n.children.some((c) => c.type === "text" && c.content.includes("−c"))
    );
    const hasItalicValue = block.children.some(
      (n) =>
        n.type === "italic" &&
        n.children.some(
          (c) => c.type === "text" && c.content.includes("standard")
        )
    );

    expect(hasBoldOption).toBe(true);
    expect(hasItalicValue).toBe(true);
  });

  test("preserves explicit line breaks in gcc SYNOPSIS", () => {
    const html = loadManPageFixture("gcc");
    const sections = parseManHtml(html);
    const synopsis = sections.find((s) => s.title === "SYNOPSIS");
    const block = synopsis?.blocks?.[0];

    expect(block?.type).toBe("paragraph");
    if (block?.type !== "paragraph") return;

    const breakCount = block.children.filter((n) => n.type === "break").length;
    expect(breakCount).toBeGreaterThan(0);
  });

  test("preserves bold option flags in ls DESCRIPTION", () => {
    const html = loadManPageFixture("ls");
    const sections = parseManHtml(html);
    const description = sections.find((s) => s.title === "DESCRIPTION");
    const blocks = description?.blocks ?? [];

    const hasBoldFlag = blocks.some((block) => {
      if (block.type !== "paragraph") return false;
      return block.children.some(
        (n) =>
          n.type === "bold" &&
          n.children.some(
            (c) => c.type === "text" && c.content.startsWith("−")
          )
      );
    });

    expect(hasBoldFlag).toBe(true);
  });

  test("parses paragraph indentation from margin-left", () => {
    const html = loadManPageFixture("gcc");
    const sections = parseManHtml(html);
    const synopsis = sections.find((s) => s.title === "SYNOPSIS");
    const block = synopsis?.blocks?.[0];

    expect(block?.type).toBe("paragraph");
    expect(block?.type === "paragraph" && block.indent).toBeGreaterThan(0);
  });
});
