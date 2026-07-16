import { describe, expect, test } from "bun:test";
import { parseMandoc } from "../../../src/core/mandoc-parser";
import { loadManPageFixture } from "../../fixtures/man-pages";

// ── Inline HTML tests ──────────────────────────────────────

describe("parseMandoc - basic structure", () => {
  test("extracts section.Sh headings", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="NAME">NAME</h1>
          <p class="Pp">ls - list directory contents</p>
        </section>
        <section class="Sh">
          <h1 class="Sh" id="SYNOPSIS">SYNOPSIS</h1>
          <p class="Pp"><b>ls</b> [OPTION]...</p>
        </section>
      </div>
    </body>`;

    const sections = parseMandoc(html);

    expect(sections).toHaveLength(2);
    expect(sections[0]?.title).toBe("NAME");
    expect(sections[0]?.level).toBe(2);
    expect(sections[1]?.title).toBe("SYNOPSIS");
  });

  test("nests section.Ss under section.Sh", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="DESCRIPTION">DESCRIPTION</h1>
          <p class="Pp">overview</p>
          <section class="Ss">
            <h2 class="Ss" id="Exit">Exit status</h2>
            <p class="Pp">exit details</p>
          </section>
        </section>
      </div>
    </body>`;

    const sections = parseMandoc(html);

    expect(sections).toHaveLength(1);
    expect(sections[0]?.title).toBe("DESCRIPTION");
    expect(sections[0]?.children).toHaveLength(1);
    expect(sections[0]?.children[0]?.title).toBe("Exit status");
    expect(sections[0]?.children[0]?.level).toBe(3);
  });

  test("handles permalink anchors inside headings", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="NAME"><a class="permalink" href="#NAME">NAME</a></h1>
          <p class="Pp">content</p>
        </section>
      </div>
    </body>`;

    const sections = parseMandoc(html);

    expect(sections).toHaveLength(1);
    expect(sections[0]?.title).toBe("NAME");
  });

  test("works without div.manual-text wrapper", () => {
    const html = `<body>
      <section class="Sh">
        <h1 class="Sh" id="NAME">NAME</h1>
        <p class="Pp">content</p>
      </section>
    </body>`;

    const sections = parseMandoc(html);

    expect(sections).toHaveLength(1);
    expect(sections[0]?.title).toBe("NAME");
  });
});

// ── Definition list tests ─────────────────────────────────

describe("parseMandoc - definition lists", () => {
  test("preserves an empty Pp paragraph as a spacer around display blocks", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="EXAMPLES">EXAMPLES</h1>
          <p class="Pp">The following commands are equivalent:</p>
          <p class="Pp"></p>
          <div class="Bd-indent"><pre>git status\ngit diff</pre></div>
          <p class="Pp">Next option</p>
        </section>
      </div>
    </body>`;

    const blocks = parseMandoc(html)[0]?.blocks ?? [];

    expect(blocks.map((block) => block.type)).toEqual([
      "paragraph",
      "spacer",
      "pre",
      "paragraph",
    ]);
    expect(blocks[1]?.type === "spacer" && blocks[1].indent).toBe(0);
    expect(blocks[2]?.type === "pre" && blocks[2].indent).toBe(4);
  });

  test("parses Bl-tag with inline dd content", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="OPTIONS">OPTIONS</h1>
          <dl class="Bl-tag">
            <dt><b>-a</b></dt>
            <dd>do not ignore entries starting with .</dd>
            <dt><b>-A</b></dt>
            <dd>do not list implied . and ..</dd>
          </dl>
        </section>
      </div>
    </body>`;

    const sections = parseMandoc(html);
    const blocks = sections[0]?.blocks ?? [];

    // When dd has only inline content, dl becomes a single list block.
    // parseListItems selects both dt and dd, so 2 dt + 2 dd = 4 items.
    expect(blocks).toHaveLength(1);
    expect(blocks[0]?.type).toBe("list");
    if (blocks[0]?.type !== "list") return;
    expect(blocks[0].items).toHaveLength(4);
    // First item (dt) should have bold -a
    const firstItem = blocks[0].items[0];
    expect(firstItem?.some((n) => n.type === "bold")).toBe(true);
    // Second item (dd) should have plain text
    const secondItem = blocks[0].items[1];
    expect(secondItem?.some((n) => n.type === "text" && n.content.includes("do not ignore"))).toBe(true);
  });

  test("parses Bl-tag with block-level dd content (pre)", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="OPTIONS">OPTIONS</h1>
          <dl class="Bl-tag">
            <dt><b>-fcond-mismatch</b></dt>
            <dd>
              Allow conditional expressions with mismatched types.
              <pre>#define abs(n) __builtin_abs((n))</pre>
              More text after.
            </dd>
          </dl>
        </section>
      </div>
    </body>`;

    const sections = parseMandoc(html);
    const blocks = sections[0]?.blocks ?? [];

    // dt paragraph, dd inline paragraph, pre block, dd after paragraph
    expect(blocks).toHaveLength(4);
    expect(blocks[0]?.type).toBe("paragraph");
    expect(blocks[1]?.type).toBe("paragraph");
    expect(blocks[2]?.type).toBe("pre");
    expect(blocks[3]?.type).toBe("paragraph");
  });

  test("handles empty dt elements", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="DESC">DESC</h1>
          <dl class="Bl-tag">
            <dt></dt>
            <dd>continuation of previous option</dd>
          </dl>
        </section>
      </div>
    </body>`;

    const sections = parseMandoc(html);
    const blocks = sections[0]?.blocks ?? [];

    // Empty dt has no content, so it's filtered out by parseListItems.
    // Only the dd content becomes a list item.
    expect(blocks).toHaveLength(1);
    expect(blocks[0]?.type).toBe("list");
    if (blocks[0]?.type !== "list") return;
    expect(blocks[0].items).toHaveLength(1);
  });
});

// ── Bd-indent tests ────────────────────────────────────────

describe("parseMandoc - Bd-indent blocks", () => {
  test("adds 4-column indent to Bd-indent children", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="DESC">DESCRIPTION</h1>
          <p class="Pp">normal paragraph</p>
          <div class="Bd-indent">
            <p class="Pp">indented paragraph</p>
          </div>
        </section>
      </div>
    </body>`;

    const sections = parseMandoc(html);
    const blocks = sections[0]?.blocks ?? [];

    expect(blocks).toHaveLength(2);
    expect(blocks[0]?.type === "paragraph" && blocks[0].indent).toBe(0);
    expect(blocks[1]?.type === "paragraph" && blocks[1].indent).toBe(4);
  });

  test("nests Bd-indent accumulatively", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="DESC">DESCRIPTION</h1>
          <div class="Bd-indent">
            <div class="Bd-indent">
              <p class="Pp">double indented</p>
            </div>
          </div>
        </section>
      </div>
    </body>`;

    const sections = parseMandoc(html);
    const blocks = sections[0]?.blocks ?? [];

    expect(blocks).toHaveLength(1);
    expect(blocks[0]?.type === "paragraph" && blocks[0].indent).toBe(8);
  });
});

// ── Inline formatting tests ────────────────────────────────

describe("parseMandoc - inline formatting", () => {
  test("preserves bold and italic in paragraphs", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="SYN">SYNOPSIS</h1>
          <p class="Pp"><b>ls</b> [<i>OPTION</i>]... [<i>FILE</i>]...</p>
        </section>
      </div>
    </body>`;

    const sections = parseMandoc(html);
    const block = sections[0]?.blocks?.[0];

    expect(block?.type).toBe("paragraph");
    if (block?.type !== "paragraph") return;

    const hasBold = block.children.some(
      (n) => n.type === "bold" && n.children.some((c) => c.type === "text" && c.content.includes("ls"))
    );
    const hasItalic = block.children.some(
      (n) => n.type === "italic" && n.children.some((c) => c.type === "text" && c.content.includes("OPTION"))
    );

    expect(hasBold).toBe(true);
    expect(hasItalic).toBe(true);
  });

  test("preserves br as break nodes", () => {
    const html = `<body>
      <div class="manual-text">
        <section class="Sh">
          <h1 class="Sh" id="COPY">COPYRIGHT</h1>
          <p class="Pp">Copyright 2023. <br/> This is free software.</p>
        </section>
      </div>
    </body>`;

    const sections = parseMandoc(html);
    const block = sections[0]?.blocks?.[0];

    expect(block?.type).toBe("paragraph");
    if (block?.type !== "paragraph") return;

    const breakCount = block.children.filter((n) => n.type === "break").length;
    expect(breakCount).toBe(1);
  });
});

