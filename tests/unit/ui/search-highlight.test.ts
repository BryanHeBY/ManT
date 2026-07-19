/**
 * @file Verifies conversion from AST search offsets to OpenTUI highlights.
 */

import { describe, expect, test } from "bun:test";
import { toTextBufferRange } from "../../../src/ui/search-highlight";

describe("search highlight coordinates", () => {
  test("removes preceding line separators from multiline buffer offsets", () => {
    const text = "first line\nsecond line\nprintf(\"hello\");";
    const start = text.indexOf("hello");

    expect(toTextBufferRange(text, { start, end: start + 5 })).toEqual({
      start: start - 2,
      end: start + 5 - 2,
    });
  });

  test("also removes a separator contained inside a cross-line range", () => {
    const text = "before\nacross\nlines\nafter";
    const start = text.indexOf("across");
    const end = text.indexOf("\nafter");

    expect(toTextBufferRange(text, { start, end })).toEqual({
      start: start - 1,
      end: end - 2,
    });
  });
});
