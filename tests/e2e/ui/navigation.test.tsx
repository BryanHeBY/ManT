/**
 * @file Exercises sidebar selection, tree expansion, scrolling, menus, and
 * keyboard navigation from the perspective of a terminal user.
 */

import { describe, expect, test } from "bun:test";
import type { MantQueryBundle } from "../../../src/native";
import {
  mockLsResult,
  mockLsWithTldrResult,
  mockQuery,
} from "../../fixtures/mock-result";
import {
  contentPosition,
  flushKeyboard,
  installOpenTuiWarningFilter,
  NAV_WIDTH,
  navigationSpans,
  navLines,
  navPosition,
  renderApp,
  waitForFrame,
} from "./test-support";

installOpenTuiWarningFilter();

function parentTreeResult(): MantQueryBundle {
  return mockQuery("parent", [{
      id: "section-0",
      title: "PARENT",
      blocks: [{
        type: "paragraph",
        children: [{ type: "text", value: "Parent content" }],
      }],
      children: [{
        id: "section-0-0",
        title: "CHILD",
        blocks: [{
          type: "paragraph",
          children: [{ type: "text", value: "Child content" }],
        }],
        children: [],
      }],
    }]);
}

describe("App navigation (e2e)", () => {
  test("selects a section on mouse click", async () => {
    const setup = await renderApp(mockLsResult);
    const synopsis = navPosition(setup.captureCharFrame(), "SYNOPSIS");

    // Click the entire row, not merely its label.
    await setup.mockMouse.click(NAV_WIDTH - 3, synopsis.y);
    await setup.flush();

    const frame = setup.captureCharFrame();
    expect(frame).toContain("SYNOPSIS");
    expect(frame).toContain("ls");
    expect(frame).toContain("[OPTION]");

    setup.renderer.destroy();
  });

  test("places a clicked section heading at the top of the content viewport", async () => {
    const result = mockQuery("scrolling", [
        {
          id: "section-0",
          title: "INTRODUCTION",
          blocks: Array.from({ length: 28 }, (_, index) => ({
            type: "paragraph" as const,
            children: [{ type: "text" as const, value: `Intro line ${index}` }],
          })),
          children: [],
        },
        {
          id: "section-1",
          title: "LATE SECTION",
          blocks: [{
            type: "paragraph",
            children: [{ type: "text", value: "Late section body" }],
          }],
          children: [],
        },
      ]);
    const setup = await renderApp(result);
    const lateSection = navPosition(setup.captureCharFrame(), "LATE SECTION");
    await setup.mockMouse.click(NAV_WIDTH - 3, lateSection.y);
    await setup.flush();

    // Row 2 starts the padded content viewport. Trailing scroll space allows
    // even the final section to be positioned at that row.
    expect(contentPosition(setup.captureCharFrame(), "LATE SECTION").y).toBe(2);

    setup.renderer.destroy();
  });

  test("follows a page reference and reveals its collapsed sidebar ancestry", async () => {
    const result = mockQuery("internal-reference", [
      {
        id: "introduction",
        title: "INTRODUCTION",
        blocks: [
          {
            type: "paragraph",
            children: [
              { type: "text", value: "Continue with " },
              {
                type: "section-reference",
                target: "destination",
                children: [{ type: "text", value: "open destination" }],
              },
              { type: "text", value: "." },
            ],
          },
          ...Array.from({ length: 24 }, (_, index) => ({
            type: "paragraph" as const,
            children: [{ type: "text" as const, value: `Introduction line ${index}` }],
          })),
        ],
        children: [],
      },
      {
        id: "parent",
        title: "PARENT",
        blocks: [],
        children: [{
          id: "middle",
          title: "MIDDLE",
          blocks: [],
          children: [{
            id: "destination",
            title: "DESTINATION HEADING",
            blocks: [{
              type: "paragraph",
              children: [{ type: "text", value: "Destination body" }],
            }],
            children: [],
          }],
        }],
      },
    ]);
    const setup = await renderApp(result, { width: 100, height: 24 });
    const reference = contentPosition(setup.captureCharFrame(), "open destination");

    await setup.mockMouse.click(reference.x + 2, reference.y);
    const frame = await waitForFrame(setup, (current) =>
      current.split("\n")[2]?.slice(NAV_WIDTH).includes("DESTINATION HEADING") === true
      && navLines(current).some((line) => line.includes("›") && line.includes("DESTINATION HEADING")),
    );

    expect(contentPosition(frame, "DESTINATION HEADING").y).toBe(2);
    expect(navLines(frame).some((line) => line.includes("MIDDLE"))).toBe(true);
    setup.renderer.destroy();
  });

  test("updates navigation only after content scrolling becomes idle", async () => {
    const result = mockQuery("scroll-spy", [
        {
          id: "section-0",
          title: "INTRODUCTION",
          blocks: Array.from({ length: 28 }, (_, index) => ({
            type: "paragraph" as const,
            children: [{ type: "text" as const, value: `Intro line ${index}` }],
          })),
          children: [],
        },
        {
          id: "section-1",
          title: "CURRENT SECTION",
          blocks: Array.from({ length: 28 }, (_, index) => ({
            type: "paragraph" as const,
            children: [{ type: "text" as const, value: `Current line ${index}` }],
          })),
          children: [],
        },
      ]);
    const setup = await renderApp(result);
    for (let index = 0; index < 4; index++) {
      setup.mockInput.pressKey("d");
      await flushKeyboard(setup);
    }

    // Sidebar selection must not re-render for every movement during a burst.
    expect(navLines(setup.captureCharFrame()).some((line) => line.includes("› · INTRODUCTION"))).toBe(true);

    await new Promise<void>((resolve) => setTimeout(resolve, 220));
    await setup.flush();
    expect(
      navLines(setup.captureCharFrame()).some((line) => line.includes("› · CURRENT SECTION")),
    ).toBe(true);

    setup.renderer.destroy();
  });

  test("quits on q", async () => {
    let quitCalled = false;
    const setup = await renderApp(mockLsResult, { onQuit: () => { quitCalled = true; } });

    setup.mockInput.pressKey("q");
    await setup.flush();

    expect(quitCalled).toBe(true);
    setup.renderer.destroy();
  });

  test("navigates with j and k", async () => {
    const setup = await renderApp(mockLsResult);

    setup.mockInput.pressKey("j");
    await flushKeyboard(setup);
    expect(setup.captureCharFrame()).toContain("› · SYNOPSIS");

    setup.mockInput.pressKey("k");
    await flushKeyboard(setup);
    expect(setup.captureCharFrame()).toContain("› · NAME");

    setup.renderer.destroy();
  });

  test("moves from tldr to the first manual section", async () => {
    const setup = await renderApp(mockLsWithTldrResult, { width: 100, height: 32 });

    setup.mockInput.pressKey("j");
    await flushKeyboard(setup);

    expect(setup.captureCharFrame()).toContain("› · NAME");
    setup.renderer.destroy();
  });

  test("toggles the sidebar from the View menu", async () => {
    const setup = await renderApp(mockLsResult);

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

  test("shows keyboard help from its shortcut", async () => {
    const setup = await renderApp(mockLsResult);

    setup.mockInput.pressKey("?");
    await flushKeyboard(setup);
    expect(setup.captureCharFrame()).toContain("Keyboard Shortcuts");

    setup.mockInput.pressKey("?");
    await flushKeyboard(setup);
    expect(setup.captureCharFrame()).not.toContain("Keyboard Shortcuts");

    setup.renderer.destroy();
  });

  test("resizes navigation by dragging its colour boundary", async () => {
    const setup = await renderApp(mockLsResult);
    let frame = setup.captureCharFrame();
    const contentColumn = (currentFrame: string) =>
      currentFrame
        .split("\n")
        .find((line) => line.includes("list directory contents"))!
        .indexOf("list directory contents");
    const initialContentX = contentColumn(frame);

    await setup.mockMouse.drag(NAV_WIDTH, 2, NAV_WIDTH + 8, 2);
    await setup.flush();
    frame = setup.captureCharFrame();

    expect(navLines(frame).some((line) => line.includes("MANUAL"))).toBe(true);
    expect(contentColumn(frame)).toBeGreaterThan(initialContentX);

    setup.renderer.destroy();
  });

  test("reveals semantic options on demand and scrolls to their definitions", async () => {
    const result = mockQuery("tar", [{
      id: "options",
      title: "OPTIONS",
      blocks: [
        ...Array.from({ length: 24 }, (_, index) => ({
          type: "paragraph" as const,
          children: [{ type: "text" as const, value: `Option introduction ${index}` }],
        })),
        {
          type: "definition-list",
          items: [{
            identity: {
              id: "option-acls",
              role: "option",
              names: ["--acls"],
            },
            terms: [[{ type: "strong", children: [{ type: "text", value: "--acls" }] }]],
            description: [{
              type: "paragraph",
              children: [{ type: "text", value: "Enable ACL support." }],
            }],
          }],
        },
      ],
      children: [],
    }]);
    const setup = await renderApp(result, { width: 100, height: 24 });
    let frame = setup.captureCharFrame();
    expect(navLines(frame).some((line) => line.includes("OPTIONS (1)"))).toBe(true);
    expect(navLines(frame).some((line) => line.includes("--acls"))).toBe(false);

    const group = navPosition(frame, "OPTIONS (1)");
    await setup.mockMouse.click(NAV_WIDTH - 3, group.y);
    await setup.flush();
    frame = setup.captureCharFrame();
    const option = navPosition(frame, "--acls");

    await setup.mockMouse.click(NAV_WIDTH - 3, option.y);
    await setup.flush();
    frame = setup.captureCharFrame();
    expect(navLines(frame).some((line) => line.includes("›") && line.includes("--acls")))
      .toBe(true);
    expect(contentPosition(frame, "--acls").y).toBe(2);

    setup.renderer.destroy();
  });

  test("folds and unfolds child sections on mouse click", async () => {
    const setup = await renderApp(parentTreeResult());
    let frame = setup.captureCharFrame();
    expect(frame).toContain("▾ PARENT");
    expect(frame).toContain("╰─· CHILD");

    let parent = navPosition(frame, "PARENT");
    await setup.mockMouse.click(NAV_WIDTH - 3, parent.y);
    await setup.flush();
    frame = setup.captureCharFrame();
    expect(frame).toContain("▸ PARENT");
    expect(navLines(frame).some((line) => line.includes("CHILD"))).toBe(false);

    parent = navPosition(frame, "PARENT");
    await setup.mockMouse.click(NAV_WIDTH - 3, parent.y);
    await setup.flush();
    expect(setup.captureCharFrame()).toContain("▾ PARENT");

    setup.renderer.destroy();
  });

  test("navigates a section tree with h and l", async () => {
    const setup = await renderApp(parentTreeResult());
    expect(setup.captureCharFrame()).toContain("▾ PARENT");

    setup.mockInput.pressKey("h");
    await setup.flush();
    expect(setup.captureCharFrame()).toContain("▸ PARENT");

    setup.mockInput.pressArrow("right");
    await flushKeyboard(setup);
    expect(navLines(setup.captureCharFrame()).some((line) => line.includes("╰─· CHILD"))).toBe(true);

    setup.mockInput.pressArrow("right");
    await flushKeyboard(setup);
    expect(setup.captureCharFrame()).toContain("› ╰─· CHILD");

    setup.mockInput.pressArrow("left");
    await flushKeyboard(setup);
    expect(setup.captureCharFrame()).toContain("› ▾ PARENT");

    setup.renderer.destroy();
  });

  test("keeps wrapped selected navigation titles visually continuous", async () => {
    const result = mockQuery("long-title", [
        {
          id: "section-0",
          title: "PARENT",
          blocks: [],
          children: [{
            id: "section-0-0",
            title: "FIRSTMARKERABCDEFGHI SECONDMARKERABCDEFGH THIRDMARKERABCDEFGHI",
            blocks: [],
            children: [],
          }],
        },
        { id: "section-1", title: "SIBLING", blocks: [], children: [] },
      ]);
    const setup = await renderApp(result);
    setup.mockInput.pressKey("j");
    await flushKeyboard(setup);

    const lines = navLines(setup.captureCharFrame());
    expect(lines.some((line) => line.includes("FIRSTMARKERABCDEFGHI"))).toBe(true);
    expect(lines.some((line) => line.includes("SECONDMARKERABCDEFGH"))).toBe(true);
    expect(lines.some((line) => line.includes("THIRDMARKERABCDEFGHI"))).toBe(true);
    expect(lines.some((line) => line.includes("│") && line.includes("SECONDMARKER"))).toBe(true);

    // Dragging text must retain the item's uniform selection colour instead
    // of entering OpenTUI's fragment-level native text selection mode.
    const firstTitle = navPosition(setup.captureCharFrame(), "FIRSTMARKERABCDEFGHI");
    await setup.mockMouse.drag(firstTitle.x, firstTitle.y, firstTitle.x + 8, firstTitle.y);
    await setup.flush();
    const draggedTitleSpans = navigationSpans(setup.captureSpans().lines).filter((span) =>
      span.text.includes("FIRSTMARKER"),
    );
    expect(
      draggedTitleSpans.every((span) => span.bg.toInts().slice(0, 3).join(",") === "49,50,68"),
    ).toBe(true);

    // Search highlighting belongs to manual content, never to sidebar labels.
    // Keeping one item-level background preserves wrapped tree connectors.
    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    setup.mockInput.typeText("firstmarker");
    await flushKeyboard(setup);
    const searchedTitleSpans = navigationSpans(setup.captureSpans().lines).filter((span) =>
      span.text.includes("FIRSTMARKER"),
    );
    expect(searchedTitleSpans.length).toBeGreaterThan(0);
    expect(
      searchedTitleSpans.every((span) => span.bg.toInts().slice(0, 3).join(",") === "49,50,68"),
    ).toBe(true);

    setup.renderer.destroy();
  });

  test("uses only the item background for searched GCC navigation titles", async () => {
    const setup = await renderApp(mockQuery("gcc", [{
      id: "options",
      title: "OPTIONS",
      blocks: [],
      children: [{
        id: "kind-of-output",
        title: "Options Controlling the Kind of Output",
        blocks: [],
        children: [],
      }],
    }]));
    setup.mockInput.pressKey("j");
    await flushKeyboard(setup);
    setup.mockInput.pressKey("/");
    await flushKeyboard(setup);
    setup.mockInput.typeText("kind of output");
    await flushKeyboard(setup);

    const titleSpans = navigationSpans(setup.captureSpans().lines).filter((span) =>
      /Options Controlling|Kind of Output/.test(span.text),
    );
    expect(titleSpans.length).toBeGreaterThan(0);
    expect(
      titleSpans.every((span) => span.bg.toInts().slice(0, 3).join(",") === "49,50,68"),
    ).toBe(true);

    setup.renderer.destroy();
  });
});
