/**
 * @file Verifies explicit-confirmation in-page search, match highlighting, and
 * exact scrolling to the matching block rather than merely its section.
 */

import { describe, expect, test } from "bun:test";
import { mockLsResult, mockQuery } from "../../fixtures/mock-result";
import {
  contentPosition,
  flushEscape,
  flushKeyboard,
  installOpenTuiWarningFilter,
  NAV_WIDTH,
  navLines,
  renderApp,
  waitForFrame,
} from "./test-support";

installOpenTuiWarningFilter();

describe("App search (e2e)", () => {
  test("searches only after the bottom input is confirmed", async () => {
    const setup = await renderApp(mockLsResult);

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
        .some((span) => {
          const background = span.bg.toInts().slice(0, 3).join(",");
          return background === "249,226,175" || background === "69,71,90";
        }),
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
        (span) => span.bg.toInts().slice(0, 3).join(",") === "249,226,175",
      ),
    ).toBe(true);

    setup.mockInput.pressEscape();
    await flushEscape(setup);
    frame = setup.captureCharFrame();
    expect(frame).not.toContain("Find: directory");
    expect(frame).not.toContain("Find “directory” · 1 matches");
    expect(
      setup.captureSpans().lines
        .flatMap((line) => line.spans)
        .filter((span) => span.text.toLocaleLowerCase() === "directory")
        .some((span) => {
          const background = span.bg.toInts().slice(0, 3).join(",");
          return background === "249,226,175" || background === "69,71,90";
        }),
    ).toBe(false);

    setup.renderer.destroy();
  });

  test("shows a prominent no-match result until the query is edited", async () => {
    const setup = await renderApp(mockLsResult);

    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    setup.mockInput.typeText("definitely-not-in-this-manual");
    await flushKeyboard(setup);
    setup.mockInput.pressEnter();
    await flushKeyboard(setup);

    let frame = setup.captureCharFrame();
    expect(frame).toContain("No matches");
    expect(frame).toContain("Edit query · Esc close");
    const noMatchSpan = setup.captureSpans().lines
      .flatMap((line) => line.spans)
      .find((span) => span.text === "No matches");
    expect(noMatchSpan?.bg.toInts().slice(0, 3).join(",")).toBe("243,139,168");

    setup.mockInput.typeText("x");
    await flushKeyboard(setup);
    frame = setup.captureCharFrame();
    expect(frame).not.toContain("No matches");
    expect(frame).toContain("Enter search · Esc cancel");

    setup.renderer.destroy();
  });

  test("shows every result while distinguishing the active result", async () => {
    const result = mockQuery("layered-search", [{
      id: "description",
      title: "DESCRIPTION",
      blocks: [{
        type: "paragraph",
        children: [{ type: "text", value: "Needle first, then needle second." }],
      }],
      children: [],
    }]);
    const setup = await renderApp(result);
    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    setup.mockInput.typeText("needle");
    await flushKeyboard(setup);
    setup.mockInput.pressEnter();
    await flushKeyboard(setup);

    const resultBackgrounds = () => setup.captureSpans().lines
      .flatMap((line) => line.spans)
      .filter((span) => span.text.toLocaleLowerCase() === "needle")
      .map((span) => span.bg.toInts().slice(0, 3).join(","))
      // The open search field also contains the query; only the two content
      // decoration colours belong to search results.
      .filter((background) => background === "249,226,175" || background === "69,71,90");

    expect(resultBackgrounds()).toEqual(["249,226,175", "69,71,90"]);

    setup.mockInput.pressEnter();
    await flushKeyboard(setup);
    expect(resultBackgrounds()).toEqual(["69,71,90", "249,226,175"]);
    setup.renderer.destroy();
  });

  test("scrolls confirmed body matches to their paragraph", async () => {
    const result = mockQuery("anchored-search", [
        {
          id: "section-0",
          title: "INTRODUCTION",
          blocks: Array.from({ length: 24 }, (_, index) => ({
            type: "paragraph" as const,
            children: [{ type: "text" as const, value: `Filler line ${index}` }],
          })),
          children: [],
        },
        {
          id: "section-1",
          title: "TARGET SECTION",
          blocks: [
            {
              type: "paragraph",
              children: [{ type: "text", value: "Context before the result." }],
            },
            {
              type: "paragraph",
              children: [{ type: "text", value: "Needle result is here." }],
            },
          ],
          children: [],
        },
      ]);
    const setup = await renderApp(result);
    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    setup.mockInput.typeText("needle");
    await flushKeyboard(setup);
    setup.mockInput.pressEnter();
    await flushKeyboard(setup);

    const frame = await waitForFrame(
      setup,
      (candidate) => candidate.includes("Needle result is here."),
    );
    expect(contentPosition(frame, "Needle result is here.").y).toBe(2);
    expect(
      frame.split("\n").some((line) => line.slice(NAV_WIDTH).includes("TARGET SECTION")),
    ).toBe(false);

    setup.renderer.destroy();
  });

  test("scrolls to the wrapped row containing a late paragraph match", async () => {
    const result = mockQuery("wrapped-search", [{
      id: "description",
      title: "DESCRIPTION",
      blocks: [{
        type: "paragraph",
        children: [{
          type: "text",
          value: `${"Earlier prose fills this wrapped paragraph. ".repeat(28)}Late needle is here.`,
        }],
      }],
      children: [],
    }]);
    const setup = await renderApp(result, { width: 100, height: 24 });
    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    setup.mockInput.typeText("late needle");
    await flushKeyboard(setup);
    setup.mockInput.pressEnter();
    await flushKeyboard(setup);

    const frame = await waitForFrame(
      setup,
      (candidate) => candidate.includes("Late needle is here."),
    );
    expect(contentPosition(frame, "Late needle is here.").y).toBe(2);
    setup.renderer.destroy();
  });

  test("targets separate matches deep inside one large definition list", async () => {
    const result = mockQuery("large-options", [{
      id: "options",
      title: "OPTIONS",
      blocks: [{
        type: "definition-list",
        compact: true,
        items: Array.from({ length: 40 }, (_, index) => ({
          terms: [[{
            type: "text" as const,
            value: index === 18
              ? "--needle-first"
              : index === 34 ? "--needle-second" : `--option-${index}`,
          }]],
          description: [{
            type: "paragraph" as const,
            children: [{ type: "text" as const, value: `Description ${index}.` }],
          }],
        })),
      }],
      children: [],
    }]);
    const setup = await renderApp(result, { width: 100, height: 24 });
    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    setup.mockInput.typeText("needle");
    await flushKeyboard(setup);
    setup.mockInput.pressEnter();
    await flushKeyboard(setup);

    let frame = await waitForFrame(setup, (candidate) => candidate.includes("--needle-first"));
    expect(frame).toContain("1/2");
    expect(contentPosition(frame, "--needle-first").y).toBe(2);

    setup.mockInput.pressEnter();
    await flushKeyboard(setup);
    frame = await waitForFrame(setup, (candidate) => candidate.includes("--needle-second"));
    expect(frame).toContain("2/2");
    expect(contentPosition(frame, "--needle-second").y).toBe(2);
    setup.renderer.destroy();
  });

  test("highlights a match that crosses an inline formatting boundary", async () => {
    const result = mockQuery("inline-search", [{
      id: "description",
      title: "DESCRIPTION",
      blocks: [{
        type: "paragraph",
        children: [
          { type: "text", value: "A result crosses " },
          { type: "strong", children: [{ type: "text", value: "formatting" }] },
          { type: "text", value: " safely." },
        ],
      }],
      children: [],
    }]);
    const setup = await renderApp(result);
    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    setup.mockInput.typeText("crosses formatting");
    await flushKeyboard(setup);
    setup.mockInput.pressEnter();
    await flushKeyboard(setup);

    const highlighted = setup.captureSpans().lines
      .flatMap((line) => line.spans)
      .filter((span) => span.bg.toInts().slice(0, 3).join(",") === "249,226,175")
      .map((span) => span.text)
      .join("");
    expect(highlighted).toBe("crosses formatting");
    setup.renderer.destroy();
  });

  test("highlights the exact GCC diagnostic text inside a multiline code block", async () => {
    const result = mockQuery("gcc", [{
      id: "diagnostic-message-formatting-options",
      title: "Diagnostic Message Formatting Options",
      blocks: [{
        type: "preformatted",
        children: [{
          type: "text",
          value: [
            "        demo.c: In function `test_bad_format_string_args':",
            "        ../../src/demo.c:25:18: warning: format `%i' expects argument of type `int', but argument 2 has type `const char *' [-Wformat=]",
            "           25 |   printf(\"hello %i\", msg);",
            "              |                 ~^   ~~~",
            "              |                  |   |",
            "              |                  int const char *",
            "              |                 %s",
          ].join("\n"),
        }],
      }],
      children: [],
    }]);
    const setup = await renderApp(result, { width: 140, height: 24 });
    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    setup.mockInput.typeText("hello");
    await flushKeyboard(setup);
    setup.mockInput.pressEnter();
    await flushKeyboard(setup);

    const highlighted = setup.captureSpans().lines
      .flatMap((line) => line.spans)
      .filter((span) => span.bg.toInts().slice(0, 3).join(",") === "249,226,175")
      .map((span) => span.text)
      .join("");
    expect(highlighted).toBe("hello");
    setup.renderer.destroy();
  });

  test("opens the bottom search input with Ctrl+F", async () => {
    const setup = await renderApp(mockLsResult);

    setup.mockInput.pressKey("f", { ctrl: true });
    await flushKeyboard(setup);

    expect(setup.captureCharFrame()).toContain("Find:");
    setup.renderer.destroy();
  });
});
