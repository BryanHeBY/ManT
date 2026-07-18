import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { App } from "../../../src/ui/app";
import { mockLsResult } from "../../fixtures/mock-result";
import { loadManPageFixture } from "../../fixtures/man-pages";
import { parseManHtml } from "../../../src/core/parser";
import type { QueryResult } from "../../../src/query";

const NAV_WIDTH = 32;

function navLines(frame: string): string[] {
  return frame.split("\n").map((line) => line.slice(0, NAV_WIDTH));
}

function navPosition(frame: string, label: string): { x: number; y: number } {
  const lines = navLines(frame);
  const y = lines.findIndex((line) => line.includes(label));
  if (y < 0) throw new Error(`Navigation item not found: ${label}`);
  return { x: lines[y]!.indexOf(label), y };
}

async function flushKeyboard(setup: { flush: () => Promise<void> }): Promise<void> {
  // Keyboard events are delivered outside React's synthetic event queue. Give
  // React one turn to schedule the state update before capturing the frame.
  await new Promise<void>((resolve) => setTimeout(resolve, 10));
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
    expect(navLines(frame).some((line) => line.includes("Options...Kind"))).toBe(true);

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
