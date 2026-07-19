/**
 * @file Renders the versioned native document and optional tldr reference.
 *
 * Rust owns document semantics. This module performs presentation-only work:
 * terminal indentation, colors, wrapping, search anchors, and syntax styling.
 */

import type { ReactNode } from "react";
import type {
  MantBlock,
  MantInline,
  MantSection,
  TldrCommandPart,
  TldrDocument,
} from "../native";
import { contentAnchorId, contentBlockId, contentId, TLDR_NAV_ID } from "./ids";
import { Pre } from "./Pre";
import { renderSearchHighlights } from "./search-highlight";

// ── Inline layout ─────────────────────────────────────────────────────────

function splitByBreak(nodes: MantInline[]): MantInline[][] {
  const segments: MantInline[][] = [[]];
  for (const node of nodes) {
    if (node.type === "line-break") segments.push([]);
    else segments[segments.length - 1]!.push(node);
  }
  return segments
    .filter((segment) => segment.length > 0)
    .map(trimSegmentWhitespace);
}

/** Removes formatter boundary whitespace without mutating native AST nodes. */
function trimSegmentWhitespace(nodes: MantInline[]): MantInline[] {
  if (nodes.length === 0) return nodes;

  const trimmed = nodes.map((node) => node.type === "text" ? { ...node } : node);
  const first = trimmed[0];
  if (first?.type === "text") first.value = first.value.replace(/^\s+/, "");
  const last = trimmed[trimmed.length - 1];
  if (last?.type === "text") last.value = last.value.replace(/\s+$/, "");
  return trimmed;
}

function layoutIndent(block: MantBlock): number {
  return block.type === "vertical-space"
    ? 0
    : block.layout?.indentColumns ?? 0;
}

// ── Native block renderer ─────────────────────────────────────────────────

/**
 * Merges adjacent prose into larger TextBuffers while retaining structural
 * blocks. Large manuals otherwise create thousands of tiny OpenTUI buffers.
 */
