import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { App } from "../../../src/ui/app";
import { mockLsResult } from "../../fixtures/mock-result";
import { loadManPageFixture } from "../../fixtures/man-pages";
import { parseManHtml } from "../../../src/core/parser";
import type { QueryResult } from "../../../src/query";

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

    expect(frame).toContain("MAN: ls");
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

    // Click on SYNOPSIS in the sidebar.
    // New layout (no borders, padding=1): header at row 0, gap, NAME at row 2, SYNOPSIS at row 3.
    await setup.mockMouse.click(1, 3);
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

    expect(frame).toContain("MAN: gcc");
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
    expect(frame).toContain("Options Controlling");

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
    await setup.flush();

    const frame = setup.captureCharFrame();
    expect(frame).toContain("SYNOPSIS");

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

    expect(frame).toContain("MAN: smoke");
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
    expect(frame).toContain("MAN: ls");

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
    expect(frame).toContain("  CHILD");

    // Click PARENT again to collapse it.
    // New layout (no borders, padding=1, gap=1): padding at row 0, header at row 1, gap at row 2, PARENT at row 3.
    await setup.mockMouse.click(1, 3);
    await setup.flush();

    frame = setup.captureCharFrame();
    expect(frame).toContain("▸ PARENT");
    // Assert only the sidebar (first 30 columns) no longer shows the child.
    let sidebarLines = frame.split("\n").map((line) => line.slice(0, 30));
    expect(sidebarLines.some((line) => line.includes("  CHILD"))).toBe(false);

    // Click PARENT again to expand it.
    await setup.mockMouse.click(1, 3);
    await setup.flush();

    frame = setup.captureCharFrame();
    expect(frame).toContain("▾ PARENT");
    sidebarLines = frame.split("\n").map((line) => line.slice(0, 30));
    expect(sidebarLines.some((line) => line.includes("  CHILD"))).toBe(true);

    setup.renderer.destroy();
  });

  test("collapses sections with h key", async () => {
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
    expect(frame).toContain("  CHILD");

    // Collapse with h.
    setup.mockInput.pressKey("h");
    await setup.flush();

    frame = setup.captureCharFrame();
    expect(frame).toContain("▸ PARENT");
    const sidebarLinesCollapsed = frame.split("\n").map((line) => line.slice(0, 28));
    expect(sidebarLinesCollapsed.some((line) => line.includes("  CHILD"))).toBe(false);

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
