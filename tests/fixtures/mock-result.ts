import type { QueryResult } from "../../src/query";

export const mockLsResult: QueryResult = {
  topic: "ls",
  sections: [
    {
      id: "section-0",
      title: "NAME",
      level: 2,
      blocks: [
        {
          type: "paragraph",
          children: [
            { type: "text", content: "ls − list directory contents" },
          ],
          indent: 0,
        },
      ],
      children: [],
    },
    {
      id: "section-1",
      title: "SYNOPSIS",
      level: 2,
      blocks: [
        {
          type: "paragraph",
          children: [
            { type: "bold", children: [{ type: "text", content: "ls" }] },
            { type: "text", content: " [OPTION]... [FILE]..." },
          ],
          indent: 0,
        },
      ],
      children: [],
    },
    {
      id: "section-2",
      title: "DESCRIPTION",
      level: 2,
      blocks: [
        {
          type: "paragraph",
          children: [
            { type: "text", content: "List information about files." },
          ],
          indent: 0,
        },
      ],
      children: [],
    },
  ],
};