function renderBlockNodes(
  blocks: MantBlock[],
  baseIndent = 0,
  searchQuery = "",
  sectionId?: string,
  activeBlockIndex?: number,
  onNavigateInternal?: (target: string) => void,
): ReactNode[] {
  const result: ReactNode[] = [];
  let inlineBuffer: ReactNode[] = [];
  let bufferIndent = 0;
  let bufferAnchorId: string | undefined;
  let keyCounter = 0;
  let inlineKey = 0;

  const renderInlineNodes = (nodes: MantInline[]): ReactNode[] => nodes.map((node) => {
    const key = inlineKey++;
    switch (node.type) {
      case "text":
        return renderSearchHighlights(node.value, searchQuery, `inline-${key}`);
      case "strong":
        return <span key={key} fg="#cdd6f4"><b>{renderInlineNodes(node.children)}</b></span>;
      case "emphasis":
        return <span key={key} fg="#7f849c"><u>{renderInlineNodes(node.children)}</u></span>;
      case "code":
        return (
          <span key={key} fg="#94e2d5">
            {renderSearchHighlights(node.value, searchQuery, `code-${key}`)}
          </span>
        );
      case "external-link":
        return <span key={key} fg="#89b4fa"><u>{renderInlineNodes(node.children)}</u></span>;
      case "email-link":
        return <span key={key} fg="#89b4fa"><u>{renderInlineNodes(node.children)}</u></span>;
      case "manual-reference":
        return <span key={key} fg="#89dceb">{renderInlineNodes(node.children)}</span>;
      case "section-reference":
        return <span key={key} fg="#89dceb"><u>{renderInlineNodes(node.children)}</u></span>;
      case "anchor":
        return null;
      case "line-break":
        return "\n";
    }
  });

  const flushInline = (anchorId?: string) => {
    // The final newline is a separator sentinel, not an empty terminal row.
    if (inlineBuffer[inlineBuffer.length - 1] === "\n") inlineBuffer.pop();
    if (inlineBuffer.length === 0) {
      bufferAnchorId = undefined;
      return;
    }
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

  const appendInlineLines = (nodes: MantInline[]) => {
    for (const segment of splitByBreak(nodes)) {
      inlineBuffer.push(...renderInlineNodes(segment), "\n");
    }
  };

  /**
   * Spans inside one OpenTUI text buffer cannot receive mouse events. Native
   * navigation paragraphs are rare, so render only their top-level reference
   * and anchor nodes as independent Text renderables in one wrapping row.
   */
  const renderNavigationParagraph = (nodes: MantInline[]): ReactNode[] => {
    const rendered: ReactNode[] = [];
    let ordinary: MantInline[] = [];
    const flushOrdinary = () => {
      if (ordinary.length === 0) return;
      const current = ordinary;
      ordinary = [];
      rendered.push(
        <text key={`navigation-text-${inlineKey++}`} fg="#a6adc8" wrapMode="word">
          {renderInlineNodes(current)}
        </text>,
      );
    };

    for (const node of nodes) {
      if (node.type === "section-reference") {
        flushOrdinary();
        rendered.push(
          <text
            key={`section-reference-${inlineKey++}`}
            fg="#89dceb"
            wrapMode="word"
            selectable={false}
            onMouseDown={(event) => {
              event.stopPropagation();
              onNavigateInternal?.(node.target);
            }}
          >
            <u>{renderInlineNodes(node.children)}</u>
          </text>,
        );
      } else if (node.type === "anchor") {
        flushOrdinary();
        rendered.push(
          <text key={`anchor-${inlineKey++}`} id={contentAnchorId(node.id)} selectable={false}>
            {"\u200b"}
          </text>,
        );
      } else {
        ordinary.push(node);
      }
    }
    flushOrdinary();
    return rendered;
  };

  for (let blockIndex = 0; blockIndex < blocks.length; blockIndex++) {
    const block = blocks[blockIndex]!;
    const isActiveBlock = sectionId !== undefined && blockIndex === activeBlockIndex;
    const anchorId = isActiveBlock ? contentBlockId(sectionId, blockIndex) : undefined;
    const indent = baseIndent + layoutIndent(block);
    if (isActiveBlock) flushInline();

    switch (block.type) {
      case "paragraph":
        if (block.children.some((node) =>
          node.type === "section-reference" || node.type === "anchor"
        )) {
          flushInline();
          result.push(
            <box
              key={`navigation-paragraph-${keyCounter++}`}
              {...(anchorId ? { id: anchorId } : {})}
              flexDirection="row"
              flexWrap="wrap"
              paddingLeft={indent}
            >
              {renderNavigationParagraph(block.children)}
            </box>,
          );
        } else {
          beginInlineBlock(indent, anchorId);
          appendInlineLines(block.children);
          if (isActiveBlock) flushInline();
        }
        break;

      case "preformatted": {
        flushInline();
        const pre = (
          <Pre
            children={block.children}
            block
            indent={indent}
            searchQuery={searchQuery}
          />
        );
        result.push(
          anchorId
            ? <box key={`pre-${keyCounter++}`} id={anchorId}>{pre}</box>
            : <box key={`pre-${keyCounter++}`}>{pre}</box>,
        );
        break;
      }

      case "list": {
        flushInline();
        result.push(
          <box
            key={`list-${keyCounter++}`}
            {...(anchorId ? { id: anchorId } : {})}
            flexDirection="column"
          >
            {block.items.map((item, itemIndex) => {
              const marker = block.kind === "ordered"
                ? `${(block.start ?? 1) + itemIndex}. `
                : block.kind === "bullet" ? "• " : "";
              return (
                <box key={`item-${itemIndex}`} flexDirection="row" paddingLeft={indent}>
                  {marker && <text fg="#94e2d5">{marker}</text>}
                  <box flexDirection="column" flexGrow={1}>
                    {renderBlockNodes(
                      item.blocks,
                      0,
                      searchQuery,
                      undefined,
                      undefined,
                      onNavigateInternal,
                    )}
                  </box>
                </box>
              );
            })}
          </box>,
        );
        break;
      }

      case "definition-list": {
        flushInline();
        result.push(
          <box
            key={`definitions-${keyCounter++}`}
            {...(anchorId ? { id: anchorId } : {})}
            flexDirection="column"
          >
            {block.items.map((item, itemIndex) => (
              <box key={`definition-${itemIndex}`} flexDirection="column">
                {item.terms.map((term, termIndex) => (
                  <box key={`term-${termIndex}`} paddingLeft={indent}>
                    <text fg="#cdd6f4" wrapMode="word">{renderInlineNodes(term)}</text>
                  </box>
                ))}
                {item.description.length > 0 && (
                  <box flexDirection="column">
                    {renderBlockNodes(
                      item.description,
                      indent + 4,
                      searchQuery,
                      undefined,
                      undefined,
                      onNavigateInternal,
                    )}
                  </box>
                )}
              </box>
            ))}
          </box>,
        );
        break;
      }

      case "table":
        flushInline();
        result.push(
          <box
            key={`table-${keyCounter++}`}
            {...(anchorId ? { id: anchorId } : {})}
            flexDirection="column"
            paddingLeft={indent}
          >
            {block.rows.map((row, rowIndex) => (
              <box key={`row-${rowIndex}`} flexDirection="row">
                {row.cells.map((cell, cellIndex) => (
                  <box key={`cell-${cellIndex}`} flexGrow={1} flexDirection="column">
                    {renderBlockNodes(
                      cell.blocks,
                      0,
                      searchQuery,
                      undefined,
                      undefined,
                      onNavigateInternal,
                    )}
                  </box>
                ))}
              </box>
            ))}
          </box>,
        );
        break;

      case "equation":
        flushInline();
        result.push(
          <box key={`equation-${keyCounter++}`} {...(anchorId ? { id: anchorId } : {})} paddingLeft={indent}>
            <text fg="#f9e2af" wrapMode="char">{block.value}</text>
          </box>,
        );
        break;

      case "unsupported":
        flushInline();
        result.push(
          <box key={`unsupported-${keyCounter++}`} {...(anchorId ? { id: anchorId } : {})} paddingLeft={indent}>
            <text fg="#fab387" wrapMode="word">{block.text}</text>
          </box>,
        );
        break;

      case "vertical-space":
        flushInline();
        result.push(
          <box
            key={`space-${keyCounter++}`}
            {...(anchorId ? { id: anchorId } : {})}
            height={Math.max(1, Math.floor(block.lines))}
          />,
        );
        break;
    }
  }
  flushInline();
  return result;
}

// ── Section hierarchy ─────────────────────────────────────────────────────

export interface SectionContentProps {
  node: MantSection;
  baseIndent?: number;
  searchQuery?: string;
  activeSearchSectionId?: string | undefined;
  activeBlockIndex?: number | undefined;
  headingIndent?: number;
  onNavigateInternal?: ((target: string) => void) | undefined;
}

/** Recursively renders one section and its children in document order. */
export function SectionContent({
  node,
  baseIndent = 3,
  searchQuery = "",
  activeSearchSectionId,
  activeBlockIndex,
  headingIndent = 0,
  onNavigateInternal,
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
        onNavigateInternal,
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
            onNavigateInternal={onNavigateInternal}
          />
        ))}
      </box>
    </box>
  );
}

// ── TLDR quick reference ──────────────────────────────────────────────────

function TldrCommand({ parts, searchQuery }: {
  parts: TldrCommandPart[];
  searchQuery: string;
}) {
  return (
    <text fg="#cdd6f4" wrapMode="char">
      {parts.map((part, index) => (
        <span key={index} fg={part.type === "placeholder" ? "#f9e2af" : "#cdd6f4"}>
          {renderSearchHighlights(part.value, searchQuery, `tldr-command-${index}`)}
        </span>
      ))}
    </text>
  );
}

/** Renders cached community examples before the authoritative man page. */
export function TldrQuickReference({ page, searchQuery }: {
  page: TldrDocument;
  searchQuery: string;
}) {
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
