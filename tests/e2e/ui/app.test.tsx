import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { App } from "../../../src/ui/app";
import { mockLsResult, mockLsWithTldrResult } from "../../fixtures/mock-result";
import { loadManPageFixture } from "../../fixtures/man-pages";
import { parseManHtml } from "../../../src/core/parser";
import type { QueryResult } from "../../../src/query";

const NAV_WIDTH = 32;

function navLines(frame: string): string[] {
  return frame.split("\n").map((line) => line.slice(0, NAV_WIDTH));
}

function navigationSpans(lines: ReturnType<Awaited<ReturnType<typeof testRender>>["captureSpans"]>["lines"]) {
  // Exclude the top menu and the bottom status/search rows, which also start
  // at column zero but are not part of the sidebar.
  return lines.slice(1, -2).flatMap((line) => {
    let column = 0;
    return line.spans.filter((span) => {
      const startsInNavigation = column < NAV_WIDTH;
      column += span.text.length;
      return startsInNavigation;
    });
  });
}

function navPosition(frame: string, label: string): { x: number; y: number } {
  const lines = navLines(frame);
  const y = lines.findIndex((line) => line.includes(label));
  if (y < 0) throw new Error(`Navigation item not found: ${label}`);
  return { x: lines[y]!.indexOf(label), y };
}

function contentPosition(frame: string, label: string): { x: number; y: number } {
  const lines = frame.split("\n");
  for (let y = 0; y < lines.length; y++) {
    const x = lines[y]!.indexOf(label);
    if (x >= NAV_WIDTH) return { x, y };
  }
  throw new Error(`Content heading not found: ${label}`);
}

async function flushKeyboard(setup: { flush: () => Promise<void> }): Promise<void> {
  // Keyboard events are delivered outside React's synthetic event queue. Give
  // React one turn to schedule the state update before capturing the frame.
  await new Promise<void>((resolve) => setTimeout(resolve, 10));
  await setup.flush();
}

async function flushEscape(setup: { flush: () => Promise<void> }): Promise<void> {
  // Escape is an ANSI sequence, so the terminal input parser waits briefly to
  // distinguish it from the start of a longer key sequence.
  await new Promise<void>((resolve) => setTimeout(resolve, 220));
  await setup.flush();
}

// OpenTUI's testRender does not wrap renderer creation in React act(), which
// causes a console warning for every e2e test.  That is framework-level noise
// we cannot fix here; filter it so the test output stays readable.
const originalConsoleError = console.error;
console.error = (...args: unknown[]) => {
  const message = typeof args[0] === "string" ? args[0] : "";
  if (
    message.includes("was not wrapped in act") ||
    message.includes("wrap tests with act")
  ) {
    return;
  }
  originalConsoleError.apply(console, args);
};

function mandocHtmlWithPreInDl(): string {
  return `
    <html>
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
    </html>
  `;
}

