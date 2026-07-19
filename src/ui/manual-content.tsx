/**
 * @file Renders the versioned native document and optional tldr reference.
 *
 * Rust owns document semantics. This module performs presentation-only work:
 * terminal indentation, colors, wrapping, stable search anchors, and styling.
 */

import { memo, type ReactNode } from "react";
import type {
  MantBlock,
  MantInline,
  MantSection,
  TldrCommandPart,
  TldrDocument,
} from "../native";
import {
  contentAnchorId,
  contentId,
  contentSearchId,
  TLDR_NAV_ID,
} from "./ids";
import { Pre } from "./Pre";
import { searchPath, visibleInlineSegments } from "./search";

// ── Inline layout ─────────────────────────────────────────────

function layoutIndent(block: MantBlock): number {
  return block.type === "vertical-space"
    ? 0
    : block.layout?.indentColumns ?? 0;
}

function layoutSpacing(block: MantBlock): number {
  return block.type === "vertical-space"
    ? 0
    : Math.max(0, Math.floor(block.layout?.spacingBeforeLines ?? 0));
}

/** Renders inline semantics without changing the visible source text. */
function renderInlineContent(nodes: MantInline[], keyPrefix: string): ReactNode[] {
  let keyCounter = 0;
  const renderNodes = (children: MantInline[]): ReactNode[] => children.map((node) => {
    const key = `${keyPrefix}-${keyCounter++}`;
    switch (node.type) {
      case "text":
        return node.value;
      case "strong":
        return <span key={key} fg="#cdd6f4"><b>{renderNodes(node.children)}</b></span>;
      case "emphasis":
        return <span key={key} fg="#7f849c"><i>{renderNodes(node.children)}</i></span>;
      case "code":
        return <span key={key} fg="#94e2d5">{node.value}</span>;
      case "external-link":
      case "email-link":
        return <span key={key} fg="#89b4fa"><u>{renderNodes(node.children)}</u></span>;
      case "manual-reference":
        return <span key={key} fg="#89dceb">{renderNodes(node.children)}</span>;
      case "section-reference":
        return <span key={key} fg="#89dceb"><u>{renderNodes(node.children)}</u></span>;
      case "anchor":
        return null;
      case "line-break":
        return "\n";
    }
  });
  return renderNodes(nodes);
}

// ── Native block renderer ───────────────────────────────────────

/**
 * Merges adjacent prose into larger TextBuffers while retaining structural
 * blocks. Search records use the same grouping and point at these stable IDs.
 */
