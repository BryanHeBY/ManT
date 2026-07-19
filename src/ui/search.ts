/**
 * @file Builds and queries the immutable, renderer-aligned page-search index.
 *
 * Every record maps one visible text leaf to a structural render path. Query
 * confirmation therefore scans pre-normalized strings without walking the AST,
 * and a result can target a definition term or nested paragraph precisely.
 */

import type {
  MantBlock,
  MantInline,
  MantSection,
  TldrDocument,
} from "../native";
import { contentId, contentSearchId, TLDR_NAV_ID } from "./ids";

export interface SearchRange {
  start: number;
  end: number;
}

export interface SearchMatch {
  targetId: string;
  targetPath: string;
  sectionId: string;
  /** IDs from the top-level section through the matching section. */
  sectionPath: readonly string[];
  title: string;
  /** Visible leaf text used to resolve the match's wrapped terminal row. */
  text: string;
  range: SearchRange;
}

export interface SearchRecord {
  targetId: string;
  targetPath: string;
  sectionId: string;
  sectionPath: readonly string[];
  title: string;
  text: string;
  normalizedText: string;
}

export interface PageSearchIndex {
  records: readonly SearchRecord[];
}

export const SECTION_HEADING_SEARCH_PATH = "heading";
export const TLDR_TITLE_SEARCH_PATH = "title";

/** Shared path construction keeps index records and render anchors identical. */
export const searchPath = {
  block: (parent: string, index: number) => extendPath(parent, `block-${index}`),
  listItem: (parent: string, index: number) => extendPath(parent, `item-${index}`),
  definition: (parent: string, index: number) => extendPath(parent, `definition-${index}`),
  term: (parent: string, index: number) => extendPath(parent, `term-${index}`),
  row: (parent: string, index: number) => extendPath(parent, `row-${index}`),
  cell: (parent: string, index: number) => extendPath(parent, `cell-${index}`),
  inline: (parent: string, index: number) => extendPath(parent, `inline-${index}`),
  tldrDescription: (index: number) => `description-${index}`,
  tldrExampleDescription: (index: number) => `example-${index}-description`,
  tldrExampleCommand: (index: number) => `example-${index}-command`,
  tldrMoreInformation: () => "more-information",
} as const;

function extendPath(parent: string, segment: string): string {
  return parent ? `${parent}.${segment}` : segment;
}

/** Flatten inline formatting while retaining visible hard line breaks. */
export function flattenInlineText(nodes: MantInline[]): string {
  return nodes
    .map((node) => {
      switch (node.type) {
        case "text":
        case "code":
          return node.value;
        case "line-break":
          return "\n";
        case "strong":
        case "emphasis":
        case "external-link":
        case "email-link":
        case "manual-reference":
        case "section-reference":
          return flattenInlineText(node.children);
        case "anchor":
          return "";
      }
    })
    .join("");
}

/** Match the prose whitespace normalization performed by the TUI renderer. */
export function visibleInlineSegments(nodes: MantInline[]): MantInline[][] {
  const segments: MantInline[][] = [[]];
  for (const node of nodes) {
    if (node.type === "line-break") segments.push([]);
    else segments[segments.length - 1]!.push(node);
  }
  return segments
    .filter((segment) => segment.length > 0)
    .map(trimSegmentWhitespace);
}

function trimSegmentWhitespace(nodes: MantInline[]): MantInline[] {
  const trimmed = nodes.map((node) => node.type === "text" ? { ...node } : node);
  const first = trimmed[0];
  if (first?.type === "text") first.value = first.value.replace(/^\s+/, "");
  const last = trimmed[trimmed.length - 1];
  if (last?.type === "text") last.value = last.value.replace(/\s+$/, "");
  return trimmed;
}

function visibleProseText(nodes: MantInline[]): string {
  return visibleInlineSegments(nodes).map(flattenInlineText).join("\n");
}

function hasInteractiveInline(nodes: MantInline[]): boolean {
  return nodes.some((node) => node.type === "section-reference" || node.type === "anchor");
}

/** Build the index once when a new native query result enters the TUI. */
export function buildPageSearchIndex(
  sections: MantSection[],
  tldr: TldrDocument | undefined,
): PageSearchIndex {
  const records: SearchRecord[] = [];

  const addRecord = (
    text: string,
    sectionId: string,
    sectionPath: readonly string[],
    title: string,
    targetPath: string,
    targetId = contentSearchId(sectionId, targetPath),
  ) => {
    if (!text) return;
    records.push(Object.freeze({
      targetId,
      targetPath,
      sectionId,
      sectionPath,
      title,
      text,
      normalizedText: text.toLocaleLowerCase(),
    }));
  };

  if (tldr) indexTldr(tldr, addRecord);

  const indexSection = (section: MantSection, ancestors: readonly string[]) => {
    const sectionIds = Object.freeze([...ancestors, section.id]);
    addRecord(
      section.title,
      section.id,
      sectionIds,
      section.title,
      SECTION_HEADING_SEARCH_PATH,
      contentId(section.id),
    );
    indexBlocks(section.blocks, section.id, sectionIds, section.title, "", addRecord);
    for (const child of section.children) indexSection(child, sectionIds);
  };

  for (const section of sections) indexSection(section, []);
  return Object.freeze({ records: Object.freeze(records) });
}

type AddRecord = (
  text: string,
  sectionId: string,
  sectionPath: readonly string[],
  title: string,
  targetPath: string,
  targetId?: string,
) => void;