// ── Fixture-based tests ────────────────────────────────────

describe("parseMandoc - fixtures", () => {
  test("parses mandoc-ls.html sections and hierarchy", () => {
    const html = loadManPageFixture("mandoc-ls");
    const sections = parseMandoc(html);

    const titles = sections.map((s) => s.title);
    expect(titles).toEqual([
      "NAME",
      "SYNOPSIS",
      "DESCRIPTION",
      "AUTHOR",
      "COPYRIGHT",
      "SEE ALSO",
    ]);

    // DESCRIPTION has Exit status subsection
    const desc = sections.find((s) => s.title === "DESCRIPTION");
    expect(desc?.children.map((c) => c.title)).toContain("Exit status:");
    expect(desc?.children[0]?.level).toBe(3);
  });

  test("parses mandoc-ls.html DESCRIPTION definition lists", () => {
    const html = loadManPageFixture("mandoc-ls");
    const sections = parseMandoc(html);

    const desc = sections.find((s) => s.title === "DESCRIPTION");
    const blocks = desc?.blocks ?? [];

    // Should have paragraphs and at least one list
    const hasList = blocks.some((b) => b.type === "list");
    expect(hasList).toBe(true);

    // The list should contain option flags (bold text starting with -)
    const list = blocks.find((b) => b.type === "list");
    if (list?.type === "list") {
      const firstItem = list.items[0];
      const hasBoldFlag = firstItem?.some(
        (n) => n.type === "bold" && n.children.some((c) => c.type === "text" && c.content.startsWith("-"))
      );
      expect(hasBoldFlag).toBe(true);
    }
  });

  test("parses mandoc-ls.html Exit status subsection with definition list", () => {
    const html = loadManPageFixture("mandoc-ls");
    const sections = parseMandoc(html);

    const desc = sections.find((s) => s.title === "DESCRIPTION");
    const exitStatus = desc?.children.find((c) => c.title.includes("Exit"));

    expect(exitStatus).toBeDefined();
    expect(exitStatus?.blocks.length).toBeGreaterThan(0);

    // Exit status has a Bl-tag list with 3 dt + 3 dd = 6 items
    const list = exitStatus?.blocks.find((b) => b.type === "list");
    expect(list).toBeDefined();
    if (list?.type === "list") {
      expect(list.items.length).toBe(6);
    }
  });

  test("skips head table in mandoc output", () => {
    const html = loadManPageFixture("mandoc-ls");
    const sections = parseMandoc(html);

    // The head table (LS(1), User Commands, LS(1)) should not create sections
    const hasHeadSection = sections.some(
      (s) => s.title.includes("LS(1)") || s.title.includes("User Commands")
    );
    expect(hasHeadSection).toBe(false);
  });
});

