/**
 * @file Renders the structured manual and optional tldr quick reference.
 *
 * This module owns display-oriented transformations only. It does not manage
 * selection, scrolling, or input state; callers supply the current search
 * target when an anchor is needed.
 */

import type { ReactNode } from "react";
import type { BlockNode, InlineNode, SectionNode } from "../core";
import { type TldrCommandPart, type TldrPage } from "../tldr";
import { contentBlockId, contentId, TLDR_NAV_ID } from "./ids";
import { Pre } from "./Pre";
import { renderSearchHighlights } from "./search-highlight";

function splitByBreak(nodes: InlineNode[]): InlineNode[][] {
  const segments: InlineNode[][] = [[]];
  for (const node of nodes) {
    if (node.type === "break") segments.push([]);
    else segments[segments.length - 1]!.push(node);
  }
  return segments.filter((segment) => segment.length > 0).map(trimSegmentWhitespace);
}

/** Removes formatter whitespace without mutating the parser-owned AST nodes. */
function trimSegmentWhitespace(nodes: InlineNode[]): InlineNode[] {
  if (nodes.length === 0) return nodes;

  const trimmed = nodes.map((node) => (
    node.type === "text" ? { ...node } : node
  ));
  const first = trimmed[0];
  if (first?.type === "text") first.content = first.content.replace(/^\s+/, "");

  const last = trimmed[trimmed.length - 1];
  if (last?.type === "text") last.content = last.content.replace(/\s+$/, "");
  return trimmed;
}

/**
 * Merges adjacent prose blocks into one text buffer. Large manuals such as
 * gcc can otherwise exceed OpenTUI's native TextBuffer limit with thousands
 * of individually rendered paragraphs.
 */
function renderBlockNodes(
  blocks: BlockNode[],
  baseIndent = 0,
  searchQuery = "",
  sectionId?: string,
  activeBlockIndex?: number,
): ReactNode[] {
  const result: ReactNode[] = [];
  let inlineBuffer: ReactNode[] = [];
  let bufferIndent = 0;
  let bufferAnchorId: string | undefined;
  let keyCounter = 0;
  let inlineKey = 0;

  const renderInlineNodes = (nodes: InlineNode[]): ReactNode[] => nodes.map((node) => {
    const key = inlineKey++;
    switch (node.type) {
      case "text":
        return renderSearchHighlights(node.content, searchQuery, `inline-${key}`);
      case "bold":
        return <span key={key} fg="#cdd6f4"><b>{renderInlineNodes(node.children)}</b></span>;
      case "italic":
        return <span key={key} fg="#7f849c"><u>{renderInlineNodes(node.children)}</u></span>;
      case "code":
        return <Pre key={key} children={node.children} searchQuery={searchQuery} />;
      case "break":
        return null;
    }
  });

  const flushInline = (anchorId?: string) => {
    if (inlineBuffer.length === 0) return;
    const resolvedAnchorId = anchorId ?? bufferAnchorId;
    result.push(
      <box
        key={`merged-${keyCounter++}`}
        {...(resolvedAnchorId ? { id: resolvedAnchorId } : {})}
        paddingLeft={bufferIndent}
        shouldFill={true}
      >
        <text fg="#a6adc8" wrapMode="word">{inlineBuffer}</text>
      </box>,
    );
    inlineBuffer = [];
    bufferAnchorId = undefined;
  };

  const beginInlineBlock = (indent: number, anchorId?: string) => {
    if (inlineBuffer.length > 0 && indent !== bufferIndent) flushInline();
    if (inlineBuffer.length === 0) {
      bufferIndent = indent;
      bufferAnchorId = anchorId;
    }
  };

  const appendInlineLines = (nodes: InlineNode[]) => {
    for (const segment of splitByBreak(nodes)) {
      inlineBuffer.push(...renderInlineNodes(segment), "\n");
    }
  };

  const beginDefinitionLine = (indent: number, anchorId?: string) => {
    // Definition terms and descriptions use different indentation buffers.
    // A trailing newline before that buffer boundary would create an extra
    // blank row in addition to mandoc's explicit spacer.
    if (
      inlineBuffer.length > 0
      && indent !== bufferIndent
      && inlineBuffer[inlineBuffer.length - 1] === "\n"
    ) {
      inlineBuffer.pop();
    }
    beginInlineBlock(indent, anchorId);
  };

  for (let blockIndex = 0; blockIndex < blocks.length; blockIndex++) {
    const block = blocks[blockIndex]!;
    const isActiveBlock = sectionId !== undefined && blockIndex === activeBlockIndex;
    if (isActiveBlock) flushInline();

    switch (block.type) {
      case "paragraph": {
        beginInlineBlock(baseIndent + block.indent);
        appendInlineLines(block.children);
        if (isActiveBlock) flushInline(contentBlockId(sectionId, blockIndex));
        break;
      }
      case "list": {
        beginInlineBlock(baseIndent + block.indent);
        for (const item of block.items) {
          inlineBuffer.push(<span key={`bullet-${inlineKey++}`} fg="#94e2d5">{"• "}</span>);
          inlineBuffer.push(...renderInlineNodes(item), "\n");
        }
        if (isActiveBlock) flushInline(contentBlockId(sectionId, blockIndex));
        break;
      }
      case "definition-list": {
        let anchorId = isActiveBlock
          ? contentBlockId(sectionId, blockIndex)
          : undefined;
        for (const item of block.items) {
          for (const term of item.terms) {
            beginDefinitionLine(baseIndent + block.indent, anchorId);
            anchorId = undefined;
            appendInlineLines(term);
          }
          if (item.description.length > 0) {
            beginDefinitionLine(baseIndent + block.indent + 4, anchorId);
            anchorId = undefined;
            appendInlineLines(item.description);
          }
        }
        if (inlineBuffer[inlineBuffer.length - 1] === "\n") inlineBuffer.pop();
        if (isActiveBlock) flushInline();
        break;
      }
      case "pre": {
        flushInline();
        const pre = (
          <Pre
            key={`pre-${keyCounter++}`}
            children={block.children}
            block
            indent={baseIndent + block.indent}
            searchQuery={searchQuery}
          />
        );
        result.push(
          isActiveBlock
            ? <box key={`pre-anchor-${keyCounter++}`} id={contentBlockId(sectionId, blockIndex)}>{pre}</box>
            : pre,
        );
        // Both renderers visually separate a display block from following
        // prose. Avoid adding a second row when parser output already has one.
        if (blocks[blockIndex + 1]?.type !== "spacer" && blockIndex < blocks.length - 1) {
          result.push(<box key={`pre-gap-${keyCounter++}`} height={1} />);
        }
        break;
      }
      case "spacer":
        flushInline();
        result.push(<box key={`spacer-${keyCounter++}`} height={1} />);
        break;
    }
  }
  flushInline();
  return result;
}

