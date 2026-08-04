/**
 * @file Verifies that semantic v2 definitions become compact sidebar nodes.
 */

import { describe, expect, test } from "bun:test";
import type { MantSection } from "../../../src/native";
import {
  buildNavigationNodes,
  buildNavigationRows,
  flattenVisibleNodes,
} from "../../../src/ui/navigation-tree";

const sections: MantSection[] = [{
  id: "options",
  title: "OPTIONS",
  blocks: [{
    type: "definition-list",
    items: [{
      identity: {
        id: "option-acls",
        role: "option",
        names: ["--acls"],
      },
      terms: [[{ type: "text", value: "--acls" }]],
      description: [{
        type: "paragraph",
        children: [{ type: "text", value: "Enable ACL support." }],
      }],
    }],
  }],
  children: [{
    id: "compatibility",
    title: "COMPATIBILITY",
    blocks: [],
    children: [],
  }],
}];

describe("semantic navigation tree", () => {
  test("groups addressable options without flattening section hierarchy", () => {
    const nodes = buildNavigationNodes(sections);
    const options = nodes[0]?.children[0];

    expect(options).toMatchObject({
      title: "OPTIONS (1)",
      kind: "entry-group",
      targetId: "options",
    });
    expect(options?.children[0]).toEqual({
      id: "option-acls",
      title: "--acls",
      kind: "option",
      targetId: "option-acls",
      children: [],
    });
    expect(nodes[0]?.children[1]?.id).toBe("compatibility");
  });

  test("reveals option entries only after their virtual group is expanded", () => {
    const nodes = buildNavigationNodes(sections);
    const groupId = nodes[0]?.children[0]?.id ?? "";

    expect(flattenVisibleNodes(nodes, new Set(["options"])).map(({ node }) => node.title))
      .toEqual(["OPTIONS", "OPTIONS (1)", "COMPATIBILITY"]);
    expect(flattenVisibleNodes(nodes, new Set(["options", groupId])).map(({ node }) => node.title))
      .toEqual(["OPTIONS", "OPTIONS (1)", "--acls", "COMPATIBILITY"]);
  });

  test("produces one row per unselected visible node", () => {
    const nodes = buildNavigationNodes(sections);
    const flat = flattenVisibleNodes(nodes, new Set(["options", groupId(nodes)]));
    const rows = buildNavigationRows(flat, new Set(["options", groupId(nodes)]), 40, "");

    expect(rows.map(({ nodeId, text }) => ({ nodeId, text }))).toEqual([
      { nodeId: "options", text: "OPTIONS" },
      { nodeId: groupId(nodes), text: "OPTIONS (1)" },
      { nodeId: "option-acls", text: "--acls" },
      { nodeId: "compatibility", text: "COMPATIBILITY" },
    ]);
  });

  test("wraps only the selected node into fixed-height continuation rows", () => {
    const nodes = buildNavigationNodes(sections);
    const flat = flattenVisibleNodes(nodes, new Set(["options"]));
    const rows = buildNavigationRows(flat, new Set(["options"]), 10, "options");

    const selectedRows = rows.filter(({ nodeId }) => nodeId === "options");
    expect(selectedRows.length).toBeGreaterThan(1);
    expect(selectedRows[0]!.lineIndex).toBe(0);
    expect(selectedRows[0]!.prefix).toContain("▾");
    expect(selectedRows.slice(1).every(({ lineIndex }) => lineIndex > 0)).toBe(true);
    expect(selectedRows.slice(1).every(({ continuationPrefix }) => continuationPrefix.length > 0))
      .toBe(true);

    const otherRows = rows.filter(({ nodeId }) => nodeId !== "options");
    expect(otherRows.every(({ lineIndex }) => lineIndex === 0)).toBe(true);
  });

  test("omits the current node's connector from continuation-line prefixes", () => {
    const nodes = buildNavigationNodes(sections);
    const flat = flattenVisibleNodes(nodes, new Set(["options"]));
    const rows = buildNavigationRows(flat, new Set(["options"]), 40, "compatibility");
    const selectedRows = rows.filter(({ nodeId }) => nodeId === "compatibility");

    expect(selectedRows[0]!.prefix).toContain("╰─");
    expect(selectedRows[0]!.continuationPrefix).not.toContain("╰─");
    expect(selectedRows[0]!.continuationPrefix).not.toContain("├─");
  });

  test("draws a connected branch for an expanded last child", () => {
    const nodes = buildNavigationNodes([{
      id: "top",
      title: "TOP",
      blocks: [],
      children: [{
        id: "parent",
        title: "PARENT",
        blocks: [],
        children: [{
          id: "child",
          title: "CHILD",
          blocks: [],
          children: [],
        }],
      }],
    }]);
    const expanded = new Set(["top", "parent"]);
    const flat = flattenVisibleNodes(nodes, expanded);
    const rows = buildNavigationRows(flat, expanded, 40, "parent");
    const parentRow = rows.find(({ nodeId }) => nodeId === "parent");
    const childRow = rows.find(({ nodeId }) => nodeId === "child");

    expect(parentRow?.prefix).toContain("│ ├─▾");
    expect(parentRow?.continuationPrefix).toContain("│ │");
    expect(childRow?.prefix).toContain("│ │ ╰─·");
  });

  test("continues the connector guide through wrapped non-last leaves", () => {
    const nodes = buildNavigationNodes([{
      id: "top",
      title: "TOP",
      blocks: [],
      children: [
        {
          id: "parent",
          title: "PARENT LONG TITLE",
          blocks: [],
          children: [],
        },
        { id: "sibling", title: "SIBLING", blocks: [], children: [] },
      ],
    }]);
    const expanded = new Set(["top"]);
    const flat = flattenVisibleNodes(nodes, expanded);
    const rows = buildNavigationRows(flat, expanded, 10, "parent");
    const parentRows = rows.filter(({ nodeId }) => nodeId === "parent");

    expect(parentRows[0]!.prefix).toContain("│ ├─·");
    expect(parentRows[1]!.continuationPrefix).toContain("│ │");
    expect(parentRows[2]!.continuationPrefix).toContain("│ │");
  });

  test("draws a guide for all top-level nodes that have children", () => {
    const nodes = buildNavigationNodes([{
      id: "top",
      title: "TOP",
      blocks: [],
      children: [
        { id: "child", title: "CHILD", blocks: [], children: [] },
      ],
    }]);

    // Expanded: guide runs from the parent down through children.
    const expanded = new Set(["top"]);
    const flatExpanded = flattenVisibleNodes(nodes, expanded);
    const rowsExpanded = buildNavigationRows(flatExpanded, expanded, 40, "top");
    const expandedTopRow = rowsExpanded.find(({ nodeId }) => nodeId === "top");
    const childRow = rowsExpanded.find(({ nodeId }) => nodeId === "child");

    expect(expandedTopRow?.prefix).toContain("│ ▾");
    expect(expandedTopRow?.continuationPrefix).toContain("│");
    expect(childRow?.prefix).toContain("│ ╰─·");

    // Collapsed: the parent still keeps its guide column so wrapped titles stay
    // connected even when the subtree is hidden.
    const collapsed = new Set<string>();
    const flatCollapsed = flattenVisibleNodes(nodes, collapsed);
    const rowsCollapsed = buildNavigationRows(flatCollapsed, collapsed, 40, "top");
    const collapsedTopRow = rowsCollapsed.find(({ nodeId }) => nodeId === "top");

    expect(collapsedTopRow?.prefix).toContain("│ ▸");
    expect(collapsedTopRow?.continuationPrefix).toContain("│");
  });

  test("adds a subtree guide to wrapped continuation lines of an expanded parent", () => {
    const nodes = buildNavigationNodes([{
      id: "top",
      title: "TOP",
      blocks: [],
      children: [{
        id: "parent",
        title: "HANDLING OF FILE ATTRIBUTES",
        blocks: [],
        children: [
          { id: "child-a", title: "CHILD A", blocks: [], children: [] },
          { id: "child-b", title: "CHILD B", blocks: [], children: [] },
        ],
      }],
    }]);
    const expanded = new Set(["top", "parent"]);
    const flat = flattenVisibleNodes(nodes, expanded);
    const rows = buildNavigationRows(flat, expanded, 10, "parent");
    const parentRows = rows.filter(({ nodeId }) => nodeId === "parent");
    const childRow = rows.find(({ nodeId }) => nodeId === "child-a");

    // First line has the disclosure at the node's own indentation.
    expect(parentRows[0]!.prefix).toContain("│ ├─▾");
    // Wrapped continuation lines sit above the children, so they carry the
    // extra guide column that connects the parent down to its subtree.
    expect(parentRows[1]!.continuationPrefix).toContain("│ │ │");
    expect(parentRows[2]!.continuationPrefix).toContain("│ │ │");
    // The child's prefix shares the ancestor columns and aligns with the
    // subtree guide in the parent's continuation lines.
    expect(childRow?.prefix).toContain("│ │ ├─·");
  });

  test("keeps the node guide through wrapped last-leaf continuation lines", () => {
    const nodes = buildNavigationNodes([{
      id: "top",
      title: "TOP",
      blocks: [],
      children: [{
        id: "leaf",
        title: "HANDLING OF FILE ATTRIBUTE",
        blocks: [],
        children: [],
      }],
    }]);
    const expanded = new Set(["top"]);
    const flat = flattenVisibleNodes(nodes, expanded);
    const rows = buildNavigationRows(flat, expanded, 10, "leaf");
    const leafRows = rows.filter(({ nodeId }) => nodeId === "leaf");

    expect(leafRows[0]!.prefix).toContain("│ ╰─·");
    expect(leafRows[1]!.continuationPrefix).toContain("│ │");
    expect(leafRows[2]!.continuationPrefix).toContain("│ │");
  });
});

function groupId(nodes: ReturnType<typeof buildNavigationNodes>): string {
  return nodes[0]?.children[0]?.id ?? "";
}