function renderBlockNodes(
  blocks: MantBlock[],
  baseIndent: number,
  sectionId: string,
  parentPath: string,
  onNavigateInternal?: (target: string) => void,
): ReactNode[] {
  const result: ReactNode[] = [];
  let inlineBuffer: ReactNode[] = [];
  let bufferIndent = 0;
  let bufferAnchorId: string | undefined;
  let keyCounter = 0;
  let inlineKey = 0;

  const flushInline = () => {
    // The final newline is a separator sentinel, not an empty terminal row.
    if (inlineBuffer[inlineBuffer.length - 1] === "\n") inlineBuffer.pop();
    if (inlineBuffer.length === 0) {
      bufferAnchorId = undefined;
      return;
    }
    result.push(
      <box
        key={`merged-${keyCounter++}`}
        {...(bufferAnchorId ? { id: bufferAnchorId } : {})}
        paddingLeft={bufferIndent}
        shouldFill={true}
      >
        <text fg="#a6adc8" wrapMode="word">{inlineBuffer}</text>
      </box>,
    );
    inlineBuffer = [];
    bufferAnchorId = undefined;
  };

  const beginInlineBlock = (indent: number, anchorId: string) => {
    if (inlineBuffer.length > 0 && indent !== bufferIndent) flushInline();
    if (inlineBuffer.length === 0) {
      bufferIndent = indent;
      bufferAnchorId = anchorId;
    }
  };

  const appendInlineLines = (nodes: MantInline[]) => {
    for (const segment of visibleInlineSegments(nodes)) {
      inlineBuffer.push(
        ...renderInlineContent(segment, `merged-${inlineKey++}`),
        "\n",
      );
    }
  };

  /**
   * Interactive references need separate Text renderables for mouse events.
   * Each visible fragment therefore receives its own search record and ID.
   */
  const renderNavigationParagraph = (
    nodes: MantInline[],
    blockPath: string,
  ): ReactNode[] => {
    const rendered: ReactNode[] = [];
    let ordinary: MantInline[] = [];
    let searchPartIndex = 0;
    const nextSearchId = () => contentSearchId(
      sectionId,
      searchPath.inline(blockPath, searchPartIndex++),
    );
    const flushOrdinary = () => {
      if (ordinary.length === 0) return;
      const current = ordinary;
      ordinary = [];
      rendered.push(
        <text
          key={`navigation-text-${inlineKey++}`}
          id={nextSearchId()}
          fg="#a6adc8"
          wrapMode="word"
        >
          {renderInlineContent(current, `navigation-${inlineKey}`)}
        </text>,
      );
    };

    for (const node of nodes) {
      if (node.type === "section-reference") {
        flushOrdinary();
        rendered.push(
          <text
            key={`section-reference-${inlineKey++}`}
            id={nextSearchId()}
            fg="#89dceb"
            wrapMode="word"
            selectable={false}
            onMouseDown={(event) => {
              event.stopPropagation();
              onNavigateInternal?.(node.target);
            }}
          >
            <u>{renderInlineContent(node.children, `reference-${inlineKey}`)}</u>
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
    const blockPath = searchPath.block(parentPath, blockIndex);
    const targetId = contentSearchId(sectionId, blockPath);
    const indent = baseIndent + layoutIndent(block);
    const spacingBefore = layoutSpacing(block);

    if (spacingBefore > 0) {
      flushInline();
      result.push(<box key={`leading-space-${keyCounter++}`} height={spacingBefore} />);
    }

    switch (block.type) {
      case "paragraph":
        if (block.children.some((node) =>
          node.type === "section-reference" || node.type === "anchor"
        )) {
          flushInline();
          result.push(
            <box
              key={`navigation-paragraph-${keyCounter++}`}
              flexDirection="row"
              flexWrap="wrap"
              paddingLeft={indent}
            >
              {renderNavigationParagraph(block.children, blockPath)}
            </box>,
          );
        } else {
          beginInlineBlock(indent, targetId);
          appendInlineLines(block.children);
        }
        break;

      case "preformatted":
        flushInline();
        result.push(
          <box key={`pre-${keyCounter++}`} id={targetId}>
            <Pre children={block.children} block indent={indent} />
          </box>,
        );
        break;

      case "list":
        flushInline();
        result.push(
          <box key={`list-${keyCounter++}`} flexDirection="column">
            {block.items.map((item, itemIndex) => {
              const itemPath = searchPath.listItem(blockPath, itemIndex);
              const marker = block.kind === "ordered"
                ? `${(block.start ?? 1) + itemIndex}. `
                : block.kind === "bullet" ? "• " : "";
              return (
                <box
                  key={`item-${itemIndex}`}
                  flexDirection="row"
                  paddingLeft={indent}
                  paddingTop={itemIndex > 0 && !block.compact ? 1 : 0}
                >
                  {marker && <text fg="#94e2d5">{marker}</text>}
                  <box flexDirection="column" flexGrow={1}>
                    {renderBlockNodes(
                      item.blocks,
                      0,
                      sectionId,
                      itemPath,
                      onNavigateInternal,
                    )}
                  </box>
                </box>
              );
            })}
          </box>,
        );
        break;

      case "definition-list":
        flushInline();
        result.push(
          <box key={`definitions-${keyCounter++}`} flexDirection="column">
            {block.items.map((item, itemIndex) => {
              const itemPath = searchPath.definition(blockPath, itemIndex);
              return (
                <box
                  key={`definition-${itemIndex}`}
                  flexDirection="column"
                  paddingTop={
                    item.spacingBeforeLines
                      ?? (itemIndex > 0 && !block.compact ? 1 : 0)
                  }
                >
                  {item.terms.map((term, termIndex) => {
                    const termPath = searchPath.term(itemPath, termIndex);
                    return (
                      <box key={`term-${termIndex}`} paddingLeft={indent}>
                        <text
                          id={contentSearchId(sectionId, termPath)}
                          fg="#cdd6f4"
                          wrapMode="word"
                        >
                          {renderInlineContent(term, `term-${termIndex}`)}
                        </text>
                      </box>
                    );
                  })}
                  {item.description.length > 0 && (
                    <box flexDirection="column">
                      {renderBlockNodes(
                        item.description,
                        indent + 4,
                        sectionId,
                        itemPath,
                        onNavigateInternal,
                      )}
                    </box>
                  )}
                </box>
              );
            })}
          </box>,
        );
        break;

      case "table":
        flushInline();
        result.push(
          <box key={`table-${keyCounter++}`} flexDirection="column" paddingLeft={indent}>
            {block.rows.map((row, rowIndex) => (
              <box key={`row-${rowIndex}`} flexDirection="row">
                {row.cells.map((cell, cellIndex) => (
                  <box key={`cell-${cellIndex}`} flexGrow={1} flexDirection="column">
                    {renderBlockNodes(
                      cell.blocks,
                      0,
                      sectionId,
                      searchPath.cell(searchPath.row(blockPath, rowIndex), cellIndex),
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
          <box key={`equation-${keyCounter++}`} paddingLeft={indent}>
            <text id={targetId} fg="#f9e2af" wrapMode="char">{block.value}</text>
          </box>,
        );
        break;

      case "unsupported":
        flushInline();
        result.push(
          <box key={`unsupported-${keyCounter++}`} paddingLeft={indent}>
            <text id={targetId} fg="#fab387" wrapMode="word">{block.text}</text>
          </box>,
        );
        break;

      case "vertical-space":
        flushInline();
        result.push(
          <box key={`space-${keyCounter++}`} height={Math.max(1, Math.floor(block.lines))} />,
        );
        break;
    }
  }
  flushInline();
  return result;
}

// ── Section hierarchy ───────────────────────────────────────────

export interface SectionContentProps {
  node: MantSection;
  baseIndent?: number;
  headingIndent?: number;
  onNavigateInternal?: ((target: string) => void) | undefined;
}

/** Recursively renders one section and its children in document order. */
function SectionContentView({
  node,
  baseIndent = 3,
  headingIndent = 0,
  onNavigateInternal,
}: SectionContentProps) {
  return (
    <box
      flexDirection="column"
      gap={0}
      paddingTop={Math.max(0, Math.floor(node.spacingBeforeLines ?? 0))}
    >
      <box paddingLeft={headingIndent}>
        <text id={contentId(node.id)} fg="#94e2d5"><b>{node.title}</b></text>
      </box>
      {renderBlockNodes(node.blocks, baseIndent, node.id, "", onNavigateInternal)}
      <box flexDirection="column" gap={0}>
        {node.children.map((child) => (
          <SectionContent
            key={child.id}
            node={child}
            baseIndent={baseIndent + 4}
            headingIndent={headingIndent + 4}
            onNavigateInternal={onNavigateInternal}
          />
        ))}
      </box>
    </box>
  );
}

function sectionContentPropsEqual(previous: SectionContentProps, next: SectionContentProps): boolean {
  return previous.node === next.node
    && previous.baseIndent === next.baseIndent
    && previous.headingIndent === next.headingIndent;
}

/** App state changes do not rebuild immutable manual sections. */
export const SectionContent = memo(SectionContentView, sectionContentPropsEqual);

// ── TLDR quick reference ───────────────────────────────────────

function TldrCommand({ parts }: { parts: TldrCommandPart[] }) {
  return (
    <text fg="#cdd6f4" wrapMode="char">
      {parts.map((part, index) => (
        <span key={index} fg={part.type === "placeholder" ? "#f9e2af" : "#cdd6f4"}>
          {part.value}
        </span>
      ))}
    </text>
  );
}

/** Renders cached community examples before the authoritative man page. */
function TldrQuickReferenceView({ page }: { page: TldrDocument }) {
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
      <text fg="#cba6f7"><b>{`TLDR QUICK REFERENCE · ${page.title}`}</b></text>
      {page.description.map((line, index) => (
        <text
          key={`description-${index}`}
          id={contentSearchId(TLDR_NAV_ID, searchPath.tldrDescription(index))}
          fg="#bac2de"
          wrapMode="word"
        >
          {line}
        </text>
      ))}
      {page.examples.map((example, index) => (
        <box key={`example-${index}`} flexDirection="column" paddingTop={1}>
          <text
            id={contentSearchId(TLDR_NAV_ID, searchPath.tldrExampleDescription(index))}
            fg="#a6e3a1"
            wrapMode="word"
          >
            {example.description}
          </text>
          {example.command && (
            <box
              id={contentSearchId(TLDR_NAV_ID, searchPath.tldrExampleCommand(index))}
              paddingLeft={2}
            >
              <TldrCommand parts={example.commandParts} />
            </box>
          )}
        </box>
      ))}
      {page.moreInformation && (
        <box paddingTop={1}>
          <text
            id={contentSearchId(TLDR_NAV_ID, searchPath.tldrMoreInformation())}
            fg="#89b4fa"
            wrapMode="char"
          >
            {`More information: ${page.moreInformation}`}
          </text>
        </box>
      )}
      <text fg="#7f849c">{`tldr-pages · CC BY 4.0 · ${page.platform} · ${page.language}`}</text>
    </box>
  );
}

/** TLDR is immutable for the lifetime of one page. */
export const TldrQuickReference = memo(TldrQuickReferenceView);
