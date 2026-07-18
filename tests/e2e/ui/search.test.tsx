/**
 * @file Verifies explicit-confirmation in-page search, match highlighting, and
 * exact scrolling to the matching block rather than merely its section.
 */

import { describe, expect, test } from "bun:test";
import type { QueryResult } from "../../../src/query";
import { mockLsResult } from "../../fixtures/mock-result";
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
        .some((span) => span.bg.toInts().slice(0, 3).join(",") === "249,226,175"),
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
    expect(frame).toContain("Find “directory” · 1 matches");

    setup.renderer.destroy();
  });

  test("scrolls confirmed body matches to their paragraph", async () => {
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

  test("opens the bottom search input with Ctrl+F", async () => {
    const setup = await renderApp(mockLsResult);

    setup.mockInput.pressKey("f", { ctrl: true });
    await flushKeyboard(setup);

    expect(setup.captureCharFrame()).toContain("Find:");
    setup.renderer.destroy();
  });
});
