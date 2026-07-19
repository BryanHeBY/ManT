/**
 * @file Defines compact native query contracts shared by terminal UI tests.
 */

import type {
  MantDocument,
  MantQueryBundle,
  MantSection,
} from "../../src/native";

function manual(sections: MantSection[]): MantDocument {
  return {
    schema: "mant.document/v1",
    producer: {
      name: "mant",
      version: "0.1.0",
      engine: { name: "libmandoc", version: "1.14.6" },
    },
    source: { format: "man", path: "/fixtures/manual.1" },
    meta: {},
    sections,
  };
}

export const mockLsSections: MantSection[] = [
  {
    id: "section-0",
    title: "NAME",
    blocks: [{
      type: "paragraph",
      children: [{ type: "text", value: "ls − list directory contents" }],
    }],
    children: [],
  },
  {
    id: "section-1",
    title: "SYNOPSIS",
    blocks: [{
      type: "paragraph",
      children: [
        { type: "strong", children: [{ type: "text", value: "ls" }] },
        { type: "text", value: " [OPTION]... [FILE]..." },
      ],
    }],
    children: [],
  },
  {
    id: "section-2",
    title: "DESCRIPTION",
    blocks: [{
      type: "paragraph",
      children: [{ type: "text", value: "List information about files." }],
    }],
    children: [],
  },
];

export const mockLsResult: MantQueryBundle = {
  schema: "mant.query/v1",
  topic: "ls",
  manual: manual(mockLsSections),
};

export const mockLsWithTldrResult: MantQueryBundle = {
  ...mockLsResult,
  tldr: {
    title: "ls",
    description: ["List directory contents."],
    examples: [
      {
        description: "List files, including hidden entries",
        command: "ls {{[-a|--all]}}",
        commandParts: [
          { type: "text", value: "ls " },
          { type: "placeholder", value: "--all" },
        ],
      },
      {
        description: "List files in long format",
        command: "ls {{[-l|--format=long]}}",
        commandParts: [
          { type: "text", value: "ls " },
          { type: "placeholder", value: "--format=long" },
        ],
      },
    ],
    moreInformation: "https://www.gnu.org/software/coreutils/ls",
    platform: "common",
    language: "en",
    sourcePath: "/cache/mant/tldr-pages/pages/common/ls.md",
  },
};

export function mockQuery(topic: string, sections: MantSection[]): MantQueryBundle {
  return {
    schema: "mant.query/v1",
    topic,
    manual: manual(sections),
  };
}
