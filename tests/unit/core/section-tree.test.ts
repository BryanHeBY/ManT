/**
 * @file Tests the shared heading stack used by groff and mandoc parsers.
 */

import { describe, expect, test } from "bun:test";
import { SectionTree } from "../../../src/core/section-tree";

describe("SectionTree", () => {
  test("nests deeper headings and attaches blocks to the current section", () => {
    const tree = new SectionTree();
    tree.pushSection("OPTIONS", 2);
    tree.addBlock({
      type: "paragraph",
      children: [{ type: "text", content: "Overview" }],
      indent: 0,
    });
    tree.pushSection("Output", 3);
    tree.addBlock({
      type: "paragraph",
      children: [{ type: "text", content: "Details" }],
      indent: 4,
    });

    expect(tree.getSections()).toEqual([{
      id: "section-0",
      title: "OPTIONS",
      level: 2,
      blocks: [{
        type: "paragraph",
        children: [{ type: "text", content: "Overview" }],
        indent: 0,
      }],
      children: [{
        id: "section-1",
        title: "Output",
        level: 3,
        blocks: [{
          type: "paragraph",
          children: [{ type: "text", content: "Details" }],
          indent: 4,
        }],
        children: [],
      }],
    }]);
  });

  test("closes same-or-shallower ancestors before starting a peer", () => {
    const tree = new SectionTree();
    tree.pushSection("FIRST", 2);
    tree.pushSection("Child", 3);
    tree.pushSection("SECOND", 2);

    expect(tree.getSections().map((section) => section.title)).toEqual(["FIRST", "SECOND"]);
    expect(tree.getSections()[0]?.children.map((section) => section.title)).toEqual(["Child"]);
    expect(tree.currentSection()?.title).toBe("SECOND");
  });

  test("ignores blocks before the first heading", () => {
    const tree = new SectionTree();
    tree.addBlock({
      type: "paragraph",
      children: [{ type: "text", content: "orphan" }],
      indent: 0,
    });

    expect(tree.getSections()).toEqual([]);
    expect(tree.currentSection()).toBeNull();
  });
});