export interface SectionContentProps {
  node: SectionNode;
  baseIndent?: number;
  searchQuery?: string;
  activeSearchSectionId?: string | undefined;
  activeBlockIndex?: number | undefined;
  headingIndent?: number;
}

/** Recursively renders one section and its children in document order. */
export function SectionContent({
  node,
  baseIndent = 3,
  searchQuery = "",
  activeSearchSectionId,
  activeBlockIndex,
  headingIndent = 0,
}: SectionContentProps) {
  return (
    <box flexDirection="column" gap={0}>
      <box paddingLeft={headingIndent}>
        <text id={contentId(node.id)} fg="#94e2d5">
          <b>{renderSearchHighlights(node.title, searchQuery, `heading-${node.id}`)}</b>
        </text>
      </box>
      {renderBlockNodes(
        node.blocks,
        baseIndent,
        searchQuery,
        node.id,
        activeSearchSectionId === node.id ? activeBlockIndex : undefined,
      )}
      <box flexDirection="column" gap={0}>
        {node.children.map((child) => (
          <SectionContent
            key={child.id}
            node={child}
            baseIndent={baseIndent + 4}
            searchQuery={searchQuery}
            activeSearchSectionId={activeSearchSectionId}
            activeBlockIndex={activeBlockIndex}
            headingIndent={headingIndent + 4}
          />
        ))}
      </box>
    </box>
  );
}

function TldrCommand({ parts, searchQuery }: { parts: TldrCommandPart[]; searchQuery: string }) {
  return (
    <text fg="#cdd6f4" wrapMode="char">
      {parts.map((part, index) => (
        <span key={index} fg={part.type === "placeholder" ? "#f9e2af" : "#cdd6f4"}>
          {renderSearchHighlights(part.content, searchQuery, `tldr-command-${index}`)}
        </span>
      ))}
    </text>
  );
}

/** Renders cached community examples before the authoritative man page. */
export function TldrQuickReference({ page, searchQuery }: { page: TldrPage; searchQuery: string }) {
  return (
    <box
      id={contentId(TLDR_NAV_ID)}
      flexDirection="column"
      backgroundColor="#28243a"
      border={["top", "right", "bottom", "left"]}
      borderColor="#cba6f7"
      paddingLeft={1}
      paddingRight={1}
      paddingTop={1}
      paddingBottom={1}
    >
      <text fg="#cba6f7">
        <b>{renderSearchHighlights(`TLDR QUICK REFERENCE · ${page.title}`, searchQuery, "tldr-title")}</b>
      </text>
      {page.description.map((line, index) => (
        <text key={`description-${index}`} fg="#bac2de" wrapMode="word">
          {renderSearchHighlights(line, searchQuery, `tldr-description-${index}`)}
        </text>
      ))}
      {page.examples.map((example, index) => (
        <box key={`example-${index}`} flexDirection="column" paddingTop={index === 0 ? 1 : 0}>
          <text fg="#a6e3a1" wrapMode="word">
            {renderSearchHighlights(example.description, searchQuery, `tldr-example-${index}`)}
          </text>
          {example.command && (
            <box paddingLeft={2}>
              <TldrCommand parts={example.commandParts} searchQuery={searchQuery} />
            </box>
          )}
        </box>
      ))}
      {page.moreInformation && (
        <text fg="#89b4fa" wrapMode="char">
          {renderSearchHighlights(`More information: ${page.moreInformation}`, searchQuery, "tldr-more-information")}
        </text>
      )}
      <text fg="#7f849c">{`tldr-pages · CC BY 4.0 · ${page.platform} · ${page.language}`}</text>
    </box>
  );
}