function indexBlocks(
  blocks: MantBlock[],
  sectionId: string,
  sectionIds: readonly string[],
  title: string,
  parentPath: string,
  addRecord: AddRecord,
): void {
  let proseGroup: { targetPath: string; indent: number; text: string[] } | undefined;
  const flushProseGroup = () => {
    if (!proseGroup) return;
    addRecord(
      proseGroup.text.join("\n"),
      sectionId,
      sectionIds,
      title,
      proseGroup.targetPath,
    );
    proseGroup = undefined;
  };

  blocks.forEach((block, blockIndex) => {
    const blockPath = searchPath.block(parentPath, blockIndex);
    if (block.type === "paragraph" && !hasInteractiveInline(block.children)) {
      const indent = block.layout?.indentColumns ?? 0;
      if (proseGroup?.indent !== indent) flushProseGroup();
      const text = visibleProseText(block.children);
      if (!text) return;
      proseGroup ??= { targetPath: blockPath, indent, text: [] };
      proseGroup.text.push(text);
      return;
    }

    flushProseGroup();
    switch (block.type) {
      case "paragraph":
        indexInteractiveParagraph(
          block.children,
          sectionId,
          sectionIds,
          title,
          blockPath,
          addRecord,
        );
        break;
      case "preformatted":
        addRecord(flattenInlineText(block.children), sectionId, sectionIds, title, blockPath);
        break;
      case "list":
        block.items.forEach((item, itemIndex) => {
          indexBlocks(
            item.blocks,
            sectionId,
            sectionIds,
            title,
            searchPath.listItem(blockPath, itemIndex),
            addRecord,
          );
        });
        break;
      case "definition-list":
        block.items.forEach((item, itemIndex) => {
          const itemPath = searchPath.definition(blockPath, itemIndex);
          item.terms.forEach((term, termIndex) => {
            addRecord(
              flattenInlineText(term),
              sectionId,
              sectionIds,
              title,
              searchPath.term(itemPath, termIndex),
            );
          });
          indexBlocks(item.description, sectionId, sectionIds, title, itemPath, addRecord);
        });
        break;
      case "table":
        block.rows.forEach((row, rowIndex) => {
          row.cells.forEach((cell, cellIndex) => {
            indexBlocks(
              cell.blocks,
              sectionId,
              sectionIds,
              title,
              searchPath.cell(searchPath.row(blockPath, rowIndex), cellIndex),
              addRecord,
            );
          });
        });
        break;
      case "equation":
        addRecord(block.value, sectionId, sectionIds, title, blockPath);
        break;
      case "unsupported":
        addRecord(block.text, sectionId, sectionIds, title, blockPath);
        break;
      case "vertical-space":
        break;
    }
  });
  flushProseGroup();
}

/** Mirror the separate Text renderables used for clickable section links. */
function indexInteractiveParagraph(
  nodes: MantInline[],
  sectionId: string,
  sectionIds: readonly string[],
  title: string,
  blockPath: string,
  addRecord: AddRecord,
): void {
  let ordinary: MantInline[] = [];
  let partIndex = 0;
  const addPart = (part: MantInline[]) => {
    addRecord(
      flattenInlineText(part),
      sectionId,
      sectionIds,
      title,
      searchPath.inline(blockPath, partIndex++),
    );
  };
  const flushOrdinary = () => {
    if (ordinary.length === 0) return;
    addPart(ordinary);
    ordinary = [];
  };

  for (const node of nodes) {
    if (node.type === "section-reference") {
      flushOrdinary();
      addPart(node.children);
    } else if (node.type === "anchor") {
      flushOrdinary();
    } else {
      ordinary.push(node);
    }
  }
  flushOrdinary();
}

function indexTldr(page: TldrDocument, addRecord: AddRecord): void {
  const sectionPath = Object.freeze([TLDR_NAV_ID]);
  addRecord(
    `TLDR QUICK REFERENCE · ${page.title}`,
    TLDR_NAV_ID,
    sectionPath,
    "TLDR QUICK REFERENCE",
    TLDR_TITLE_SEARCH_PATH,
    contentId(TLDR_NAV_ID),
  );
  page.description.forEach((line, index) => {
    addRecord(
      line,
      TLDR_NAV_ID,
      sectionPath,
      "TLDR QUICK REFERENCE",
      searchPath.tldrDescription(index),
    );
  });
  page.examples.forEach((example, index) => {
    addRecord(
      example.description,
      TLDR_NAV_ID,
      sectionPath,
      "TLDR QUICK REFERENCE",
      searchPath.tldrExampleDescription(index),
    );
    const command = example.commandParts.map((part) => part.value).join("");
    addRecord(
      command,
      TLDR_NAV_ID,
      sectionPath,
      "TLDR QUICK REFERENCE",
      searchPath.tldrExampleCommand(index),
    );
  });
  if (page.moreInformation) {
    addRecord(
      `More information: ${page.moreInformation}`,
      TLDR_NAV_ID,
      sectionPath,
      "TLDR QUICK REFERENCE",
      searchPath.tldrMoreInformation(),
    );
  }
}

/** Return one result per visible occurrence, in document order. */
export function queryPageSearchIndex(index: PageSearchIndex, query: string): SearchMatch[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  if (!normalizedQuery) return [];

  const matches: SearchMatch[] = [];
  for (const record of index.records) {
    let cursor = 0;
    while (cursor < record.normalizedText.length) {
      const start = record.normalizedText.indexOf(normalizedQuery, cursor);
      if (start < 0) break;
      matches.push({
        targetId: record.targetId,
        targetPath: record.targetPath,
        sectionId: record.sectionId,
        sectionPath: record.sectionPath,
        title: record.title,
        text: record.text,
        range: { start, end: start + normalizedQuery.length },
      });
      cursor = start + normalizedQuery.length;
    }
  }
  return matches;
}
