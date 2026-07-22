/**
 * @file Verifies that semantic v2 definitions become compact sidebar nodes.
 */

import { describe, expect, test } from "bun:test";
import type { MantSection } from "../../../src/native";
import {
  buildNavigationNodes,
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
});