// ── Inline content grouping in Bd-indent ───────────────
//
// Regression: mandoc Bd-indent divs interleave bare text with inline
// <b>/<i> elements. parseMandocChildren must merge consecutive inline
// content into one paragraph instead of emitting a block per fragment
// (which scattered a single sentence across many lines).

describe("parseMandoc - inline content grouping", () => {
  test("merges bare text + inline <b> into one paragraph", () => {
    const html = `<body><div class="manual-text">
      <section class="Sh">
        <h1 class="Sh" id="OPTIONS">OPTIONS</h1>
        <p class="Pp">-h, --help</p>
        <div class="Bd-indent">Prints the synopsis. If the option <b>--all</b> or <b>-a</b> is given then all available commands are printed.</div>
      </section>
    </div></body>`;

    const sections = parseMandoc(html);
    const options = sections[0]!;

    // The Bd-indent content must be ONE paragraph, not five.
    const descBlocks = options.blocks.filter(
      (b) =>
        b.type === "paragraph" &&
        b.children.some(
          (n) => n.type === "text" && n.content.includes("Prints the synopsis"),
        ),
    );
    expect(descBlocks.length).toBe(1);

    const block = descBlocks[0]!;
    if (block.type === "paragraph") {
      // Must contain both surrounding text and the inline bold flags.
      const hasBoldAll = block.children.some(
        (n) =>
          n.type === "bold" &&
          n.children.some((c) => c.type === "text" && c.content === "--all"),
      );
      const hasBoldA = block.children.some(
        (n) =>
          n.type === "bold" &&
          n.children.some((c) => c.type === "text" && c.content === "-a"),
      );
      expect(hasBoldAll).toBe(true);
      expect(hasBoldA).toBe(true);
    }
  });

  test("bare inline fragment never becomes a standalone block", () => {
    const html = `<body><div class="manual-text">
      <section class="Sh">
        <h1 class="Sh" id="OPTIONS">OPTIONS</h1>
        <div class="Bd-indent">Use <b>--all</b> or <b>-a</b> here.</div>
      </section>
    </div></body>`;

    const sections = parseMandoc(html);
    const options = sections[0]!;

    const standalone = options.blocks.filter((b) => {
      if (b.type !== "paragraph") return false;
      const flat = b.children
        .map((n) =>
          n.type === "text"
            ? n.content
            : n.type === "bold"
              ? n.children.map((c) => (c.type === "text" ? c.content : "")).join("")
              : "",
        )
        .join("")
        .trim();
      return flat === "--all" || flat === "or" || flat === "-a";
    });
    expect(standalone).toEqual([]);
  });

  test("block-level <p> inside Bd-indent still splits the paragraph", () => {
    // Inline grouping must NOT swallow block-level children: the leading
    // bare text and the following <p> should be two separate paragraphs.
    const html = `<body><div class="manual-text">
      <section class="Sh">
        <h1 class="Sh" id="OPTIONS">OPTIONS</h1>
        <div class="Bd-indent">Leading text with <b>flag</b>.
          <p class="Pp">Second paragraph.</p>
        </div>
      </section>
    </div></body>`;

    const sections = parseMandoc(html);
    const options = sections[0]!;
    const paragraphs = options.blocks.filter((b) => b.type === "paragraph");
    expect(paragraphs.length).toBe(2);

    const first = paragraphs[0]!;
    const second = paragraphs[1]!;
    if (first.type === "paragraph") {
      expect(
        first.children.some(
          (n) => n.type === "text" && n.content.includes("Leading text"),
        ),
      ).toBe(true);
    }
    if (second.type === "paragraph") {
      expect(
        second.children.some(
          (n) => n.type === "text" && n.content.includes("Second paragraph"),
        ),
      ).toBe(true);
    }
  });

  test("whitespace-only inline content does not create empty paragraphs", () => {
    const html = `<body><div class="manual-text">
      <section class="Sh">
        <h1 class="Sh" id="OPTIONS">OPTIONS</h1>
        <p class="Pp">First.</p>

        <p class="Pp">Second.</p>
      </section>
    </div></body>`;

    const sections = parseMandoc(html);
    const options = sections[0]!;
    // Only the two real <p> blocks; the whitespace between them must not
    // produce a space-only paragraph.
    const paragraphs = options.blocks.filter((b) => b.type === "paragraph");
    expect(paragraphs.length).toBe(2);
  });
});