describe("App (e2e)", () => {
  test("renders topic and section titles", async () => {
    const setup = await testRender(
      <App result={mockLsResult} onQuit={() => {}} />,
      {
        width: 80,
        height: 24,
      }
    );

    await setup.renderOnce();
    const frame = setup.captureCharFrame();

    expect(frame).toContain("MANUAL");
    expect(frame).toContain("ls");
    expect(frame).toContain("NAME");
    expect(frame).toContain("SYNOPSIS");
    expect(frame).toContain("DESCRIPTION");

    setup.renderer.destroy();
  });

  test("renders full manual content, not just selected section", async () => {
    const setup = await testRender(
      <App result={mockLsResult} onQuit={() => {}} />,
      {
        width: 80,
        height: 24,
      }
    );

    await setup.renderOnce();
    const frame = setup.captureCharFrame();

    // All sections should be visible in the content pane.
    expect(frame).toContain("list directory contents");
    expect(frame).toContain("[OPTION]");
    expect(frame).toContain("List information about files.");

    setup.renderer.destroy();
  });

  test("selects section on mouse click", async () => {
    const setup = await testRender(
      <App result={mockLsResult} onQuit={() => {}} />,
      {
        width: 80,
        height: 24,
      }
    );

    await setup.renderOnce();

    // Click the entire SYNOPSIS row, not just its label.
    const synopsis = navPosition(setup.captureCharFrame(), "SYNOPSIS");
    await setup.mockMouse.click(NAV_WIDTH - 3, synopsis.y);
    await setup.flush();

    const frame = setup.captureCharFrame();
    expect(frame).toContain("SYNOPSIS");
    expect(frame).toContain("ls");
    expect(frame).toContain("[OPTION]");

    setup.renderer.destroy();
  });

  test("places a clicked section heading at the top of the content viewport", async () => {
    const result: QueryResult = {
      topic: "scrolling",
      sections: [
        {
          id: "section-0",
          title: "INTRODUCTION",
          level: 2,
          blocks: Array.from({ length: 28 }, (_, index) => ({
            type: "paragraph" as const,
            children: [{ type: "text" as const, content: `Intro line ${index}` }],
            indent: 0,
          })),
          children: [],
        },
        {
          id: "section-1",
          title: "LATE SECTION",
          level: 2,
          blocks: [
            {
              type: "paragraph",
              children: [{ type: "text", content: "Late section body" }],
              indent: 0,
            },
          ],
          children: [],
        },
      ],
    };
    const setup = await testRender(<App result={result} onQuit={() => {}} />, {
      width: 80,
      height: 24,
    });

    await setup.renderOnce();
    const lateSection = navPosition(setup.captureCharFrame(), "LATE SECTION");
    await setup.mockMouse.click(NAV_WIDTH - 3, lateSection.y);
    await setup.flush();

    // Row 2 is the first row in the padded content scroll viewport. The
    // trailing scroll space lets even the final section land here.
    expect(contentPosition(setup.captureCharFrame(), "LATE SECTION").y).toBe(2);

    setup.renderer.destroy();
  });

  test("updates navigation after the content scroll becomes idle", async () => {
    const result: QueryResult = {
      topic: "scroll-spy",
      sections: [
        {
          id: "section-0",
          title: "INTRODUCTION",
          level: 2,
          blocks: Array.from({ length: 28 }, (_, index) => ({
            type: "paragraph" as const,
            children: [{ type: "text" as const, content: `Intro line ${index}` }],
            indent: 0,
          })),
          children: [],
        },
        {
          id: "section-1",
          title: "CURRENT SECTION",
          level: 2,
          blocks: Array.from({ length: 28 }, (_, index) => ({
            type: "paragraph" as const,
            children: [{ type: "text" as const, content: `Current line ${index}` }],
            indent: 0,
          })),
          children: [],
        },
      ],
    };
    const setup = await testRender(<App result={result} onQuit={() => {}} />, {
      width: 80,
      height: 24,
    });

    await setup.renderOnce();
    for (let index = 0; index < 4; index++) {
      setup.mockInput.pressKey("d");
      await flushKeyboard(setup);
    }

    // No sidebar render occurs during an active sequence of page scrolling.
    expect(navLines(setup.captureCharFrame()).some((line) => line.includes("› · INTRODUCTION"))).toBe(true);

    await new Promise<void>((resolve) => setTimeout(resolve, 220));
    await setup.flush();
    expect(
      navLines(setup.captureCharFrame()).some((line) => line.includes("› · CURRENT SECTION")),
    ).toBe(true);

    setup.renderer.destroy();
  });

  test("renders gcc full manual with bold and italic parameters", async () => {
    const result = {
      topic: "gcc",
      sections: parseManHtml(loadManPageFixture("gcc")),
    };

    const setup = await testRender(
      <App result={result} onQuit={() => {}} />,
      {
        width: 100,
        height: 40,
      }
    );

    await setup.renderOnce();
    const frame = setup.captureCharFrame();

    expect(frame).toContain("MANUAL");
    expect(frame).toContain("gcc");
    expect(frame).toContain("NAME");
    expect(frame).toContain("SYNOPSIS");
    expect(frame).toContain("DESCRIPTION");
    expect(frame).toContain("OPTIONS");

    // Bold and italic content from the SYNOPSIS should be rendered.
    expect(frame).toContain("gcc");
    expect(frame).toContain("standard");
    expect(frame).toContain("outfile");

    setup.renderer.destroy();
  });

  test("renders hierarchical subsections", async () => {
    const result = {
      topic: "gcc",
      sections: parseManHtml(loadManPageFixture("gcc")),
    };

    const setup = await testRender(
      <App result={result} onQuit={() => {}} />,
      {
        width: 100,
        height: 40,
      }
    );

    await setup.renderOnce();
    const frame = setup.captureCharFrame();

    // Subsections should appear in the navigation.
    expect(frame).toContain("Option Summary");
    expect(
      navLines(frame).some(
        (line) => line.includes("Options") && line.includes("Kind")
      )
    ).toBe(true);

    setup.renderer.destroy();
  });

  test("quits on q key", async () => {
    let quitCalled = false;
    const setup = await testRender(
      <App result={mockLsResult} onQuit={() => { quitCalled = true; }} />,
      {
        width: 80,
        height: 24,
      }
    );

    await setup.renderOnce();
    setup.mockInput.pressKey("q");
    await setup.flush();

    expect(quitCalled).toBe(true);
    setup.renderer.destroy();
  });

  test("navigates with j/k keys", async () => {
    const setup = await testRender(
      <App result={mockLsResult} onQuit={() => {}} />,
      {
        width: 80,
        height: 24,
      }
    );

    await setup.renderOnce();

    // Move down to SYNOPSIS.
    setup.mockInput.pressKey("j");
    await flushKeyboard(setup);

    let frame = setup.captureCharFrame();
    expect(frame).toContain("› · SYNOPSIS");

    // Move back to NAME.
    setup.mockInput.pressKey("k");
    await flushKeyboard(setup);
    frame = setup.captureCharFrame();
    expect(frame).toContain("› · NAME");

    setup.renderer.destroy();
  });

  test("smoke: renders inline code and pre blocks without crashing", async () => {
    const result: QueryResult = {
      topic: "smoke",
      sections: [
        {
          id: "section-0",
          title: "CODE",
          level: 2,
          blocks: [
            {
              type: "paragraph",
              children: [
                { type: "text", content: "Run " },
                {
                  type: "code",
                  children: [{ type: "text", content: "ls -la" }],
                },
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
        },
      ],
    };

    const setup = await testRender(
      <App result={result} onQuit={() => {}} />,
      {
        width: 80,
        height: 24,
      }
    );

    await setup.renderOnce();
    const frame = setup.captureCharFrame();

    expect(frame).toContain("MANUAL");
    expect(frame).toContain("smoke");
    expect(frame).toContain("CODE");
    expect(frame).toContain("ls -la");
    expect(frame).toContain("int main()");
    expect(frame).toContain("return 0;");

    setup.renderer.destroy();
  });

  test("renders mandoc <pre> inside definition lists as separate code blocks", async () => {
    const result = {
      topic: "gcc",
      sections: parseManHtml(mandocHtmlWithPreInDl()),
    };

    const setup = await testRender(
      <App result={result} onQuit={() => {}} />,
      {
        width: 100,
        height: 40,
      }
    );

    await setup.renderOnce();
    const frame = setup.captureCharFrame();

    // The option term and description should be visible.
    expect(frame).toContain("-fcond-mismatch");
    expect(frame).toContain("Allow conditional expressions");

    // The code example must be visible and preserved as a block.
    expect(frame).toContain("#define abs(n)");
    expect(frame).toContain("__builtin_strcpy");

    // It should not be flattened into a bulleted list item.
    const lines = frame.split("\n");
    const codeLine = lines.find((line) => line.includes("#define abs(n)"));
    expect(codeLine).toBeDefined();
    expect(codeLine?.includes("•")).toBe(false);

    setup.renderer.destroy();
  });

  test("frame is not empty after render", async () => {
    const setup = await testRender(
      <App result={mockLsResult} onQuit={() => {}} />,
      {
        width: 80,
        height: 24,
      }
    );

    await setup.renderOnce();
    const frame = setup.captureCharFrame();

    expect(frame.trim().length).toBeGreaterThan(0);
    expect(frame).toContain("MANUAL");
    expect(frame).toContain("ls");

    setup.renderer.destroy();
  });

  test("exposes the classic menu bar and a compact status bar", async () => {
    const setup = await testRender(
      <App result={mockLsResult} onQuit={() => {}} />,
      { width: 80, height: 24 }
    );

    await setup.renderOnce();
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

  test("shows the tldr quick reference before the man page and in the navigation tree", async () => {
    const setup = await testRender(
      <App result={mockLsWithTldrResult} onQuit={() => {}} />,
      { width: 100, height: 32 }
    );

    await setup.renderOnce();
    let frame = setup.captureCharFrame();
    const tldrPosition = frame.indexOf("TLDR QUICK REFERENCE · ls");
    const manualPosition = frame.indexOf("list directory contents");

    expect(navLines(frame).some((line) => line.includes("◆ TLDR QUICK REFERENCE"))).toBe(true);
    expect(frame).toContain("tldr-pages · CC BY 4.0 · common · en");
    expect(frame).toContain("List files, including hidden entries");
    expect(tldrPosition).toBeGreaterThanOrEqual(0);
    expect(manualPosition).toBeGreaterThan(tldrPosition);
    expect(frame).toContain("› ◆ TLDR QUICK REFERENCE");

    setup.mockInput.pressKey("j");
    await flushKeyboard(setup);
    frame = setup.captureCharFrame();
    expect(frame).toContain("› · NAME");

    setup.renderer.destroy();
  });

  test("keeps a cached tldr page usable when no local man page exists", async () => {
    const setup = await testRender(
      <App result={{ ...mockLsWithTldrResult, sections: [] }} onQuit={() => {}} />,
      { width: 100, height: 28 }
    );

    await setup.renderOnce();
    const frame = setup.captureCharFrame();

    expect(frame).toContain("TLDR QUICK REFERENCE · ls");
    expect(frame).toContain("No local man page was found");
    expect(navLines(frame).some((line) => line.includes("◆ TLDR QUICK REFERENCE"))).toBe(true);

    setup.renderer.destroy();
  });

  test("toggles the sidebar from the View menu", async () => {
    const setup = await testRender(
      <App result={mockLsResult} onQuit={() => {}} />,
      { width: 80, height: 24 }
    );

    await setup.renderOnce();
    await setup.mockMouse.click(7, 0);
    await setup.flush();

    let frame = setup.captureCharFrame();
    expect(frame).toContain("✓ Sidebar");
    expect(frame).toContain("Reset Sidebar Width");

    await setup.mockMouse.click(8, 1);
    await setup.flush();
    frame = setup.captureCharFrame();

    expect(navLines(frame).some((line) => line.includes("MANUAL"))).toBe(false);
    expect(frame).toContain("list directory contents");

    setup.renderer.destroy();
  });

  test("searches the manual from the temporary bottom input", async () => {
    const setup = await testRender(
      <App result={mockLsResult} onQuit={() => {}} />,
      { width: 80, height: 24 }
    );

    await setup.renderOnce();
    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    expect(setup.captureCharFrame()).toContain("Find:");

    setup.mockInput.typeText("directory");
    await flushKeyboard(setup);
    let frame = setup.captureCharFrame();
    expect(frame).toContain("Find: directory");
    expect(frame).toContain("Enter search · Esc cancel");
    expect(frame).not.toContain("1/1");
    expect(navLines(frame).some((line) => line.includes("› · NAME"))).toBe(true);
    expect(
      setup.captureSpans().lines
        .flatMap((line) => line.spans)
        .some((span) => span.bg.toInts().slice(0, 3).join(",") === "249,226,175")
    ).toBe(false);

    setup.mockInput.pressEnter();
    await flushKeyboard(setup);
    frame = setup.captureCharFrame();
    expect(frame).toContain("1/1");
    const highlightedDirectorySpans = setup.captureSpans().lines
      .flatMap((line) => line.spans)
      .filter((span) => span.text.toLocaleLowerCase() === "directory");
    expect(
      highlightedDirectorySpans.some(
        (span) => span.bg.toInts().slice(0, 3).join(",") === "249,226,175"
      )
    ).toBe(true);

    setup.mockInput.pressEscape();
    await flushEscape(setup);
    frame = setup.captureCharFrame();
    expect(frame).not.toContain("Find: directory");
    expect(frame).toContain("Find “directory” · 1 matches");

    setup.renderer.destroy();
  });

  test("scrolls confirmed body matches to their paragraph instead of the section title", async () => {
    const result: QueryResult = {
      topic: "anchored-search",
      sections: [
        {
          id: "section-0",
          title: "INTRODUCTION",
          level: 2,
          blocks: Array.from({ length: 24 }, (_, index) => ({
            type: "paragraph" as const,
            children: [{ type: "text" as const, content: `Filler line ${index}` }],
            indent: 0,
          })),
          children: [],
        },
        {
          id: "section-1",
          title: "TARGET SECTION",
          level: 2,
          blocks: [
            {
              type: "paragraph",
              children: [{ type: "text", content: "Context before the result." }],
              indent: 0,
            },
            {
              type: "paragraph",
              children: [{ type: "text", content: "Needle result is here." }],
              indent: 0,
            },
          ],
          children: [],
        },
      ],
    };
    const setup = await testRender(<App result={result} onQuit={() => {}} />, {
      width: 80,
      height: 24,
    });

    await setup.renderOnce();
    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    setup.mockInput.typeText("needle");
    await flushKeyboard(setup);
    setup.mockInput.pressEnter();
    await flushKeyboard(setup);
    await flushKeyboard(setup);

    const frame = setup.captureCharFrame();
    expect(contentPosition(frame, "Needle result is here.").y).toBe(2);
    expect(
      frame.split("\n").some((line) => line.slice(NAV_WIDTH).includes("TARGET SECTION")),
    ).toBe(false);

    setup.renderer.destroy();
  });

  test("opens the bottom search input with Ctrl+F", async () => {
    const setup = await testRender(
      <App result={mockLsResult} onQuit={() => {}} />,
      { width: 80, height: 24 }
    );

    await setup.renderOnce();
    setup.mockInput.pressKey("f", { ctrl: true });
    await flushKeyboard(setup);

    expect(setup.captureCharFrame()).toContain("Find:");
    setup.renderer.destroy();
  });

  test("shows keyboard help from the Help menu shortcut", async () => {
    const setup = await testRender(
      <App result={mockLsResult} onQuit={() => {}} />,
      { width: 80, height: 24 }
    );

    await setup.renderOnce();
    setup.mockInput.pressKey("?");
    await flushKeyboard(setup);
    expect(setup.captureCharFrame()).toContain("Keyboard Shortcuts");

    setup.mockInput.pressKey("?");
    await flushKeyboard(setup);
    expect(setup.captureCharFrame()).not.toContain("Keyboard Shortcuts");

    setup.renderer.destroy();
  });

  test("resizes the navigation by dragging its boundary", async () => {
    const setup = await testRender(
      <App result={mockLsResult} onQuit={() => {}} />,
      {
        width: 80,
        height: 24,
      }
    );

    await setup.renderOnce();
    let frame = setup.captureCharFrame();
    expect(navLines(frame).some((line) => line.includes("MANUAL"))).toBe(true);
    const contentColumn = (currentFrame: string) =>
      currentFrame
        .split("\n")
        .find((line) => line.includes("list directory contents"))!
        .indexOf("list directory contents");
    const initialContentX = contentColumn(frame);

    // Dragging the colour boundary widens the sidebar and shifts content right.
    await setup.mockMouse.drag(NAV_WIDTH, 2, NAV_WIDTH + 8, 2);
    await setup.flush();

    frame = setup.captureCharFrame();
    expect(navLines(frame).some((line) => line.includes("MANUAL"))).toBe(true);
    expect(contentColumn(frame)).toBeGreaterThan(initialContentX);

    setup.renderer.destroy();
  });

  test("toggles child sections on mouse click", async () => {
    const result: QueryResult = {
      topic: "parent",
      sections: [
        {
          id: "section-0",
          title: "PARENT",
          level: 2,
          blocks: [
            {
              type: "paragraph",
              children: [{ type: "text", content: "Parent content" }],
              indent: 0,
            },
          ],
          children: [
            {
              id: "section-0-0",
              title: "CHILD",
              level: 3,
              blocks: [
                {
                  type: "paragraph",
                  children: [{ type: "text", content: "Child content" }],
                  indent: 0,
                },
              ],
              children: [],
            },
          ],
        },
      ],
    };

    const setup = await testRender(
      <App result={result} onQuit={() => {}} />,
      {
        width: 80,
        height: 24,
      }
    );

    await setup.renderOnce();

    // Initially PARENT is selected and expanded in the sidebar.
    let frame = setup.captureCharFrame();
    expect(frame).toContain("▾ PARENT");
    expect(frame).toContain("╰─· CHILD");

    // Click PARENT again to collapse it.
    let parent = navPosition(frame, "PARENT");
    await setup.mockMouse.click(NAV_WIDTH - 3, parent.y);
    await setup.flush();

    frame = setup.captureCharFrame();
    expect(frame).toContain("▸ PARENT");
    expect(navLines(frame).some((line) => line.includes("CHILD"))).toBe(false);

    // Click PARENT again to expand it.
    parent = navPosition(frame, "PARENT");
    await setup.mockMouse.click(NAV_WIDTH - 3, parent.y);
    await setup.flush();

    frame = setup.captureCharFrame();
    expect(frame).toContain("▾ PARENT");
    expect(navLines(frame).some((line) => line.includes("╰─· CHILD"))).toBe(true);

    setup.renderer.destroy();
  });

  test("navigates the section tree with h/l keys", async () => {
    const result: QueryResult = {
      topic: "parent",
      sections: [
        {
          id: "section-0",
          title: "PARENT",
          level: 2,
          blocks: [
            {
              type: "paragraph",
              children: [{ type: "text", content: "Parent content" }],
              indent: 0,
            },
          ],
          children: [
            {
              id: "section-0-0",
              title: "CHILD",
              level: 3,
              blocks: [
                {
                  type: "paragraph",
                  children: [{ type: "text", content: "Child content" }],
                  indent: 0,
                },
              ],
              children: [],
            },
          ],
        },
      ],
    };

    const setup = await testRender(
      <App result={result} onQuit={() => {}} />,
      {
        width: 80,
        height: 24,
      }
    );

    await setup.renderOnce();

    // PARENT is expanded by default in the sidebar.
    let frame = setup.captureCharFrame();
    expect(frame).toContain("▾ PARENT");
    expect(frame).toContain("╰─· CHILD");

    // Collapse with h.
    setup.mockInput.pressKey("h");
    await setup.flush();

    frame = setup.captureCharFrame();
    expect(frame).toContain("▸ PARENT");
    expect(navLines(frame).some((line) => line.includes("CHILD"))).toBe(false);

    // Right opens a collapsed branch, then moves into its first child.
    setup.mockInput.pressArrow("right");
    await flushKeyboard(setup);
    expect(navLines(setup.captureCharFrame()).some((line) => line.includes("╰─· CHILD"))).toBe(true);

    setup.mockInput.pressArrow("right");
    await flushKeyboard(setup);
    expect(setup.captureCharFrame()).toContain("› ╰─· CHILD");

    // Left on a leaf returns to its parent.
    setup.mockInput.pressArrow("left");
    await flushKeyboard(setup);
    expect(setup.captureCharFrame()).toContain("› ▾ PARENT");

    setup.renderer.destroy();
  });

  test("expands a selected long navigation title across multiple rows", async () => {
    const result: QueryResult = {
      topic: "long-title",
      sections: [
        {
          id: "section-0",
          title: "PARENT",
          level: 2,
          blocks: [],
          children: [
            {
              id: "section-0-0",
              title: "FIRSTMARKERABCDEFGHI SECONDMARKERABCDEFGH THIRDMARKERABCDEFGHI",
              level: 3,
              blocks: [],
              children: [],
            },
          ],
        },
        {
          id: "section-1",
          title: "SIBLING",
          level: 2,
          blocks: [],
          children: [],
        },
      ],
    };
    const setup = await testRender(<App result={result} onQuit={() => {}} />, {
      width: 80,
      height: 24,
    });

    await setup.renderOnce();
    setup.mockInput.pressKey("j");
    await flushKeyboard(setup);

    const lines = navLines(setup.captureCharFrame());
    expect(lines.some((line) => line.includes("FIRSTMARKERABCDEFGHI"))).toBe(true);
    expect(lines.some((line) => line.includes("SECONDMARKERABCDEFGH"))).toBe(true);
    expect(lines.some((line) => line.includes("THIRDMARKERABCDEFGHI"))).toBe(true);
    expect(lines.some((line) => line.includes("│") && line.includes("SECONDMARKER"))).toBe(true);

    // Text nodes in the navigation must not enter OpenTUI's native selection
    // mode when a user drags over a wrapped title. That selection paints only
    // text fragments and looks like a broken search highlight.
    const firstTitle = navPosition(setup.captureCharFrame(), "FIRSTMARKERABCDEFGHI");
    await setup.mockMouse.drag(firstTitle.x, firstTitle.y, firstTitle.x + 8, firstTitle.y);
    await setup.flush();
    const draggedTitleSpans = navigationSpans(setup.captureSpans().lines).filter((span) =>
      span.text.includes("FIRSTMARKER"),
    );
    expect(
      draggedTitleSpans.every(
        (span) => span.bg.toInts().slice(0, 3).join(",") === "49,50,68",
      ),
    ).toBe(true);

    // Search highlighting belongs to the manual content, not navigation
    // labels. The selected item's uniform background keeps wrapped titles and
    // tree connectors visually continuous.
    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    setup.mockInput.typeText("firstmarker");
    await flushKeyboard(setup);
    const titleSpans = navigationSpans(setup.captureSpans().lines).filter((span) =>
      span.text.includes("FIRSTMARKER"),
    );
    expect(titleSpans.length).toBeGreaterThan(0);
    expect(
      titleSpans.every(
        (span) => span.bg.toInts().slice(0, 3).join(",") === "49,50,68",
      ),
    ).toBe(true);

    setup.renderer.destroy();
  });

  test("uses only the item background for a searched GCC navigation title", async () => {
    const result: QueryResult = {
      topic: "gcc",
      sections: parseManHtml(loadManPageFixture("gcc")),
    };
    const setup = await testRender(<App result={result} onQuit={() => {}} />, {
      width: 80,
      height: 24,
    });

    await setup.renderOnce();
    for (let index = 0; index < 5; index++) {
      setup.mockInput.pressKey("j");
      await flushKeyboard(setup);
    }
    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    setup.mockInput.typeText("kind of output");
    await flushKeyboard(setup);

    const titleSpans = navigationSpans(setup.captureSpans().lines).filter((span) =>
      /Options Controlling|Kind of Output/.test(span.text),
    );
    expect(titleSpans.length).toBeGreaterThan(0);
    expect(
      titleSpans.every(
        (span) => span.bg.toInts().slice(0, 3).join(",") === "49,50,68",
      ),
    ).toBe(true);

    setup.renderer.destroy();
  });

  test("SYNOPSIS pre block is indented to the section body level", async () => {
    // Regression: `paddingLeft` on a <text> element has no visual effect in
    // this OpenTUI version, so the Pre component must apply indent on its
    // wrapping <box>. Previously the SYNOPSIS pre rendered flush at column 0
    // instead of at the section-body indent.
    const result = {
      topic: "git",
      sections: parseManHtml(loadManPageFixture("mandoc-git")),
    };

    const setup = await testRender(<App result={result} onQuit={() => {}} />, {
      width: 100,
      height: 40,
    });
    await setup.renderOnce();
    const frame = setup.captureCharFrame();

    const colOf = (needle: string): number => {
      for (const line of frame.split("\n")) {
        const idx = line.indexOf(needle);
        if (idx >= 0) return idx;
      }
      return -1;
    };

    // The SYNOPSIS pre first line and the DESCRIPTION body first line must
    // share the same left indent (both at the section-body level).
    const synopsisPreCol = colOf("git [-v | --version]");
    const descBodyCol = colOf("Git is a fast");

    expect(synopsisPreCol).toBeGreaterThan(0);
    expect(descBodyCol).toBeGreaterThan(0);
    expect(synopsisPreCol).toBe(descBodyCol);

    setup.renderer.destroy();
  });

  test("keeps a blank row between a pre block and the following option", async () => {
    const result: QueryResult = {
      topic: "spacing",
      sections: [
        {
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
        },
      ],
    };

    const setup = await testRender(<App result={result} onQuit={() => {}} />, {
      width: 80,
      height: 24,
    });
    await setup.renderOnce();
    const lines = setup.captureCharFrame().split("\n");
    const lastCodeLine = lines.findIndex((line) => line.includes("command two"));

    expect(lastCodeLine).toBeGreaterThanOrEqual(0);
    expect(lines[lastCodeLine + 1]).not.toContain("-c <name>=<value>");
    expect(lines[lastCodeLine + 2]).toContain("-c <name>=<value>");

    setup.renderer.destroy();
  });
});
