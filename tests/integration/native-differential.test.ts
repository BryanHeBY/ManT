/**
 * @file Guards the native cut-over against the current HTML-based query path.
 *
 * Both sides read the same installed source page. Exact layout nodes differ by
 * design, so the gate compares semantic section topology and normalized prose
 * vocabulary rather than renderer whitespace.
 */

import { describe, expect, test } from "bun:test";
import type {
  BlockNode as LegacyBlock,
  InlineNode as LegacyInline,
  SectionNode as LegacySection,
} from "../../src/core";
import {
  createNativeCliClient,
  type MantBlock,
  type MantInline,
  type MantSection,
} from "../../src/native";
import { query as legacyQuery } from "../../src/query";

const topics = ["ls", "git", "gcc", "clang", "tar"] as const;
const nativeCliPath = new URL("../../native/bin/mant-cli", import.meta.url).pathname;
const nativeCliAvailable = Bun.spawnSync(
  [nativeCliPath, "protocol-version", "--compact"],
  { stdout: "ignore", stderr: "ignore" },
).exitCode === 0;
const describeDifferential = nativeCliAvailable ? describe : describe.skip;

describeDifferential("native/legacy installed-man differential", () => {
  for (const topic of topics) {
    const manualAvailable = Bun.spawnSync(["man", "-w", topic], {
      stdout: "ignore",
      stderr: "ignore",
    }).exitCode === 0;
    const testTopic = manualAvailable ? test : test.skip;

    testTopic(`${topic} preserves topology and semantic prose`, async () => {
      const nativeClient = createNativeCliClient({
        env: { MANT_CLI_PATH: nativeCliPath },
        which: () => null,
      });
      const [legacy, native] = await Promise.all([
        legacyQuery({ topic }),
        nativeClient.query({ topic }),
      ]);
      const nativeSections = native.manual?.sections;
      expect(nativeSections?.length).toBeGreaterThan(0);

      expect(flattenLegacyTitles(legacy.sections)).toEqual(
        flattenNativeTitles(nativeSections ?? []),
      );

      const legacyWords = vocabulary(legacy.sections.flatMap(legacySectionText).join(" "));
      const nativeWords = vocabulary((nativeSections ?? []).flatMap(nativeSectionText).join(" "));
      const sharedWords = [...legacyWords].filter((word) => nativeWords.has(word));
      const legacyCoverage = sharedWords.length / Math.max(legacyWords.size, 1);
      const nativeCoverage = sharedWords.length / Math.max(nativeWords.size, 1);
      // Renderer link labels and semantic mdoc expansions intentionally differ,
      // but neither representation may lose a substantial part of the page.
      expect(legacyCoverage).toBeGreaterThanOrEqual(0.85);
      expect(nativeCoverage).toBeGreaterThanOrEqual(0.85);
    }, 15_000);
  }
});

function flattenLegacyTitles(sections: LegacySection[]): string[] {
  return sections.flatMap((section) => [
    normalizeText(section.title),
    ...flattenLegacyTitles(section.children),
  ]);
}

function flattenNativeTitles(sections: MantSection[]): string[] {
  return sections.flatMap((section) => [
    normalizeText(section.title),
    ...flattenNativeTitles(section.children),
  ]);
}

function legacySectionText(section: LegacySection): string[] {
  return [
    section.title,
    ...section.blocks.map(legacyBlockText),
    ...section.children.flatMap(legacySectionText),
  ];
}

function legacyBlockText(block: LegacyBlock): string {
  switch (block.type) {
    case "paragraph":
    case "pre":
      return legacyInlineText(block.children);
    case "list":
      return block.items.map(legacyInlineText).join(" ");
    case "definition-list":
      return block.items.flatMap((item) => [
        ...item.terms.map(legacyInlineText),
        legacyInlineText(item.description),
      ]).join(" ");
    case "spacer":
      return "";
  }
}

function legacyInlineText(inlines: LegacyInline[]): string {
  return inlines.map((inline) => {
    if (inline.type === "text") return inline.content;
    if (inline.type === "break") return " ";
    return legacyInlineText(inline.children);
  }).join("");
}

function nativeSectionText(section: MantSection): string[] {
  return [
    section.title,
    ...section.blocks.map(nativeBlockText),
    ...section.children.flatMap(nativeSectionText),
  ];
}

function nativeBlockText(block: MantBlock): string {
  switch (block.type) {
    case "paragraph":
    case "preformatted":
      return nativeInlineText(block.children);
    case "list":
      return block.items.flatMap((item) => item.blocks.map(nativeBlockText)).join(" ");
    case "definition-list":
      return block.items.flatMap((item) => [
        ...item.terms.map(nativeInlineText),
        ...item.description.map(nativeBlockText),
      ]).join(" ");
    case "table":
      return block.rows.flatMap((row) => row.cells)
        .flatMap((cell) => cell.blocks.map(nativeBlockText)).join(" ");
    case "equation":
      return block.value;
    case "unsupported":
      return block.text;
    case "vertical-space":
      return "";
  }
}

function nativeInlineText(inlines: MantInline[]): string {
  return inlines.map((inline) => {
    switch (inline.type) {
      case "text":
      case "code":
        return inline.value;
      case "line-break":
        return " ";
      case "strong":
      case "emphasis":
      case "link":
      case "manual-reference":
        return nativeInlineText(inline.children);
    }
  }).join("");
}

function vocabulary(text: string): Set<string> {
  return new Set(
    normalizeText(text)
      .split(/[^\p{L}\p{N}_+-]+/u)
      .filter((word) => word.length >= 3),
  );
}

function normalizeText(text: string): string {
  return text
    .normalize("NFKC")
    .replace(/[‐‑‒–—−]/g, "-")
    .replace(/\s+/g, " ")
    .trim()
    .toLocaleLowerCase();
}
